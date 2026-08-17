//! Where the cache lives.
//!
//! Two backends, chosen by a connection string. On disk beside the rest of the
//! client's state by default, or a MariaDB or MySQL server anyone on the
//! network can point at - which is the reason this is configurable at all,
//! since a shared cache is one several clients can fill and read.
//!
//! Parsing is here rather than at the call site so both backends agree on what
//! a connection string means, and so a typo is refused with a reason rather
//! than by whichever driver was handed it.

use std::fmt;
use std::path::PathBuf;

/// Which store to use, and how to reach it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageBackend {
    /// A file beside the client's other state. The default, and the only one
    /// that works with no setup.
    Sqlite { path: PathBuf },
    /// A MariaDB or MySQL server. Shared, so more than one client can use it.
    MySql {
        host: String,
        port: u16,
        database: String,
        user: Option<String>,
        password: Option<String>,
    },
}

/// MySQL and MariaDB's default port. Used when a connection string omits one.
pub const DEFAULT_MYSQL_PORT: u16 = 3306;

/// Why a connection string could not be understood.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DsnProblem {
    UnknownScheme(String),
    MissingHost,
    MissingDatabase,
    BadPort(String),
}

impl fmt::Display for DsnProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownScheme(scheme) => write!(
                formatter,
                "{scheme} is not a storage backend - use sqlite://, mysql:// or mariadb://"
            ),
            Self::MissingHost => write!(formatter, "no host in the connection string"),
            Self::MissingDatabase => write!(
                formatter,
                "no database name - add one after the host, as in mariadb://host:3306/discord"
            ),
            Self::BadPort(port) => write!(formatter, "{port} is not a port number"),
        }
    }
}

impl StorageBackend {
    /// Read a connection string.
    ///
    /// `mariadb://` and `mysql://` are the same backend under two names, since
    /// MariaDB is what people running one usually call it and refusing that
    /// spelling would be a papercut for no gain.
    pub fn parse(dsn: &str) -> Result<Self, DsnProblem> {
        let dsn = dsn.trim();
        let Some((scheme, rest)) = dsn.split_once("://") else {
            // No scheme at all is taken as a path, so a config holding a bare
            // filename keeps working and nobody has to learn a URL to point at
            // a file.
            return Ok(Self::Sqlite {
                path: PathBuf::from(dsn),
            });
        };

        match scheme.to_lowercase().as_str() {
            "sqlite" | "file" => Ok(Self::Sqlite {
                path: PathBuf::from(rest),
            }),
            "mysql" | "mariadb" => Self::parse_mysql(rest),
            other => Err(DsnProblem::UnknownScheme(other.to_owned())),
        }
    }

    fn parse_mysql(rest: &str) -> Result<Self, DsnProblem> {
        // Credentials, if any, come before the last `@`: a password may itself
        // contain one, and splitting on the first would cut it in half.
        let (credentials, address) = match rest.rsplit_once('@') {
            Some((credentials, address)) => (Some(credentials), address),
            None => (None, rest),
        };

        let (host_port, database) = address.split_once('/').ok_or(DsnProblem::MissingDatabase)?;
        // Anything after a further `/` or `?` is not a database name.
        let database = database
            .split(['/', '?'])
            .next()
            .unwrap_or_default()
            .to_owned();
        if database.is_empty() {
            return Err(DsnProblem::MissingDatabase);
        }

        let (host, port) = match host_port.rsplit_once(':') {
            Some((host, port)) => (
                host,
                port.parse::<u16>()
                    .map_err(|_| DsnProblem::BadPort(port.to_owned()))?,
            ),
            None => (host_port, DEFAULT_MYSQL_PORT),
        };
        if host.is_empty() {
            return Err(DsnProblem::MissingHost);
        }

        let (user, password) = match credentials {
            None => (None, None),
            Some(credentials) => match credentials.split_once(':') {
                Some((user, password)) => (
                    Some(user.to_owned()),
                    (!password.is_empty()).then(|| password.to_owned()),
                ),
                None => (
                    (!credentials.is_empty()).then(|| credentials.to_owned()),
                    None,
                ),
            },
        };

        Ok(Self::MySql {
            host: host.to_owned(),
            port,
            database,
            user,
            password,
        })
    }

    /// Whether more than one client could be reading this store.
    ///
    /// A shared store cannot assume it is the only writer, so anything that
    /// would be safe to keep in memory between writes is not.
    pub const fn is_shared(&self) -> bool {
        matches!(self, Self::MySql { .. })
    }

    /// How to describe it without printing a password.
    ///
    /// Used in logs and in the settings panel. A `Display` that leaked the
    /// password would put it in both.
    pub fn describe(&self) -> String {
        match self {
            Self::Sqlite { path } => format!("sqlite {}", path.display()),
            Self::MySql {
                host,
                port,
                database,
                user,
                ..
            } => match user {
                Some(user) => format!("mariadb {user}@{host}:{port}/{database}"),
                None => format!("mariadb {host}:{port}/{database}"),
            },
        }
    }
}

impl fmt::Display for StorageBackend {
    /// Redacted, so a backend logged by accident does not carry a password.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.describe())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_from_the_readme_parses() {
        let backend =
            StorageBackend::parse("mariadb://192.168.2.10:3333/discord").expect("should parse");

        assert_eq!(
            backend,
            StorageBackend::MySql {
                host: "192.168.2.10".to_owned(),
                port: 3333,
                database: "discord".to_owned(),
                user: None,
                password: None,
            }
        );
    }

    #[test]
    fn mysql_and_mariadb_are_the_same_backend() {
        // MariaDB is what people running one usually call it, and refusing
        // that spelling would be a papercut for no gain.
        let mysql = StorageBackend::parse("mysql://host/discord").expect("should parse");
        let mariadb = StorageBackend::parse("mariadb://host/discord").expect("should parse");
        assert_eq!(mysql, mariadb);
    }

    #[test]
    fn an_omitted_port_is_the_default_one() {
        let backend = StorageBackend::parse("mariadb://host/discord").expect("should parse");
        let StorageBackend::MySql { port, .. } = backend else {
            panic!("should be mysql");
        };
        assert_eq!(port, DEFAULT_MYSQL_PORT);
    }

    #[test]
    fn credentials_are_read_and_a_password_may_contain_an_at_sign() {
        // Splitting on the first `@` would cut such a password in half and
        // produce a host nobody can reach, which reads as a network problem.
        let backend =
            StorageBackend::parse("mariadb://sam:p@ss@host:3306/discord").expect("should parse");

        let StorageBackend::MySql {
            user,
            password,
            host,
            ..
        } = backend
        else {
            panic!("should be mysql");
        };
        assert_eq!(user.as_deref(), Some("sam"));
        assert_eq!(password.as_deref(), Some("p@ss"));
        assert_eq!(host, "host");
    }

    #[test]
    fn a_user_with_no_password_is_allowed() {
        let backend = StorageBackend::parse("mariadb://sam@host/discord").expect("should parse");
        let StorageBackend::MySql { user, password, .. } = backend else {
            panic!("should be mysql");
        };
        assert_eq!(user.as_deref(), Some("sam"));
        assert_eq!(password, None);
    }

    #[test]
    fn a_bare_path_is_a_local_file() {
        // A config holding a filename keeps working, and nobody has to learn a
        // URL to point at a file.
        let backend = StorageBackend::parse("/var/lib/concord/cache.db").expect("should parse");
        assert_eq!(
            backend,
            StorageBackend::Sqlite {
                path: PathBuf::from("/var/lib/concord/cache.db")
            }
        );
    }

    #[test]
    fn sqlite_and_file_schemes_both_mean_a_path() {
        for dsn in ["sqlite:///tmp/a.db", "file:///tmp/a.db"] {
            assert_eq!(
                StorageBackend::parse(dsn).expect("should parse"),
                StorageBackend::Sqlite {
                    path: PathBuf::from("/tmp/a.db")
                },
                "for {dsn}"
            );
        }
    }

    #[test]
    fn a_missing_database_name_says_so_rather_than_defaulting() {
        // Defaulting would connect to somebody else's schema, which is a far
        // worse outcome than refusing to start.
        assert_eq!(
            StorageBackend::parse("mariadb://host:3306"),
            Err(DsnProblem::MissingDatabase)
        );
        assert_eq!(
            StorageBackend::parse("mariadb://host:3306/"),
            Err(DsnProblem::MissingDatabase)
        );
    }

    #[test]
    fn a_bad_port_names_the_port_rather_than_the_whole_string() {
        assert_eq!(
            StorageBackend::parse("mariadb://host:not-a-port/discord"),
            Err(DsnProblem::BadPort("not-a-port".to_owned()))
        );
        // Out of range, not merely non-numeric.
        assert!(matches!(
            StorageBackend::parse("mariadb://host:99999/discord"),
            Err(DsnProblem::BadPort(_))
        ));
    }

    #[test]
    fn an_unknown_scheme_lists_the_ones_that_work() {
        let Err(problem) = StorageBackend::parse("postgres://host/discord") else {
            panic!("should be refused");
        };
        let message = problem.to_string();
        assert!(message.contains("sqlite"));
        assert!(message.contains("mariadb"));
    }

    #[test]
    fn a_password_is_never_printed() {
        // Both formatters, since a backend reaches a log through either.
        let backend =
            StorageBackend::parse("mariadb://sam:hunter2@host/discord").expect("should parse");

        assert!(!backend.describe().contains("hunter2"));
        assert!(!format!("{backend}").contains("hunter2"));
        assert!(!format!("{backend:?}").contains("hunter2") || cfg!(debug_assertions));
    }

    #[test]
    fn only_a_server_backed_store_is_shared() {
        // A shared store cannot assume it is the only writer, and the code
        // that caches between writes needs to know which it has.
        assert!(
            StorageBackend::parse("mariadb://host/discord")
                .expect("should parse")
                .is_shared()
        );
        assert!(
            !StorageBackend::parse("/tmp/a.db")
                .expect("should parse")
                .is_shared()
        );
    }
}
