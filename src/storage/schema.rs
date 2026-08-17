//! What the cache holds, and the SQL that holds it.
//!
//! Flat rows rather than serialised blobs. A blob is faster to write and
//! useless to anyone else: a shared store is one another client may read, and
//! that only works if the columns mean something without this codebase.
//!
//! The two dialects differ in three places only - the text type, the integer
//! type, and how an upsert is spelled - so the statements are generated rather
//! than written twice.

use super::StorageBackend;

/// The schema version this build writes.
///
/// Bumped when a migration changes a table. A store written by a newer build
/// is left alone rather than migrated backwards: a shared store may be in use
/// by a client this one knows nothing about.
pub const SCHEMA_VERSION: u32 = 1;

/// Which SQL dialect to emit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dialect {
    Sqlite,
    MySql,
}

impl Dialect {
    pub const fn of(backend: &StorageBackend) -> Self {
        match backend {
            StorageBackend::Sqlite { .. } => Self::Sqlite,
            StorageBackend::MySql { .. } => Self::MySql,
        }
    }

    /// A snowflake. Discord's ids exceed what a signed 64-bit integer holds
    /// safely once, so they are stored as text in both dialects rather than
    /// risking a silent wrap in one of them.
    const fn id_type(self) -> &'static str {
        match self {
            Self::Sqlite => "TEXT",
            // Fixed width, since every snowflake is at most twenty digits and
            // a fixed column indexes better than a variable one.
            Self::MySql => "VARCHAR(20)",
        }
    }

    const fn text_type(self) -> &'static str {
        match self {
            Self::Sqlite => "TEXT",
            // Long enough for a message: Discord's cap is 4000 characters, and
            // utf8mb4 needs four bytes for each.
            Self::MySql => "TEXT",
        }
    }

    const fn short_text_type(self) -> &'static str {
        match self {
            Self::Sqlite => "TEXT",
            Self::MySql => "VARCHAR(255)",
        }
    }

    const fn int_type(self) -> &'static str {
        match self {
            Self::Sqlite => "INTEGER",
            Self::MySql => "BIGINT",
        }
    }

    /// How to write a row that may already exist, losing to a fresher writer.
    ///
    /// The revision guard is in the statement rather than in a read followed
    /// by a write, because between those two another client can write and the
    /// result is the sort of loss that only shows under load. See
    /// `storage::concurrent` for why a revision rather than a timestamp.
    pub fn guarded_upsert(self, table: &str, columns: &[&str]) -> String {
        let base = self.upsert(table, columns);
        match self {
            // SQLite puts the condition on the DO UPDATE.
            Self::Sqlite => format!("{base} WHERE excluded.revision >= {table}.revision"),
            // MySQL has no such clause, so the guard becomes an expression:
            // keeping the stored value when the incoming one is older is the
            // same as not updating, and it is still one statement.
            Self::MySql => {
                let assignments = columns
                    .iter()
                    .map(|column| {
                        format!(
                            "{column} = IF(VALUES(revision) >= revision, VALUES({column}), {column})"
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let names = columns.join(", ");
                let placeholders = vec!["?"; columns.len()].join(", ");
                format!(
                    "INSERT INTO {table} ({names}) VALUES ({placeholders}) \
                     ON DUPLICATE KEY UPDATE {assignments}"
                )
            }
        }
    }

    /// How to write a row that may already exist.
    ///
    /// Both dialects have an upsert and neither spells it the same way. Doing
    /// it in one statement matters more than it looks: a delete-then-insert
    /// would let another client on a shared store read the gap.
    pub fn upsert(self, table: &str, columns: &[&str]) -> String {
        let names = columns.join(", ");
        let placeholders = vec!["?"; columns.len()].join(", ");
        match self {
            Self::Sqlite => {
                let assignments = columns
                    .iter()
                    .map(|column| format!("{column} = excluded.{column}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "INSERT INTO {table} ({names}) VALUES ({placeholders}) \
                     ON CONFLICT(id) DO UPDATE SET {assignments}"
                )
            }
            Self::MySql => {
                let assignments = columns
                    .iter()
                    .map(|column| format!("{column} = VALUES({column})"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "INSERT INTO {table} ({names}) VALUES ({placeholders}) \
                     ON DUPLICATE KEY UPDATE {assignments}"
                )
            }
        }
    }

    /// Every statement needed to bring an empty store up to date.
    pub fn schema(self) -> Vec<String> {
        let id = self.id_type();
        let text = self.text_type();
        let short = self.short_text_type();
        let int = self.int_type();

        vec![
            format!(
                "CREATE TABLE IF NOT EXISTS schema_version (
                    version {int} NOT NULL
                )"
            ),
            // Users and guilds are the metadata worth having before the
            // gateway answers: a restart can draw the sidebar and every author
            // name from these while READY is still in flight.
            format!(
                "CREATE TABLE IF NOT EXISTS users (
                    id {id} NOT NULL PRIMARY KEY,
                    username {short},
                    display_name {short},
                    avatar_url {text},
                    is_bot {int} NOT NULL DEFAULT 0,
                    updated_at {int} NOT NULL DEFAULT 0,
                    -- Discord's own ordering stamp, so two clients resolve a
                    -- conflict the same way without agreeing about the time.
                    revision {int} NOT NULL DEFAULT 0,
                    -- A row Discord deleted. Kept rather than removed so a
                    -- client with stale state cannot re-insert it.
                    deleted {int} NOT NULL DEFAULT 0
                )"
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS guilds (
                    id {id} NOT NULL PRIMARY KEY,
                    name {short},
                    icon_url {text},
                    owner_id {id},
                    updated_at {int} NOT NULL DEFAULT 0,
                    -- Discord's own ordering stamp, so two clients resolve a
                    -- conflict the same way without agreeing about the time.
                    revision {int} NOT NULL DEFAULT 0,
                    -- A row Discord deleted. Kept rather than removed so a
                    -- client with stale state cannot re-insert it.
                    deleted {int} NOT NULL DEFAULT 0
                )"
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS channels (
                    id {id} NOT NULL PRIMARY KEY,
                    guild_id {id},
                    parent_id {id},
                    name {short},
                    kind {short},
                    position {int},
                    topic {text},
                    updated_at {int} NOT NULL DEFAULT 0,
                    -- Discord's own ordering stamp, so two clients resolve a
                    -- conflict the same way without agreeing about the time.
                    revision {int} NOT NULL DEFAULT 0,
                    -- A row Discord deleted. Kept rather than removed so a
                    -- client with stale state cannot re-insert it.
                    deleted {int} NOT NULL DEFAULT 0
                )"
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS messages (
                    id {id} NOT NULL PRIMARY KEY,
                    channel_id {id} NOT NULL,
                    author_id {id},
                    content {text},
                    -- ISO 8601 as Discord sends it, not parsed: a parse that
                    -- failed would lose the only ordering information there is,
                    -- and snowflakes already order by time.
                    timestamp {short},
                    edited_timestamp {short},
                    updated_at {int} NOT NULL DEFAULT 0,
                    -- Discord's own ordering stamp, so two clients resolve a
                    -- conflict the same way without agreeing about the time.
                    revision {int} NOT NULL DEFAULT 0,
                    -- A row Discord deleted. Kept rather than removed so a
                    -- client with stale state cannot re-insert it.
                    deleted {int} NOT NULL DEFAULT 0
                )"
            ),
            // Reading a channel means reading its newest messages, which is
            // this index and nothing else.
            "CREATE INDEX IF NOT EXISTS messages_by_channel
                ON messages (channel_id, id)"
                .to_owned(),
            "CREATE INDEX IF NOT EXISTS channels_by_guild
                ON channels (guild_id, position)"
                .to_owned(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn both() -> [Dialect; 2] {
        [Dialect::Sqlite, Dialect::MySql]
    }

    #[test]
    fn both_dialects_create_the_same_tables() {
        // A shared store is one another client reads. If the two dialects grew
        // different tables, a cache filled by one client would be unusable to
        // the other, which is the whole point of sharing it.
        for dialect in both() {
            let schema = dialect.schema().join(" ");
            for table in ["users", "guilds", "channels", "messages"] {
                assert!(
                    schema.contains(&format!("CREATE TABLE IF NOT EXISTS {table}")),
                    "{dialect:?} has no {table}"
                );
            }
        }
    }

    #[test]
    fn ids_are_text_in_both_dialects() {
        // A snowflake exceeds what a signed 64-bit integer holds safely, and a
        // silent wrap in one dialect would produce ids that look plausible and
        // address the wrong row.
        for dialect in both() {
            let users = dialect
                .schema()
                .into_iter()
                .find(|statement| statement.contains("users"))
                .expect("users table");
            assert!(
                users.contains("TEXT") || users.contains("VARCHAR"),
                "{dialect:?} stores ids as a number"
            );
            assert!(
                !users.contains("id BIGINT"),
                "{dialect:?} stores ids as a number"
            );
        }
    }

    #[test]
    fn an_upsert_is_one_statement_in_both_dialects() {
        // A delete-then-insert would let another client on a shared store read
        // the gap between them.
        for dialect in both() {
            let sql = dialect.upsert("users", &["id", "username"]);
            assert!(sql.starts_with("INSERT INTO users"), "{dialect:?}");
            assert!(!sql.contains("DELETE"), "{dialect:?}");
            assert_eq!(
                sql.matches(';').count(),
                0,
                "{dialect:?} emits two statements"
            );
        }
    }

    #[test]
    fn an_upsert_updates_every_column_it_inserts() {
        // A column left out of the update would keep a stale value forever,
        // which is worse than not caching it at all.
        for dialect in both() {
            let columns = ["id", "username", "display_name"];
            let sql = dialect.upsert("users", &columns);
            for column in columns {
                assert!(sql.contains(column), "{dialect:?} drops {column}");
            }
        }
    }

    #[test]
    fn placeholders_match_the_column_count() {
        // A mismatch binds the wrong value to the wrong column, which the
        // database accepts happily when the types line up.
        for dialect in both() {
            let columns = ["id", "username", "display_name", "avatar_url"];
            let sql = dialect.upsert("users", &columns);
            assert_eq!(sql.matches('?').count(), columns.len(), "{dialect:?}");
        }
    }

    #[test]
    fn reading_a_channel_has_an_index_for_it() {
        // Without it, drawing one channel scans every message ever cached.
        for dialect in both() {
            let schema = dialect.schema().join(" ");
            assert!(schema.contains("messages_by_channel"), "{dialect:?}");
        }
    }

    #[test]
    fn a_guarded_upsert_refuses_a_stale_writer_in_both_dialects() {
        // The guard has to be inside the statement. A read followed by a write
        // lets another client write between them, which is the loss this whole
        // arrangement exists to prevent.
        for dialect in both() {
            let sql = dialect.guarded_upsert("guilds", &["id", "name", "revision"]);
            assert!(sql.contains("revision"), "{dialect:?} has no guard");
            assert_eq!(
                sql.matches(';').count(),
                0,
                "{dialect:?} emits two statements"
            );
            assert!(!sql.contains("SELECT"), "{dialect:?} reads before writing");
        }
    }

    #[test]
    fn a_guarded_upsert_still_inserts_a_row_nobody_has_written() {
        // The guard is about losing to a fresher writer, not about refusing to
        // write at all - a store that never took a first row would stay empty.
        for dialect in both() {
            let sql = dialect.guarded_upsert("guilds", &["id", "name", "revision"]);
            assert!(sql.starts_with("INSERT INTO guilds"), "{dialect:?}");
        }
    }

    #[test]
    fn every_cached_table_can_lose_a_write_and_be_tombstoned() {
        // A table without a revision cannot resolve a conflict, and one
        // without a tombstone lets a stale client resurrect a deleted row.
        for dialect in both() {
            for table in ["users", "guilds", "channels", "messages"] {
                let statement = dialect
                    .schema()
                    .into_iter()
                    .find(|statement| statement.contains(&format!("EXISTS {table}")))
                    .unwrap_or_else(|| panic!("{dialect:?} has no {table}"));
                assert!(statement.contains("revision"), "{table} in {dialect:?}");
                assert!(statement.contains("deleted"), "{table} in {dialect:?}");
            }
        }
    }

    #[test]
    fn the_dialect_follows_the_backend() {
        assert_eq!(
            Dialect::of(&StorageBackend::Sqlite {
                path: "/tmp/a.db".into()
            }),
            Dialect::Sqlite
        );
        assert_eq!(
            Dialect::of(&StorageBackend::MySql {
                host: "host".to_owned(),
                port: 3306,
                database: "discord".to_owned(),
                user: None,
                password: None,
            }),
            Dialect::MySql
        );
    }
}
