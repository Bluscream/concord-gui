use super::super::*;

use concord::discord::{
    AppCommand, AppEvent, AttachmentDownloadId, DownloadAttachmentSource, Id, MediaPlaybackSource,
    MediaPlaybackTarget, MessageHistoryAfterMode, MuteDuration, ReactionEmoji, marker,
    next_message_nonce,
};

use crate::model::projection::{self, Selection};

use crate::ui::switcher::Switcher;
use crate::ui::workspace::SwitcherPurpose;

impl Workspace {
    pub fn set_thread_archived(&mut self, archived: bool) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        handle.send(AppCommand::SetThreadArchived {
            channel_id,
            archived,
            label: String::new(),
        });
    }

    /// Follow or unfollow the open thread, which controls whether its
    /// activity reaches the sidebar at all.
    pub fn set_thread_followed(&mut self, followed: bool) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        handle.send(AppCommand::SetThreadFollowed {
            channel_id,
            followed,
            label: String::new(),
        });
    }

    /// Play an attachment in the configured external player.
    ///
    /// Upstream shells out to mpv rather than decoding in-process, so this
    /// opens a separate window. That is stated in the UI rather than dressed
    /// up as inline playback.
    pub fn play_attachment(&mut self, index: usize, attachment: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(file) = self
            .messages
            .get(index)
            .and_then(|row| row.attachments.get(attachment))
        else {
            return;
        };

        if file.url.is_empty() {
            self.model.status_line = format!("{} has no source to play", file.filename);
            return;
        }

        if !self.options.display.media_playback {
            // Enabled explicitly rather than assumed: playback launches an
            // external process, which is not something to do unasked.
            self.model.status_line =
                "Enable media playback in settings to open this externally".to_string();
            return;
        }

        handle.send(AppCommand::PlayMedia {
            target: MediaPlaybackTarget {
                url: file.url.clone(),
                label: file.filename.clone(),
                source: MediaPlaybackSource::Message,
            },
            request_id: None,
        });
        self.model.status_line = format!("Opening {} externally…", file.filename);
    }

    /// Download an attachment to the user's download directory.
    pub fn download_attachment(&mut self, index: usize, attachment: usize) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Some(row) = self.messages.get(index) else {
            return;
        };
        let Some(file) = row.attachments.get(attachment) else {
            return;
        };

        // A demo attachment carries no URL, since nothing was uploaded; there
        // is nothing to fetch, so this reports rather than failing opaquely.
        if file.url.is_empty() {
            self.model.status_line = format!("{} has no source to download", file.filename);
            return;
        }

        handle.send(AppCommand::DownloadAttachment {
            id: AttachmentDownloadId::new(row.id.get()),
            url: file.url.clone(),
            filename: file.filename.clone(),
            source: DownloadAttachmentSource::AttachmentViewer,
        });
    }

    /// Pin or unpin a message.
    pub fn set_pinned(&mut self, index: usize, pinned: bool) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let Some(row) = self.messages.get(index) else {
            return;
        };

        handle.send(AppCommand::SetMessagePinned {
            channel_id,
            message_id: row.id,
            pinned,
        });
    }

    /// Open the pinned-messages panel for the current channel.
    pub fn open_pins(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        self.pins = Some(Vec::new());
        handle.send(AppCommand::LoadPinnedMessages { channel_id });
    }

    /// Mute or unmute the open channel.
    ///
    /// Permanent rather than timed: a timed mute needs a duration picker, and
    /// silently choosing one for the user would be worse than not offering it.
    pub fn toggle_channel_muted(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let guild_id = match self.nav.selection {
            Selection::Guild(id) => Some(id),
            Selection::DirectMessages => None,
        };

        let muted = !self.channel_muted;
        self.channel_muted = muted;

        handle.send(AppCommand::SetChannelMuted {
            guild_id,
            channel_id,
            muted,
            duration: Some(MuteDuration::Permanent),
            label: String::new(),
        });
    }

    /// Mute or unmute the open guild.
    pub fn toggle_guild_muted(&mut self) {
        let (Some(handle), Selection::Guild(guild_id)) = (&self.handle, self.nav.selection) else {
            return;
        };

        let muted = !self.guild_muted;
        self.guild_muted = muted;

        handle.send(AppCommand::SetGuildMuted {
            guild_id,
            muted,
            duration: Some(MuteDuration::Permanent),
            label: String::new(),
        });
    }

    /// A discord.com link to a message in the open channel.
    ///
    /// DMs use the `@me` sentinel in place of a guild id, matching Discord's
    /// own link format - a DM link built with a guild id resolves to nothing.
    pub fn message_link(&self, message_id: Id<marker::MessageMarker>) -> String {
        let guild = match self.nav.selection {
            Selection::Guild(id) => id.get().to_string(),
            Selection::DirectMessages => "@me".to_string(),
        };
        let channel = self
            .nav
            .channel
            .map(|id| id.get().to_string())
            .unwrap_or_default();

        format!(
            "https://discord.com/channels/{guild}/{channel}/{}",
            message_id.get()
        )
    }

    /// Ask who reacted with a given emoji.
    pub fn show_reaction_users(&mut self, message: usize, reaction: usize) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let Some(row) = self.messages.get(message) else {
            return;
        };
        let Some((glyph, _, _)) = row.reactions.get(reaction) else {
            return;
        };

        // Custom emoji round-trip as :name:, which is not a reaction identity
        // the API accepts, so only unicode reactions can be queried.
        if glyph.starts_with(':') {
            return;
        }

        self.reaction_users = Some((row.id, glyph.clone(), Vec::new()));
        handle.send(AppCommand::LoadReactionUsers {
            channel_id,
            message_id: row.id,
            emoji: ReactionEmoji::Unicode(glyph.clone()),
            after: None,
        });
    }

    /// Open the quick switcher, seeded with the full candidate list.
    pub fn open_switcher(&mut self) {
        self.open_switcher_for(SwitcherPurpose::Navigate);
    }

    /// Open the switcher to pick a channel for something other than navigating.
    ///
    /// Forwarding needs exactly the picker the switcher already is - every
    /// channel across every guild, fuzzy-ranked - so it reuses it rather than
    /// growing a second one that would rank differently.
    pub fn open_switcher_for(&mut self, purpose: SwitcherPurpose) {
        let mut switcher = Switcher::default();
        if let Some(state) = &self.last_state {
            switcher.rank(projection::switcher_candidates(state));
        }
        self.switcher_purpose = purpose;
        self.switcher = Some(switcher);
    }

    /// Begin forwarding a message: pick the destination.
    pub fn start_forward(&mut self, index: usize) {
        let Some(row) = self.messages.get(index) else {
            return;
        };
        let source = (row.id, self.nav.channel);
        let Some(channel_id) = source.1 else {
            return;
        };

        self.open_switcher_for(SwitcherPurpose::Forward {
            message_id: source.0,
            source_channel_id: channel_id,
        });
    }

    /// Re-rank after the query changes.
    pub fn rerank_switcher(&mut self) {
        let Some(state) = self.last_state.clone() else {
            return;
        };
        if let Some(switcher) = &mut self.switcher {
            switcher.rank(projection::switcher_candidates(&state));
        }
    }

    /// Jump to the highlighted candidate.
    pub fn activate_switcher(&mut self) {
        let Some(target) = self
            .switcher
            .as_ref()
            .and_then(|switcher| switcher.selection())
            .map(|candidate| (candidate.channel_id, candidate.guild_id))
        else {
            return;
        };

        self.switcher = None;

        // Forwarding consumes the selection instead of navigating to it: the
        // point is to send the message elsewhere, not to go there.
        if let SwitcherPurpose::Forward {
            message_id,
            source_channel_id,
        } = std::mem::take(&mut self.switcher_purpose)
        {
            if let Some(handle) = &self.handle {
                let source_guild_id = match self.nav.selection {
                    Selection::Guild(guild_id) => Some(guild_id),
                    Selection::DirectMessages => None,
                };
                handle.send(AppCommand::ForwardMessage {
                    source_channel_id,
                    source_guild_id,
                    message_id,
                    target_channel_id: target.0,
                    nonce: next_message_nonce(),
                });
                self.model.status_line = "Forwarded".to_string();
            }
            return;
        }

        // Switching guild first keeps the sidebar and the open channel
        // consistent; opening the channel alone would leave the wrong guild
        // selected and its channel list showing.
        let (channel_id, guild_id) = target;
        if self.nav.selection != guild_id.map_or(Selection::DirectMessages, Selection::Guild) {
            self.open_guild(guild_id);
        }
        self.forum = None;
        self.open_channel(channel_id);
    }

    /// Mark the open channel read up to its newest message.
    ///
    /// Without this, unread badges accumulate with no way to clear them - the
    /// counts are correct but permanently rising, which is worse than not
    /// showing them.
    pub fn mark_read(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let Some(newest) = self.messages.last().map(|row| row.id) else {
            return;
        };

        handle.send(AppCommand::AckChannel {
            channel_id,
            message_id: newest,
        });
    }

    /// Mark every unread channel in view read.
    ///
    /// Batched into one command rather than one per channel: the core accepts
    /// a list, and a burst of individual acks is exactly the traffic pattern
    /// that gets a third-party client flagged.
    pub fn mark_all_read(&mut self) {
        let Some(handle) = &self.handle else {
            return;
        };

        let targets: Vec<_> = self
            .model
            .channels
            .iter()
            .filter(|channel| channel.unread)
            .filter_map(|channel| {
                // Acking needs a message to ack up to; a channel whose last
                // message is unknown is skipped rather than guessed at.
                channel.id.zip(channel.last_message)
            })
            .collect();

        if targets.is_empty() {
            return;
        }

        handle.send(AppCommand::AckChannels { targets });
    }

    /// Request the page of messages before the oldest one loaded.
    ///
    /// The message cache is lazily populated, so scrollback exists only if it
    /// is asked for. Without this the log stops at whatever the initial fetch
    /// returned, which is far short of the TUI's unlimited scrollback.
    pub fn load_older_messages(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let Some(oldest) = self.messages.first().map(|row| row.id) else {
            return;
        };

        handle.send(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(oldest),
        });
    }

    /// Request the page of messages after the newest one loaded.
    ///
    /// Needed whenever the loaded range is not anchored to the live end of the
    /// channel: jumping to a search result or an inbox mention lands mid-history,
    /// and without forward paging the view is stuck there.
    pub fn load_newer_messages(&mut self, mode: MessageHistoryAfterMode) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let Some(newest) = self.messages.last().map(|row| row.id) else {
            return;
        };

        handle.send(AppCommand::LoadMessageHistoryAfter {
            channel_id,
            after: newest,
            mode,
        });
    }

    /// Whether the channel has messages newer than the loaded range.
    ///
    /// Compared against the channel's own `last_message_id` rather than a
    /// scroll position: after jumping to a search result the view is at the
    /// bottom of what is loaded, which is not the bottom of the channel.
    pub fn has_newer_messages(&self) -> bool {
        let (Some(channel_id), Some(newest)) =
            (self.nav.channel, self.messages.last().map(|row| row.id))
        else {
            return false;
        };

        self.model
            .channels
            .iter()
            .find(|channel| channel.id == Some(channel_id))
            .and_then(|channel| channel.last_message)
            .is_some_and(|last| last > newest)
    }

    /// Re-fetch the open channel from scratch.
    ///
    /// The gateway can drop messages across a reconnect, leaving a hole that no
    /// amount of scrolling fills, because both paging directions extend from
    /// what is already cached.
    pub fn refresh_history(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        handle.send(AppCommand::RefreshMessageHistory { channel_id });
    }

    /// Mark the open channel read, after a delay.
    ///
    /// Used when the newest message arrives while the channel is on screen.
    /// An immediate ack would race the user's eyes - and, sent on every
    /// incoming message, would be a request per message.
    pub fn schedule_mark_read(&mut self) {
        let (Some(handle), Some(channel_id)) = (&self.handle, self.nav.channel) else {
            return;
        };
        let Some(newest) = self.messages.last().map(|row| row.id) else {
            return;
        };

        handle.send(AppCommand::ScheduleAckChannel {
            channel_id,
            message_id: newest,
        });
    }

    /// Ask the server for members matching a query.
    ///
    /// The member list only holds the windowed ranges this client subscribed
    /// to, so a mention for someone further down it has never seen would
    /// otherwise not resolve.
    pub fn search_members(&mut self, query: String) {
        let Some(handle) = &self.handle else {
            return;
        };
        let Selection::Guild(guild_id) = self.nav.selection else {
            return;
        };
        if query.trim().is_empty() {
            return;
        }

        handle.send(AppCommand::SearchGuildMembers {
            guild_id,
            query,
            limit: 25,
        });
    }

    /// Fetch members referenced on screen but absent from the cache.
    ///
    /// Without this an unhydrated author renders as a raw id, and a mention of
    /// someone outside the subscribed window stays unresolved.
    ///
    /// The demand comes from the core rather than from a scan of the visible
    /// rows: it already tracks voice, typing and thread participants too, and
    /// a second heuristic here would drift from the one the TUI uses.
    pub fn hydrate_missing_members(&mut self) {
        let (Some(handle), Some(state)) = (&self.handle, self.last_state.as_ref()) else {
            return;
        };
        let selected = match self.nav.selection {
            Selection::Guild(guild_id) => Some(guild_id),
            Selection::DirectMessages => None,
        };

        for (guild_id, user_ids) in
            state.missing_member_hydration_requests(selected, std::time::Instant::now())
        {
            handle.send(AppCommand::LoadGuildMembersByIds { guild_id, user_ids });
        }
    }

    /// Switch the open guild, clearing the channel selection.
    pub fn open_guild(&mut self, guild_id: Option<Id<marker::GuildMarker>>) {
        self.nav.selection = match guild_id {
            Some(id) => Selection::Guild(id),
            None => Selection::DirectMessages,
        };
        self.nav.channel = None;
        self.messages.clear();
        // Commands are per guild, so the previous guild's set must not linger.
        self.app_commands.clear();
        self.load_app_commands();

        if let (Some(handle), Some(guild_id)) = (&self.handle, guild_id) {
            handle.send(AppCommand::SetSelectedGuild {
                guild_id: Some(guild_id),
            });
        }
        self.reproject();
    }

    /// Fold a discrete event into the model.
    ///
    /// Most state arrives through reprojection; this handles only what is not
    /// represented in the state store, such as transient errors.
    pub fn absorb(&mut self, event: AppEvent) {
        self.absorb_event(event);
    }
}
