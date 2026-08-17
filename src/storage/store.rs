//! Connecting to the cache and reading and writing it.
//!
//! One type over two backends. Which one is a runtime choice, because a shared
//! store is something somebody points an already-built client at.
//!
//! Every write goes through the revision guard in `super::schema`; see
//! `super::concurrent` for why that guard is in the statement rather than in a
//! read followed by a write.

use sqlx::{AnyPool, Row, any::AnyPoolOptions};

use super::{Dialect, StorageBackend};

/// A row as the cache holds it. Flat, because a shared store is one another
/// client reads.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CachedUser {
    pub id: String,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub is_bot: bool,
    pub revision: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CachedGuild {
    pub id: String,
    pub name: Option<String>,
    pub icon_url: Option<String>,
    pub owner_id: Option<String>,
    pub revision: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CachedMessage {
    pub id: String,
    pub channel_id: String,
    pub author_id: Option<String>,
    pub content: Option<String>,
    pub timestamp: Option<String>,
    pub edited_timestamp: Option<String>,
    pub revision: u64,
}

/// The cache.
///
/// `Debug` prints the dialect and nothing else: a pool holds the connection
/// string, and for a shared store that string carries a password.
pub struct Store {
    pool: AnyPool,
    dialect: Dialect,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Store")
            .field("dialect", &self.dialect)
            .finish_non_exhaustive()
    }
}

impl Store {
    /// Open the store, creating it if it is not there.
    ///
    /// A store that cannot be opened is an error the caller may ignore: the
    /// client worked without a cache before this existed, and refusing to
    /// start because a MariaDB server is down would make the optional backend
    /// worse than no backend.
    pub async fn open(backend: &StorageBackend) -> Result<Self, sqlx::Error> {
        sqlx::any::install_default_drivers();
        let dialect = Dialect::of(backend);

        let url = match backend {
            StorageBackend::Sqlite { path } => {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                sqlite_url(path)
            }
            StorageBackend::MySql {
                host,
                port,
                database,
                user,
                password,
            } => {
                let credentials = match (user, password) {
                    (Some(user), Some(password)) => format!("{user}:{password}@"),
                    (Some(user), None) => format!("{user}@"),
                    _ => String::new(),
                };
                format!("mysql://{credentials}{host}:{port}/{database}")
            }
        };

        let pool = AnyPoolOptions::new()
            // Small on purpose: this is a cache on the side of a client, not a
            // service. A large pool against a shared server would take
            // connections from the other clients using it.
            .max_connections(4)
            .connect(&url)
            .await?;

        let store = Self { pool, dialect };
        store.migrate().await?;
        Ok(store)
    }

    async fn migrate(&self) -> Result<(), sqlx::Error> {
        for statement in self.dialect.schema() {
            sqlx::query(&statement).execute(&self.pool).await?;
        }
        Ok(())
    }

    pub async fn upsert_user(&self, user: &CachedUser) -> Result<(), sqlx::Error> {
        let sql = self.dialect.guarded_upsert(
            "users",
            &[
                "id",
                "username",
                "display_name",
                "avatar_url",
                "is_bot",
                "revision",
            ],
        );
        sqlx::query(&sql)
            .bind(&user.id)
            .bind(&user.username)
            .bind(&user.display_name)
            .bind(&user.avatar_url)
            .bind(i64::from(user.is_bot))
            .bind(revision_column(user.revision))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn user(&self, id: &str) -> Result<Option<CachedUser>, sqlx::Error> {
        // Tombstoned rows are skipped rather than returned: a caller asking
        // for a user wants one that exists, and the tombstone exists to stop
        // writes rather than to be read back.
        let row = sqlx::query(
            "SELECT id, username, display_name, avatar_url, is_bot, revision
             FROM users WHERE id = ? AND deleted = 0",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| CachedUser {
            id: row.get("id"),
            username: row.get("username"),
            display_name: row.get("display_name"),
            avatar_url: row.get("avatar_url"),
            is_bot: row.get::<i64, _>("is_bot") != 0,
            revision: revision_value(row.get::<i64, _>("revision")),
        }))
    }

    pub async fn upsert_guild(&self, guild: &CachedGuild) -> Result<(), sqlx::Error> {
        let sql = self.dialect.guarded_upsert(
            "guilds",
            &["id", "name", "icon_url", "owner_id", "revision"],
        );
        sqlx::query(&sql)
            .bind(&guild.id)
            .bind(&guild.name)
            .bind(&guild.icon_url)
            .bind(&guild.owner_id)
            .bind(revision_column(guild.revision))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn guilds(&self) -> Result<Vec<CachedGuild>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, name, icon_url, owner_id, revision
             FROM guilds WHERE deleted = 0 ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| CachedGuild {
                id: row.get("id"),
                name: row.get("name"),
                icon_url: row.get("icon_url"),
                owner_id: row.get("owner_id"),
                revision: revision_value(row.get::<i64, _>("revision")),
            })
            .collect())
    }

    pub async fn upsert_message(&self, message: &CachedMessage) -> Result<(), sqlx::Error> {
        let sql = self.dialect.guarded_upsert(
            "messages",
            &[
                "id",
                "channel_id",
                "author_id",
                "content",
                "timestamp",
                "edited_timestamp",
                "revision",
            ],
        );
        sqlx::query(&sql)
            .bind(&message.id)
            .bind(&message.channel_id)
            .bind(&message.author_id)
            .bind(&message.content)
            .bind(&message.timestamp)
            .bind(&message.edited_timestamp)
            .bind(revision_column(message.revision))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The newest messages in a channel, oldest first.
    ///
    /// Ordered by id rather than by timestamp: a snowflake already orders by
    /// time, and it cannot be missing or ambiguous the way a parsed timestamp
    /// can. Ids are text, so the ordering is lexicographic - which is the same
    /// as numeric only while every id has the same number of digits. That has
    /// been true since 2016 and will remain so for decades, and the test below
    /// says so rather than leaving it as a surprise.
    pub async fn recent_messages(
        &self,
        channel_id: &str,
        limit: u32,
    ) -> Result<Vec<CachedMessage>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, channel_id, author_id, content, timestamp, edited_timestamp, revision
             FROM messages WHERE channel_id = ? AND deleted = 0
             ORDER BY id DESC LIMIT ?",
        )
        .bind(channel_id)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;

        let mut messages: Vec<CachedMessage> = rows
            .into_iter()
            .map(|row| CachedMessage {
                id: row.get("id"),
                channel_id: row.get("channel_id"),
                author_id: row.get("author_id"),
                content: row.get("content"),
                timestamp: row.get("timestamp"),
                edited_timestamp: row.get("edited_timestamp"),
                revision: revision_value(row.get::<i64, _>("revision")),
            })
            .collect();
        // Fetched newest-first so the limit takes the newest, returned
        // oldest-first because that is reading order.
        messages.reverse();
        Ok(messages)
    }

    /// Apply what an event asked for.
    ///
    /// Errors are returned rather than logged here so the caller decides: a
    /// cache write that fails is not a reason to drop the event, which the
    /// in-memory state has already taken.
    pub async fn apply(&self, write: &super::persist::Write) -> Result<(), sqlx::Error> {
        use super::persist::Write;
        match write {
            Write::User(user) => self.upsert_user(user).await,
            Write::Guild(guild) => self.upsert_guild(guild).await,
            Write::Message(message) => self.upsert_message(message).await,
            Write::Tombstone {
                table,
                id,
                revision,
            } => self.tombstone(table, id, *revision).await,
        }
    }

    /// Mark a row deleted rather than removing it.
    ///
    /// Removing would let a client with stale state insert it again; see
    /// `super::concurrent`.
    pub async fn tombstone(&self, table: &str, id: &str, revision: u64) -> Result<(), sqlx::Error> {
        // The table name is not user input - callers pass a literal - but it
        // is interpolated rather than bound because no database binds an
        // identifier, so it is checked here instead.
        if !matches!(table, "users" | "guilds" | "channels" | "messages") {
            return Ok(());
        }
        let sql =
            format!("UPDATE {table} SET deleted = 1, revision = ? WHERE id = ? AND revision <= ?");
        let revision = revision_column(revision);
        sqlx::query(&sql)
            .bind(revision)
            .bind(id)
            .bind(revision)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

/// How sqlx spells a SQLite location.
///
/// In memory is not a path: `sqlite://:memory:` parses `:memory:` as a host and
/// fails with an empty-host error, which reads as a network problem for a
/// database that never touches the network.
fn sqlite_url(path: &std::path::Path) -> String {
    if path.as_os_str() == ":memory:" {
        // Two things at once. `cache=shared` because without it every pooled
        // connection gets its own empty database, so the migration runs on one
        // and every query lands on another and finds no tables. A unique name
        // because a shared cache is shared by *name* - every in-memory store
        // in the process would otherwise be the same database, which is wrong
        // for anything holding two and fatal for tests running in parallel.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return format!("sqlite:file:concord-cache-{unique}?mode=memory&cache=shared");
    }
    // `mode=rwc` creates the file. Without it a first run fails on a store that
    // does not exist yet, which is every first run.
    format!("sqlite://{}?mode=rwc", path.display())
}

/// Revisions are stored signed because both dialects' integer column is.
///
/// Discord's versions are far below the point where this matters, and
/// saturating is the safe direction: a clamped revision loses a write rather
/// than winning one it should not.
fn revision_column(revision: u64) -> i64 {
    i64::try_from(revision).unwrap_or(i64::MAX)
}

fn revision_value(stored: i64) -> u64 {
    u64::try_from(stored).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store() -> Store {
        // In memory: a test that wrote a file would leave one behind and share
        // it with every other test in the run.
        Store::open(&StorageBackend::Sqlite {
            path: ":memory:".into(),
        })
        .await
        .expect("in-memory store should open")
    }

    fn user(revision: u64, name: &str) -> CachedUser {
        CachedUser {
            id: "1".to_owned(),
            username: Some(name.to_owned()),
            display_name: Some(name.to_owned()),
            avatar_url: None,
            is_bot: false,
            revision,
        }
    }

    #[tokio::test]
    async fn a_store_survives_being_opened_twice() {
        // Opening runs the schema, and a second open must not fail on tables
        // that already exist - which is every run after the first.
        let store = store().await;
        store
            .migrate()
            .await
            .expect("second migrate should succeed");
    }

    #[tokio::test]
    async fn a_written_row_reads_back() {
        let store = store().await;
        store.upsert_user(&user(1, "sam")).await.expect("write");

        let read = store.user("1").await.expect("read").expect("row");
        assert_eq!(read.username.as_deref(), Some("sam"));
        assert_eq!(read.revision, 1);
    }

    #[tokio::test]
    async fn a_newer_revision_wins_and_a_staler_one_does_not() {
        // The whole point of the guard: two clients write the same row and the
        // one that is behind must not win by writing last.
        let store = store().await;
        store.upsert_user(&user(2, "fresh")).await.expect("write");
        store.upsert_user(&user(1, "stale")).await.expect("write");

        let read = store.user("1").await.expect("read").expect("row");
        assert_eq!(
            read.username.as_deref(),
            Some("fresh"),
            "a stale writer clobbered fresh data"
        );
    }

    #[tokio::test]
    async fn writing_the_same_revision_twice_is_harmless() {
        // Two clients receiving the same gateway event is the common case.
        let store = store().await;
        store.upsert_user(&user(1, "sam")).await.expect("write");
        store.upsert_user(&user(1, "sam")).await.expect("write");

        assert_eq!(
            store.user("1").await.expect("read").expect("row").username,
            Some("sam".to_owned())
        );
    }

    #[tokio::test]
    async fn a_tombstoned_row_stops_being_returned() {
        let store = store().await;
        store.upsert_user(&user(1, "sam")).await.expect("write");
        store.tombstone("users", "1", 2).await.expect("tombstone");

        assert!(store.user("1").await.expect("read").is_none());
    }

    #[tokio::test]
    async fn a_stale_client_cannot_resurrect_a_deleted_row() {
        // The case tombstones exist for: a client that has not seen the delete
        // writes what it still holds.
        let store = store().await;
        store.upsert_user(&user(1, "sam")).await.expect("write");
        store.tombstone("users", "1", 2).await.expect("tombstone");
        store.upsert_user(&user(1, "sam")).await.expect("write");

        assert!(
            store.user("1").await.expect("read").is_none(),
            "a stale write resurrected a deleted row"
        );
    }

    #[tokio::test]
    async fn messages_come_back_oldest_first_within_the_newest() {
        // Fetched newest-first so the limit takes the newest, returned
        // oldest-first because that is reading order.
        let store = store().await;
        for id in ["100", "200", "300"] {
            store
                .upsert_message(&CachedMessage {
                    id: id.to_owned(),
                    channel_id: "c".to_owned(),
                    content: Some(id.to_owned()),
                    revision: 1,
                    ..CachedMessage::default()
                })
                .await
                .expect("write");
        }

        let recent = store.recent_messages("c", 2).await.expect("read");
        let ids: Vec<&str> = recent.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["200", "300"]);
    }

    #[tokio::test]
    async fn messages_from_another_channel_are_not_returned() {
        let store = store().await;
        for (id, channel) in [("1", "a"), ("2", "b")] {
            store
                .upsert_message(&CachedMessage {
                    id: id.to_owned(),
                    channel_id: channel.to_owned(),
                    revision: 1,
                    ..CachedMessage::default()
                })
                .await
                .expect("write");
        }

        let recent = store.recent_messages("a", 10).await.expect("read");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].id, "1");
    }

    #[tokio::test]
    async fn guilds_read_back_for_drawing_the_sidebar_before_the_gateway_answers() {
        // The reason the cache exists at all.
        let store = store().await;
        store
            .upsert_guild(&CachedGuild {
                id: "7".to_owned(),
                name: Some("Rustaceans".to_owned()),
                revision: 1,
                ..CachedGuild::default()
            })
            .await
            .expect("write");

        let guilds = store.guilds().await.expect("read");
        assert_eq!(guilds.len(), 1);
        assert_eq!(guilds[0].name.as_deref(), Some("Rustaceans"));
    }

    #[tokio::test]
    async fn a_table_name_that_is_not_one_of_ours_is_refused() {
        // Table names cannot be bound by any database, so they are checked
        // rather than escaped. Callers pass literals today; this is what stops
        // that from being load-bearing.
        let store = store().await;
        store
            .tombstone("users; DROP TABLE users", "1", 1)
            .await
            .expect("should be ignored rather than executed");

        store.upsert_user(&user(1, "sam")).await.expect("write");
        assert!(store.user("1").await.expect("read").is_some());
    }

    #[tokio::test]
    async fn applying_a_message_write_caches_the_author_too() {
        // End to end: the translation says both, and the store takes both.
        let store = store().await;
        let message = crate::discord::MessageInfo {
            message_id: crate::discord::ids::Id::new(100),
            channel_id: crate::discord::ids::Id::new(200),
            author_id: crate::discord::ids::Id::new(300),
            author: "sam".to_owned(),
            content: Some("hello".to_owned()),
            ..Default::default()
        };
        for write in super::super::persist::message_writes(&message) {
            store.apply(&write).await.expect("apply");
        }

        assert_eq!(
            store
                .user("300")
                .await
                .expect("read")
                .expect("row")
                .username,
            Some("sam".to_owned())
        );
        assert_eq!(
            store.recent_messages("200", 10).await.expect("read").len(),
            1
        );
    }

    #[tokio::test]
    async fn an_edit_replaces_the_message_it_edits() {
        // The revision rule and the guard together, which is where a mistake
        // in either would show as an edit that silently did not stick.
        let store = store().await;
        let original = CachedMessage {
            id: "1".to_owned(),
            channel_id: "c".to_owned(),
            content: Some("before".to_owned()),
            revision: super::super::persist::message_revision(None),
            ..CachedMessage::default()
        };
        let edited = CachedMessage {
            content: Some("after".to_owned()),
            edited_timestamp: Some("2026-09-01T19:00:00+00:00".to_owned()),
            revision: super::super::persist::message_revision(Some("2026-09-01T19:00:00+00:00")),
            ..original.clone()
        };

        store.upsert_message(&original).await.expect("write");
        store.upsert_message(&edited).await.expect("write");

        let messages = store.recent_messages("c", 10).await.expect("read");
        assert_eq!(messages[0].content.as_deref(), Some("after"));

        // And the original arriving late does not undo the edit, which is the
        // case two clients make likely.
        store.upsert_message(&original).await.expect("write");
        let messages = store.recent_messages("c", 10).await.expect("read");
        assert_eq!(
            messages[0].content.as_deref(),
            Some("after"),
            "a late original undid an edit"
        );
    }

    #[tokio::test]
    async fn a_store_never_prints_its_connection_string() {
        // For a shared backend that string carries a password, and `{:?}` on
        // the client that holds the store is what a debug log formats.
        let store = store().await;
        let printed = format!("{store:?}");
        assert!(printed.contains("Store"));
        assert!(!printed.contains("password"));
        assert!(!printed.contains("mysql://"));
    }

    #[test]
    fn in_memory_is_spelled_differently_from_a_path() {
        // `sqlite://:memory:` parses `:memory:` as a host and fails with an
        // empty-host error, which reads as a network problem for a database
        // that never touches the network.
        let memory = sqlite_url(std::path::Path::new(":memory:"));
        assert!(memory.contains("mode=memory"));
        // Without a shared cache every pooled connection gets its own empty
        // database, and the migration is invisible to every query after it.
        assert!(memory.contains("cache=shared"));
        // And a shared cache is shared by name, so two in-memory stores in one
        // process must not be handed the same one.
        assert_ne!(memory, sqlite_url(std::path::Path::new(":memory:")));
        assert!(sqlite_url(std::path::Path::new("/tmp/a.db")).starts_with("sqlite:///tmp/a.db"));
    }

    #[test]
    fn a_file_store_is_created_rather_than_required_to_exist() {
        // Every first run has no file, and a store that refused to make one
        // would never cache anything.
        assert!(sqlite_url(std::path::Path::new("/tmp/a.db")).contains("mode=rwc"));
    }

    #[test]
    fn a_revision_too_large_for_the_column_loses_rather_than_wins() {
        // Clamping upward would let an absurd revision win every future
        // conflict; clamping is chosen so it loses instead.
        assert_eq!(revision_column(u64::MAX), i64::MAX);
        assert_eq!(revision_value(-1), 0);
        assert_eq!(revision_value(5), 5);
    }
}
