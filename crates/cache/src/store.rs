//! Connecting to the cache and reading and writing it.
//!
//! One type over two backends. Which one is a runtime choice, because a shared
//! store is something somebody points an already-built client at.
//!
//! Every write goes through the revision guard in `super::schema`; see
//! `super::concurrent` for why that guard is in the statement rather than in a
//! read followed by a write.

use sqlx::{AnyPool, Row, any::AnyPoolOptions};

use super::schema::SCHEMA_VERSION;
use super::{Dialect, StorageBackend};

/// The row in `schema_meta` holding the version. Named rather than implied by
/// being the only row, so a later value can share the table.
const SCHEMA_VERSION_KEY: &str = "schema_version";

/// Marks the one store failure worth telling somebody about.
///
/// Every other reason to go without a cache is transient or local. This one
/// means another client on a shared store is newer than this build, which is
/// a thing a person can act on and would otherwise never see.
pub const NEWER_STORE_MARKER: &str = "the cache was written by a newer client";

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
pub struct CachedSticker {
    pub id: String,
    pub message_id: String,
    pub name: Option<String>,
    /// Discord's own format number, so it reads back as the format it was.
    pub format: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CachedAttachment {
    pub id: String,
    pub message_id: String,
    pub filename: Option<String>,
    pub url: Option<String>,
    pub content_type: Option<String>,
    pub size: u64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub description: Option<String>,
    pub revision: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CachedMember {
    pub guild_id: String,
    pub user_id: String,
    pub display_name: Option<String>,
    pub nickname: Option<String>,
    pub avatar_url: Option<String>,
    pub is_bot: bool,
    pub joined_at: Option<String>,
    pub role_ids: Vec<String>,
    pub revision: u64,
}

impl CachedMember {
    /// The primary key: a member is a guild and a user together, and the rest
    /// of the store is keyed by a single `id` column.
    pub fn id(&self) -> String {
        format!("{}:{}", self.guild_id, self.user_id)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CachedChannel {
    pub id: String,
    pub guild_id: Option<String>,
    pub parent_id: Option<String>,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub position: Option<i64>,
    pub topic: Option<String>,
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
    /// Whether the message had attachments, embeds, stickers or a poll. None
    /// of those are cached, so one that had them is not replayed.
    pub has_extras: bool,
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

        let found = self.schema_version().await?;
        match found {
            // Nothing has stamped this store: either it is new, or it was
            // written before versions were recorded. `CREATE TABLE IF NOT
            // EXISTS` has just made the tables but cannot add a column to a
            // table that already existed, so the older case still needs the
            // migrations run.
            None => {
                self.run_migrations_from(1).await;
                self.stamp_schema_version().await?;
            }
            Some(version) if version < SCHEMA_VERSION => {
                self.run_migrations_from(version).await;
                self.stamp_schema_version().await?;
            }
            Some(version) if version > SCHEMA_VERSION => {
                // A shared store may be in use by a client this build knows
                // nothing about. Refusing is what keeps this one from writing
                // rows the other cannot read; the caller falls back to running
                // without a cache, which is slower rather than broken.
                return Err(sqlx::Error::Protocol(format!(
                    "{NEWER_STORE_MARKER}: schema {version}, this build understands \
                     {SCHEMA_VERSION}"
                )));
            }
            Some(_) => {}
        }
        Ok(())
    }

    async fn schema_version(&self) -> Result<Option<u32>, sqlx::Error> {
        let row = sqlx::query("SELECT value FROM schema_meta WHERE name = ?")
            .bind(SCHEMA_VERSION_KEY)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.get::<i64, _>("value").max(0) as u32))
    }

    async fn stamp_schema_version(&self) -> Result<(), sqlx::Error> {
        let sql = match self.dialect {
            Dialect::Sqlite => {
                "INSERT INTO schema_meta (name, value) VALUES (?, ?)
                 ON CONFLICT(name) DO UPDATE SET value = excluded.value"
            }
            Dialect::MySql => {
                "INSERT INTO schema_meta (name, value) VALUES (?, ?)
                 ON DUPLICATE KEY UPDATE value = VALUES(value)"
            }
        };
        sqlx::query(sql)
            .bind(SCHEMA_VERSION_KEY)
            .bind(i64::from(SCHEMA_VERSION))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Bring an older store forward, one version at a time.
    ///
    /// Each step is allowed to fail: the usual reason is that another client
    /// on a shared store ran the same migration a moment earlier, and adding a
    /// column that is already there is success as far as this client is
    /// concerned. A step that fails for a real reason shows up as the missing
    /// data it was meant to add rather than as a store that will not open,
    /// which is the better failure for a cache.
    async fn run_migrations_from(&self, from: u32) {
        if from <= 1 {
            // Version 2 records whether a message had attachments, embeds,
            // stickers or a poll, so the replay can skip the ones it would
            // draw without their contents.
            let int = match self.dialect {
                Dialect::Sqlite => "INTEGER",
                Dialect::MySql => "BIGINT",
            };
            let sql =
                format!("ALTER TABLE messages ADD COLUMN has_extras {int} NOT NULL DEFAULT 0");
            let _ = sqlx::query(&sql).execute(&self.pool).await;
        }
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

    pub async fn upsert_sticker(&self, sticker: &CachedSticker) -> Result<(), sqlx::Error> {
        let sql = self.dialect.guarded_upsert(
            "stickers",
            &["id", "message_id", "name", "format", "revision"],
        );
        sqlx::query(&sql)
            .bind(&sticker.id)
            .bind(&sticker.message_id)
            .bind(&sticker.name)
            .bind(revision_column(sticker.format))
            .bind(0_i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Every sticker on the given messages, keyed by message.
    ///
    /// Batched for the same reason as attachments: one query per message would
    /// cost more than the fetch it is meant to pre-empt.
    pub async fn stickers_for(
        &self,
        message_ids: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<CachedSticker>>, sqlx::Error> {
        use std::collections::HashMap;
        if message_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let placeholders = vec!["?"; message_ids.len()].join(", ");
        let sql = format!(
            "SELECT id, message_id, name, format FROM stickers
             WHERE deleted = 0 AND message_id IN ({placeholders})
             ORDER BY LENGTH(id), id"
        );
        let mut query = sqlx::query(&sql);
        for id in message_ids {
            query = query.bind(id);
        }

        let mut by_message: HashMap<String, Vec<CachedSticker>> = HashMap::new();
        for row in query.fetch_all(&self.pool).await? {
            let sticker = CachedSticker {
                id: row.get("id"),
                message_id: row.get("message_id"),
                name: row.get("name"),
                format: revision_value(row.get::<i64, _>("format")),
            };
            by_message
                .entry(sticker.message_id.clone())
                .or_default()
                .push(sticker);
        }
        Ok(by_message)
    }

    pub async fn upsert_attachment(
        &self,
        attachment: &CachedAttachment,
    ) -> Result<(), sqlx::Error> {
        let sql = self.dialect.guarded_upsert(
            "attachments",
            &[
                "id",
                "message_id",
                "filename",
                "url",
                "content_type",
                "size",
                "width",
                "height",
                "description",
                "revision",
            ],
        );
        sqlx::query(&sql)
            .bind(&attachment.id)
            .bind(&attachment.message_id)
            .bind(&attachment.filename)
            .bind(&attachment.url)
            .bind(&attachment.content_type)
            .bind(byte_count_column(attachment.size))
            .bind(attachment.width)
            .bind(attachment.height)
            .bind(&attachment.description)
            .bind(revision_column(attachment.revision))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Every attachment on the given messages, keyed by message.
    ///
    /// Asked for in one query rather than one per message: a channel of fifty
    /// messages would otherwise be fifty round trips before anything is drawn,
    /// which costs more than the fetch it is meant to pre-empt.
    pub async fn attachments_for(
        &self,
        message_ids: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<CachedAttachment>>, sqlx::Error> {
        use std::collections::HashMap;
        if message_ids.is_empty() {
            return Ok(HashMap::new());
        }

        // Built rather than bound as a list because no database binds one.
        // Safe because every id here came from a column this store wrote.
        let placeholders = vec!["?"; message_ids.len()].join(", ");
        let sql = format!(
            "SELECT id, message_id, filename, url, content_type, size, width, height,
                    description, revision
             FROM attachments WHERE deleted = 0 AND message_id IN ({placeholders})
             ORDER BY LENGTH(id), id"
        );
        let mut query = sqlx::query(&sql);
        for id in message_ids {
            query = query.bind(id);
        }

        let mut by_message: HashMap<String, Vec<CachedAttachment>> = HashMap::new();
        for row in query.fetch_all(&self.pool).await? {
            let attachment = CachedAttachment {
                id: row.get("id"),
                message_id: row.get("message_id"),
                filename: row.get("filename"),
                url: row.get("url"),
                content_type: row.get("content_type"),
                size: byte_count_value(row.get::<i64, _>("size")),
                width: row.get("width"),
                height: row.get("height"),
                description: row.get("description"),
                revision: revision_value(row.get::<i64, _>("revision")),
            };
            by_message
                .entry(attachment.message_id.clone())
                .or_default()
                .push(attachment);
        }
        Ok(by_message)
    }

    pub async fn upsert_member(&self, member: &CachedMember) -> Result<(), sqlx::Error> {
        let sql = self.dialect.guarded_upsert(
            "members",
            &[
                "id",
                "guild_id",
                "user_id",
                "display_name",
                "nickname",
                "avatar_url",
                "is_bot",
                "joined_at",
                "role_ids",
                "revision",
            ],
        );
        sqlx::query(&sql)
            .bind(member.id())
            .bind(&member.guild_id)
            .bind(&member.user_id)
            .bind(&member.display_name)
            .bind(&member.nickname)
            .bind(&member.avatar_url)
            .bind(i64::from(member.is_bot))
            .bind(&member.joined_at)
            .bind(member.role_ids.join(","))
            .bind(revision_column(member.revision))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// A guild's cached members.
    ///
    /// Capped by the caller rather than read whole: a large server has more
    /// members than anyone scrolls, and the point is to draw the list quickly
    /// rather than to hold a copy of the server.
    pub async fn members(
        &self,
        guild_id: &str,
        limit: u32,
    ) -> Result<Vec<CachedMember>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT guild_id, user_id, display_name, nickname, avatar_url, is_bot,
                    joined_at, role_ids, revision
             FROM members WHERE guild_id = ? AND deleted = 0
             ORDER BY display_name, LENGTH(user_id), user_id
             LIMIT ?",
        )
        .bind(guild_id)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let roles: Option<String> = row.get("role_ids");
                CachedMember {
                    guild_id: row.get("guild_id"),
                    user_id: row.get("user_id"),
                    display_name: row.get("display_name"),
                    nickname: row.get("nickname"),
                    avatar_url: row.get("avatar_url"),
                    is_bot: row.get::<i64, _>("is_bot") != 0,
                    joined_at: row.get("joined_at"),
                    role_ids: roles
                        .unwrap_or_default()
                        .split(',')
                        .filter(|role| !role.is_empty())
                        .map(str::to_owned)
                        .collect(),
                    revision: revision_value(row.get::<i64, _>("revision")),
                }
            })
            .collect())
    }

    pub async fn upsert_channel(&self, channel: &CachedChannel) -> Result<(), sqlx::Error> {
        let sql = self.dialect.guarded_upsert(
            "channels",
            &[
                "id",
                "guild_id",
                "parent_id",
                "name",
                "kind",
                "position",
                "topic",
                "revision",
            ],
        );
        sqlx::query(&sql)
            .bind(&channel.id)
            .bind(&channel.guild_id)
            .bind(&channel.parent_id)
            .bind(&channel.name)
            .bind(&channel.kind)
            .bind(channel.position)
            .bind(&channel.topic)
            .bind(revision_column(channel.revision))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// A guild's channels, in the order the sidebar draws them.
    ///
    /// Ordered here rather than by the caller so the cached sidebar and the
    /// live one agree: a cached list in a different order would look like the
    /// channels had moved and then moved back.
    pub async fn channels(&self, guild_id: &str) -> Result<Vec<CachedChannel>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, guild_id, parent_id, name, kind, position, topic, revision
             FROM channels WHERE guild_id = ? AND deleted = 0
             ORDER BY position, LENGTH(id), id",
        )
        .bind(guild_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| CachedChannel {
                id: row.get("id"),
                guild_id: row.get("guild_id"),
                parent_id: row.get("parent_id"),
                name: row.get("name"),
                kind: row.get("kind"),
                position: row.get("position"),
                topic: row.get("topic"),
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
                "has_extras",
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
            .bind(i64::from(message.has_extras))
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
            "SELECT id, channel_id, author_id, content, timestamp, edited_timestamp,
                    has_extras, revision
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
                has_extras: row.get::<i64, _>("has_extras") != 0,
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
            Write::Channel(channel) => self.upsert_channel(channel).await,
            Write::Member(member) => self.upsert_member(member).await,
            Write::Attachment(attachment) => self.upsert_attachment(attachment).await,
            Write::Sticker(sticker) => self.upsert_sticker(sticker).await,
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
    /// Keep only the newest messages in each channel.
    ///
    /// Per channel rather than a total, and by count rather than by age: an
    /// age bound would empty a quiet channel completely, which is the one the
    /// cache is most useful for, and a global total would let one busy channel
    /// evict every other.
    ///
    /// Tombstones are pruned along with the rest. A tombstone only has to
    /// outlive the stale copies it is guarding against, and a row this far
    /// down the channel is one no client is still holding a newer write for.
    pub async fn prune_messages(&self, keep_per_channel: u32) -> Result<u64, sqlx::Error> {
        // Ordered by length first because ids are text: a snowflake is a
        // number stored as a string, so plain lexicographic ordering puts a
        // shorter id after a longer one and would evict the newest messages
        // rather than the oldest. Discord's ids are all the same length today,
        // which is exactly why this would go unnoticed until they are not.
        let sql = "DELETE FROM messages WHERE id IN (
                       SELECT id FROM (
                           SELECT id, ROW_NUMBER() OVER (
                               PARTITION BY channel_id
                               ORDER BY LENGTH(id) DESC, id DESC
                           ) AS row_number
                           FROM messages
                       ) AS ranked
                       WHERE row_number > ?
                   )";
        let deleted = sqlx::query(sql)
            .bind(i64::from(keep_per_channel))
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(deleted)
    }

    /// Drop rows belonging to guilds that are gone.
    ///
    /// Leaving a server tombstones the guild, but its channels and members are
    /// separate rows that nothing touches - so a server left years ago keeps
    /// its whole member list forever. The guild row itself stays: it is one
    /// row, and it is the tombstone that stops a stale client re-adding it.
    ///
    /// Rows with no guild at all are left alone. A direct message is a channel
    /// with a null `guild_id`, and treating "belongs to no guild" as "belongs
    /// to a guild that is gone" would delete every DM in the cache.
    pub async fn prune_orphans(&self) -> Result<u64, sqlx::Error> {
        let mut removed = 0;
        for table in ["channels", "members"] {
            let sql = format!(
                "DELETE FROM {table}
                 WHERE guild_id IS NOT NULL
                   AND guild_id NOT IN (SELECT id FROM guilds WHERE deleted = 0)"
            );
            removed += sqlx::query(&sql).execute(&self.pool).await?.rows_affected();
        }
        Ok(removed)
    }

    /// Drop attachments whose message is no longer cached.
    ///
    /// Run after the message prune, which deletes rows outright rather than
    /// tombstoning them, so the attachments it leaves behind reference nothing
    /// and would never be read again.
    pub async fn prune_orphan_attachments(&self) -> Result<u64, sqlx::Error> {
        let mut removed = 0;
        for table in ["attachments", "stickers"] {
            let sql =
                format!("DELETE FROM {table} WHERE message_id NOT IN (SELECT id FROM messages)");
            removed += sqlx::query(&sql).execute(&self.pool).await?.rows_affected();
        }
        Ok(removed)
    }

    pub async fn tombstone(&self, table: &str, id: &str, revision: u64) -> Result<(), sqlx::Error> {
        // The table name is not user input - callers pass a literal - but it
        // is interpolated rather than bound because no database binds an
        // identifier, so it is checked here instead.
        if !matches!(
            table,
            "users" | "guilds" | "channels" | "messages" | "members" | "attachments" | "stickers"
        ) {
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

/// A file size on its way into an integer column.
///
/// Separate from `revision_column` although both clamp: a revision that
/// saturates is a real design decision about ordering, while a size that
/// saturates is impossible - Discord's largest upload is under a gigabyte, so
/// the clamp is unreachable rather than lossy. Sharing the function would
/// suggest the two are the same kind of number.
const fn byte_count_column(size: u64) -> i64 {
    if size > i64::MAX as u64 {
        i64::MAX
    } else {
        size as i64
    }
}

const fn byte_count_value(stored: i64) -> u64 {
    if stored < 0 { 0 } else { stored as u64 }
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
        let message = concord::discord::MessageInfo {
            message_id: concord::discord::ids::Id::new(100),
            channel_id: concord::discord::ids::Id::new(200),
            author_id: concord::discord::ids::Id::new(300),
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

#[cfg(test)]
mod orphan_tests {
    use super::*;

    async fn store() -> Store {
        Store::open(&StorageBackend::Sqlite {
            path: std::path::PathBuf::from(":memory:"),
        })
        .await
        .expect("should open")
    }

    async fn guild(store: &Store, id: &str) {
        store
            .upsert_guild(&CachedGuild {
                id: id.to_owned(),
                name: Some(id.to_owned()),
                ..Default::default()
            })
            .await
            .expect("should write");
    }

    #[tokio::test]
    async fn a_guild_that_was_left_takes_its_channels_and_members_with_it() {
        let store = store().await;
        guild(&store, "g").await;
        store
            .upsert_channel(&CachedChannel {
                id: "c".to_owned(),
                guild_id: Some("g".to_owned()),
                ..Default::default()
            })
            .await
            .expect("should write");
        store
            .upsert_member(&CachedMember {
                guild_id: "g".to_owned(),
                user_id: "u".to_owned(),
                ..Default::default()
            })
            .await
            .expect("should write");

        store
            .tombstone("guilds", "g", u64::MAX)
            .await
            .expect("should tombstone");
        assert_eq!(store.prune_orphans().await.expect("should prune"), 2);

        assert!(store.channels("g").await.expect("read").is_empty());
        assert!(store.members("g", 10).await.expect("read").is_empty());
    }

    #[tokio::test]
    async fn a_direct_message_is_not_an_orphan() {
        // A DM is a channel with no guild. Reading "belongs to no guild" as
        // "belongs to a guild that is gone" would delete every DM in the
        // cache, which is the one thing here with no server to refill it.
        let store = store().await;
        store
            .upsert_channel(&CachedChannel {
                id: "dm".to_owned(),
                guild_id: None,
                ..Default::default()
            })
            .await
            .expect("should write");

        assert_eq!(store.prune_orphans().await.expect("should prune"), 0);
    }

    #[tokio::test]
    async fn a_guild_still_joined_keeps_everything() {
        let store = store().await;
        guild(&store, "g").await;
        store
            .upsert_channel(&CachedChannel {
                id: "c".to_owned(),
                guild_id: Some("g".to_owned()),
                ..Default::default()
            })
            .await
            .expect("should write");

        assert_eq!(store.prune_orphans().await.expect("should prune"), 0);
        assert_eq!(store.channels("g").await.expect("read").len(), 1);
    }

    #[tokio::test]
    async fn attachments_of_an_evicted_message_go_too() {
        let store = store().await;
        store
            .upsert_attachment(&CachedAttachment {
                id: "a".to_owned(),
                message_id: "gone".to_owned(),
                ..Default::default()
            })
            .await
            .expect("should write");

        assert_eq!(
            store
                .prune_orphan_attachments()
                .await
                .expect("should prune"),
            1
        );
    }
}

#[cfg(test)]
mod attachment_tests {
    use super::*;

    async fn store() -> Store {
        Store::open(&StorageBackend::Sqlite {
            path: std::path::PathBuf::from(":memory:"),
        })
        .await
        .expect("should open")
    }

    async fn add(store: &Store, id: &str, message: &str) {
        store
            .upsert_attachment(&CachedAttachment {
                id: id.to_owned(),
                message_id: message.to_owned(),
                filename: Some(format!("{id}.png")),
                url: Some(format!("https://cdn.example/{id}.png")),
                size: 10,
                ..Default::default()
            })
            .await
            .expect("should write");
    }

    #[tokio::test]
    async fn attachments_come_back_grouped_by_their_message() {
        let store = store().await;
        add(&store, "1", "m1").await;
        add(&store, "2", "m1").await;
        add(&store, "3", "m2").await;

        let found = store
            .attachments_for(&["m1".to_owned(), "m2".to_owned()])
            .await
            .expect("should read");

        assert_eq!(found["m1"].len(), 2);
        assert_eq!(found["m2"].len(), 1);
    }

    #[tokio::test]
    async fn asking_for_nothing_does_not_build_an_empty_in_clause() {
        // `IN ()` is a syntax error rather than an empty result, and a channel
        // whose every message was skipped asks for exactly that.
        assert!(
            store()
                .await
                .attachments_for(&[])
                .await
                .expect("should read")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_message_with_no_attachments_is_simply_absent() {
        let store = store().await;
        add(&store, "1", "m1").await;

        let found = store
            .attachments_for(&["m2".to_owned()])
            .await
            .expect("should read");
        assert!(!found.contains_key("m2"));
    }

    #[tokio::test]
    async fn a_large_file_size_survives_the_round_trip() {
        // Sizes cross an unsigned-to-signed boundary on the way into the
        // column, and one that wrapped would read back negative.
        let store = store().await;
        let large = 8 * 1024 * 1024 * 1024;
        store
            .upsert_attachment(&CachedAttachment {
                id: "1".to_owned(),
                message_id: "m".to_owned(),
                size: large,
                ..Default::default()
            })
            .await
            .expect("should write");

        let found = store
            .attachments_for(&["m".to_owned()])
            .await
            .expect("should read");
        assert_eq!(found["m"][0].size, large);
    }
}

#[cfg(test)]
mod member_tests {
    use super::*;

    async fn store() -> Store {
        Store::open(&StorageBackend::Sqlite {
            path: std::path::PathBuf::from(":memory:"),
        })
        .await
        .expect("should open")
    }

    fn member(guild: &str, user: &str, name: &str) -> CachedMember {
        CachedMember {
            guild_id: guild.to_owned(),
            user_id: user.to_owned(),
            display_name: Some(name.to_owned()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn the_same_user_in_two_guilds_is_two_rows() {
        // The reason the key is guild and user together: keyed by user alone,
        // joining a second server would overwrite the nickname in the first.
        let store = store().await;
        store
            .upsert_member(&CachedMember {
                nickname: Some("at work".to_owned()),
                ..member("g1", "u1", "sam")
            })
            .await
            .expect("should write");
        store
            .upsert_member(&CachedMember {
                nickname: Some("at home".to_owned()),
                ..member("g2", "u1", "sam")
            })
            .await
            .expect("should write");

        let first = store.members("g1", 10).await.expect("should read");
        let second = store.members("g2", 10).await.expect("should read");
        assert_eq!(first[0].nickname.as_deref(), Some("at work"));
        assert_eq!(second[0].nickname.as_deref(), Some("at home"));
    }

    #[tokio::test]
    async fn roles_survive_the_round_trip() {
        let store = store().await;
        store
            .upsert_member(&CachedMember {
                role_ids: vec!["10".to_owned(), "20".to_owned()],
                ..member("g", "u", "sam")
            })
            .await
            .expect("should write");

        let read = store.members("g", 10).await.expect("should read");
        assert_eq!(read[0].role_ids, ["10", "20"]);
    }

    #[tokio::test]
    async fn a_member_with_no_roles_reads_back_empty_rather_than_blank() {
        // Joining an empty list gives an empty string, and splitting that
        // gives one empty entry - a role id nothing matches.
        let store = store().await;
        store
            .upsert_member(&member("g", "u", "sam"))
            .await
            .expect("should write");

        assert!(
            store.members("g", 10).await.expect("should read")[0]
                .role_ids
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_member_who_left_is_not_drawn() {
        let store = store().await;
        let left = member("g", "u", "sam");
        store.upsert_member(&left).await.expect("should write");
        store
            .tombstone("members", &left.id(), 1)
            .await
            .expect("should tombstone");

        assert!(
            store
                .members("g", 10)
                .await
                .expect("should read")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn the_read_is_capped() {
        let store = store().await;
        for index in 0..5 {
            store
                .upsert_member(&member("g", &index.to_string(), &format!("name{index}")))
                .await
                .expect("should write");
        }

        assert_eq!(store.members("g", 2).await.expect("should read").len(), 2);
    }
}

#[cfg(test)]
mod channel_tests {
    use super::*;

    async fn store() -> Store {
        Store::open(&StorageBackend::Sqlite {
            path: std::path::PathBuf::from(":memory:"),
        })
        .await
        .expect("should open")
    }

    async fn add(store: &Store, id: &str, guild: &str, position: i64) {
        store
            .upsert_channel(&CachedChannel {
                id: id.to_owned(),
                guild_id: Some(guild.to_owned()),
                name: Some(format!("channel-{id}")),
                kind: Some("0".to_owned()),
                position: Some(position),
                ..Default::default()
            })
            .await
            .expect("should write");
    }

    #[tokio::test]
    async fn a_guild_reads_back_only_its_own_channels() {
        let store = store().await;
        add(&store, "1", "guild-a", 0).await;
        add(&store, "2", "guild-b", 0).await;

        let read = store.channels("guild-a").await.expect("should read");
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].id, "1");
    }

    #[tokio::test]
    async fn channels_come_back_in_the_order_the_sidebar_draws_them() {
        // Ordered by the store rather than the caller so the cached sidebar
        // and the live one agree: a different order would look like the
        // channels had moved and then moved back.
        let store = store().await;
        add(&store, "10", "g", 2).await;
        add(&store, "20", "g", 0).await;
        add(&store, "30", "g", 1).await;

        let order: Vec<String> = store
            .channels("g")
            .await
            .expect("should read")
            .into_iter()
            .map(|channel| channel.id)
            .collect();
        assert_eq!(order, ["20", "30", "10"]);
    }

    #[tokio::test]
    async fn a_channel_recreated_after_a_delete_comes_back() {
        // The failure this guards: with a fixed revision the tombstone always
        // outranks the create, so a rejoined guild or a recreated channel
        // stays invisible until the cache is deleted by hand.
        let store = store().await;
        add(&store, "1", "g", 0).await;

        let deleted_at = crate::persist::wall_clock_revision();
        store
            .tombstone("channels", "1", deleted_at)
            .await
            .expect("should tombstone");
        assert!(store.channels("g").await.expect("should read").is_empty());

        store
            .upsert_channel(&CachedChannel {
                id: "1".to_owned(),
                guild_id: Some("g".to_owned()),
                name: Some("back".to_owned()),
                revision: deleted_at + 1,
                ..Default::default()
            })
            .await
            .expect("should write");

        let read = store.channels("g").await.expect("should read");
        assert_eq!(read.len(), 1, "the recreated channel should be drawn again");
    }

    #[tokio::test]
    async fn a_deleted_channel_is_not_drawn() {
        let store = store().await;
        add(&store, "1", "g", 0).await;
        store
            .tombstone("channels", "1", 1)
            .await
            .expect("should tombstone");

        assert!(store.channels("g").await.expect("should read").is_empty());
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "concord-migration-{name}-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[tokio::test]
    async fn a_store_from_the_previous_build_gains_the_new_column() {
        // The case this exists for: version 1 shipped without `has_extras`,
        // and `CREATE TABLE IF NOT EXISTS` cannot add a column to a table that
        // is already there. Without the migration every read of that column
        // fails and the cache silently stops working.
        let path = temp_path("upgrade");
        let backend = StorageBackend::Sqlite { path: path.clone() };

        {
            let store = Store::open(&backend).await.expect("should open");
            sqlx::query("ALTER TABLE messages DROP COLUMN has_extras")
                .execute(&store.pool)
                .await
                .expect("should drop");
            sqlx::query("DELETE FROM schema_meta")
                .execute(&store.pool)
                .await
                .expect("should clear");
        }

        let store = Store::open(&backend).await.expect("should reopen");
        store
            .upsert_message(&CachedMessage {
                id: "1".to_owned(),
                channel_id: "c".to_owned(),
                has_extras: true,
                ..Default::default()
            })
            .await
            .expect("should write");

        let read = store.recent_messages("c", 10).await.expect("should read");
        assert_eq!(read.len(), 1);
        assert!(read[0].has_extras);
        assert_eq!(
            store.schema_version().await.expect("should read"),
            Some(SCHEMA_VERSION)
        );

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_store_from_a_newer_build_is_refused_rather_than_written_to() {
        // A shared store may be in use by a client this build knows nothing
        // about. Writing rows it cannot read would corrupt its cache, so this
        // one goes without rather than guess.
        let path = temp_path("newer");
        let backend = StorageBackend::Sqlite { path: path.clone() };

        {
            let store = Store::open(&backend).await.expect("should open");
            sqlx::query("UPDATE schema_meta SET value = ? WHERE name = ?")
                .bind(i64::from(SCHEMA_VERSION) + 1)
                .bind(SCHEMA_VERSION_KEY)
                .execute(&store.pool)
                .await
                .expect("should stamp");
        }

        let Err(refused) = Store::open(&backend).await else {
            panic!("a newer store should not be opened");
        };
        // The marker is what the client keys the visible warning off, so a
        // reworded message that dropped it would silently downgrade this back
        // to an invisible debug line.
        assert!(refused.to_string().contains(NEWER_STORE_MARKER));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_fresh_store_is_stamped_with_this_version() {
        let store = Store::open(&StorageBackend::Sqlite {
            path: std::path::PathBuf::from(":memory:"),
        })
        .await
        .expect("should open");

        assert_eq!(
            store.schema_version().await.expect("should read"),
            Some(SCHEMA_VERSION)
        );
    }

    #[tokio::test]
    async fn opening_the_same_store_twice_does_not_migrate_twice() {
        // Re-running the migration would try to add a column that is already
        // there; the step has to be forgiving of that rather than fail.
        let path = temp_path("twice");
        let backend = StorageBackend::Sqlite { path: path.clone() };

        Store::open(&backend).await.expect("first open");
        Store::open(&backend).await.expect("second open");

        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod eviction_tests {
    use super::*;

    async fn store() -> Store {
        Store::open(&crate::StorageBackend::Sqlite {
            path: std::path::PathBuf::from(":memory:"),
        })
        .await
        .expect("should open")
    }

    async fn add(store: &Store, channel: &str, id: &str) {
        store
            .upsert_message(&CachedMessage {
                id: id.to_owned(),
                channel_id: channel.to_owned(),
                content: Some(id.to_owned()),
                ..Default::default()
            })
            .await
            .expect("should write");
    }

    #[tokio::test]
    async fn the_oldest_messages_in_a_channel_go_first() {
        let store = store().await;
        for id in ["100", "200", "300", "400"] {
            add(&store, "c", id).await;
        }

        assert_eq!(store.prune_messages(2).await.expect("should prune"), 2);

        let kept: Vec<String> = store
            .recent_messages("c", 10)
            .await
            .expect("should read")
            .into_iter()
            .map(|message| message.id)
            .collect();
        assert!(kept.contains(&"400".to_owned()) && kept.contains(&"300".to_owned()));
        assert!(!kept.contains(&"100".to_owned()));
    }

    #[tokio::test]
    async fn a_busy_channel_does_not_evict_a_quiet_one() {
        // The reason this is per channel: a global cap would empty the
        // channel the cache is most useful for.
        let store = store().await;
        for id in ["100", "200", "300", "400", "500"] {
            add(&store, "busy", id).await;
        }
        add(&store, "quiet", "600").await;

        store.prune_messages(2).await.expect("should prune");

        assert_eq!(
            store
                .recent_messages("quiet", 10)
                .await
                .expect("read")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn a_shorter_id_is_older_rather_than_later() {
        // Ids are text, so lexicographic ordering would sort "9" after "10"
        // and evict the newest message. Discord's ids are all one length
        // today, which is why this would go unnoticed until they are not.
        let store = store().await;
        add(&store, "c", "9").await;
        add(&store, "c", "10").await;

        store.prune_messages(1).await.expect("should prune");

        let kept = store.recent_messages("c", 10).await.expect("should read");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "10");
    }

    #[tokio::test]
    async fn pruning_an_empty_store_is_not_an_error() {
        assert_eq!(
            store()
                .await
                .prune_messages(50)
                .await
                .expect("should prune"),
            0
        );
    }
}

#[cfg(test)]
mod hydration_tests {
    use super::*;

    /// The whole point, end to end: what a run wrote, the next run reads.
    ///
    /// Two stores over one file rather than one store used twice, because the
    /// question is whether it survives the process, not whether a pool is
    /// consistent with itself.
    #[tokio::test]
    async fn what_one_run_cached_the_next_run_reads() {
        let dir = std::env::temp_dir().join(format!("concord-hydration-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("cache.db");
        let backend = StorageBackend::Sqlite { path };

        {
            let first = Store::open(&backend).await.expect("first run should open");
            first
                .upsert_guild(&CachedGuild {
                    id: "7".to_owned(),
                    name: Some("Rustaceans".to_owned()),
                    revision: 1,
                    ..CachedGuild::default()
                })
                .await
                .expect("write");
            first
                .upsert_message(&CachedMessage {
                    id: "100".to_owned(),
                    channel_id: "c".to_owned(),
                    content: Some("still here".to_owned()),
                    revision: 0,
                    ..CachedMessage::default()
                })
                .await
                .expect("write");
        }

        let second = Store::open(&backend).await.expect("second run should open");
        let guilds = second.guilds().await.expect("read");
        assert_eq!(guilds.len(), 1, "the sidebar would be empty on restart");
        assert_eq!(guilds[0].name.as_deref(), Some("Rustaceans"));

        let messages = second.recent_messages("c", 10).await.expect("read");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content.as_deref(), Some("still here"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_store_whose_directory_does_not_exist_is_created() {
        // Every first run is this case, and one that failed here would never
        // cache anything at all.
        let dir = std::env::temp_dir().join(format!("concord-fresh-{}/nested", std::process::id()));
        let _ = std::fs::remove_dir_all(dir.parent().unwrap_or(&dir));

        let store = Store::open(&StorageBackend::Sqlite {
            path: dir.join("cache.db"),
        })
        .await
        .expect("a first run should create its store");
        assert!(store.guilds().await.expect("read").is_empty());

        let _ = std::fs::remove_dir_all(dir.parent().unwrap_or(&dir));
    }
}
