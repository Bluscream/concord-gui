//! One view across several signed-in accounts.
//!
//! Discord's own clients make you pick an account and see only that one. A
//! person with a work account and a personal one reads two inboxes and misses
//! things in whichever they are not looking at. This crate is for showing both
//! at once.
//!
//! It sits between the core and a front end, like `concord-cache`: one
//! `DiscordClient` per account, each knowing only its own session, and this
//! crate deciding what a combined list looks like. The core is not aware there
//! is more than one.
//!
//! # What is decided
//!
//! The hard part is not plumbing, it is identity: when two accounts both see
//! something, is it one thing or two? [`identity`] answers that, and the
//! answer differs per entity type in ways that are not obvious. Everything
//! else depends on getting it right, which is why it is the first thing here.
//!
//! # What is not decided yet
//!
//! - Sending. A message goes *from* an account, so a merged view has to know
//!   which one, and picking wrong sends from the wrong identity. That is a
//!   worse failure than anything on the read side and wants its own design.
//! - Notification and read state, which are per-account and per-entity at once.
//! - Voice, where being in two calls from one screen may not be coherent.

pub mod account;
pub mod identity;

pub use account::{AccountId, AccountLabel};
pub use identity::{Shared, SharedBy};
