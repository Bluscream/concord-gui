use std::{fmt, marker::PhantomData, num::NonZeroU64};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

// Type adapted from Twilight <https://github.com/twilight-rs/twilight>
// ISC License (ISC)
// Copyright (c) 2025 Twilight Contributors
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(transparent)]
pub struct Id<T> {
    value: NonZeroU64,
    marker: PhantomData<fn(T) -> T>,
}

impl<T> Id<T> {
    pub const fn new(value: u64) -> Self {
        let Some(value) = NonZeroU64::new(value) else {
            panic!("Discord snowflake ids must be non-zero");
        };
        Self {
            value,
            marker: PhantomData,
        }
    }

    pub const fn new_checked(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self {
                value,
                marker: PhantomData,
            }),
            None => None,
        }
    }

    /// The first millisecond Discord will issue an id for.
    ///
    /// Snowflakes count from here rather than from the Unix epoch, so the
    /// timestamp has to be shifted before it means anything.
    pub const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

    /// Build a snowflake for a moment in time.
    ///
    /// Discord's layout: 42 bits of milliseconds since its own epoch, then a
    /// worker, a process and an increment. The lower bits carry no meaning to
    /// a client, but they are not always zero on a real id, and an id that is
    /// suspiciously round is the sort of thing that only shows up as a bug
    /// once something starts sorting by it.
    pub const fn from_parts(created_ms: u64, worker: u64, process: u64, increment: u64) -> Self {
        let elapsed = created_ms.saturating_sub(Self::DISCORD_EPOCH_MS);
        let value = (elapsed << 22)
            | ((worker & 0b1_1111) << 17)
            | ((process & 0b1_1111) << 12)
            | (increment & 0xFFF);
        Self::new(value)
    }

    /// When Discord would have issued this id, in milliseconds.
    pub const fn created_at_ms(self) -> u64 {
        (self.value.get() >> 22) + Self::DISCORD_EPOCH_MS
    }

    /// Whether this could be an id Discord actually issued.
    ///
    /// Checks the only part of a snowflake that carries meaning: the
    /// timestamp. It has to be after Discord existed and not in the future.
    ///
    /// The point is to catch a number that was never a snowflake - a test
    /// fixture's `1001`, an array index that leaked into an id, a truncated
    /// parse - rather than to authenticate anything. A small number decodes
    /// to a timestamp at or near Discord's epoch, which is how they are
    /// caught: id 1001 claims to have been created in January 2015, in the
    /// same millisecond as every other small number.
    pub fn is_plausible(self, now_ms: u64) -> bool {
        let created = self.created_at_ms();
        // A day of slack, for a clock that is behind the server's.
        created > Self::DISCORD_EPOCH_MS && created <= now_ms.saturating_add(86_400_000)
    }

    pub const fn get(self) -> u64 {
        self.value.get()
    }
}

impl<T> fmt::Display for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.get().fmt(f)
    }
}

impl<T> Serialize for Id<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.value.get().to_string())
    }
}

impl<'de, T> Deserialize<'de> for Id<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct IdVisitor<T>(PhantomData<fn(T) -> T>);

        impl<T> de::Visitor<'_> for IdVisitor<T> {
            type Value = Id<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a non-zero Discord snowflake as a string or integer")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Id::new_checked(value).ok_or_else(|| E::custom("Discord snowflake id was zero"))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let value = value.parse::<u64>().map_err(E::custom)?;
                self.visit_u64(value)
            }
        }

        deserializer.deserialize_any(IdVisitor(PhantomData))
    }
}

pub mod marker {
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct ApplicationMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct AttachmentMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct ChannelMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct EmojiMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct GuildMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct ForumTagMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct MessageMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct RoleMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct StickerMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct UserMarker;

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct WebhookMarker;
}
