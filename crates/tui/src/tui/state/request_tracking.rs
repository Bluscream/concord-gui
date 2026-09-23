use std::collections::{HashMap, HashSet, VecDeque};

use concord::discord::AppCommand;
use concord::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker},
};

use super::DashboardState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LatestMessageHistoryState {
    Loading,
    Loaded,
    Failed,
}

#[derive(Debug, Default)]
pub(super) struct RequestTrackingState {
    latest_message_history: HashMap<Id<ChannelMarker>, LatestMessageHistoryState>,
    pub(super) pending_commands: VecDeque<AppCommand>,
    /// Addresses already sent to the embed proxy this session.
    ///
    /// A page that has no preview will not grow one while the client is open,
    /// so asking twice is a request nobody reads.
    requested_embeds: HashSet<String>,
}

impl DashboardState {
    pub(in crate::tui) fn drain_pending_commands(&mut self) -> Vec<AppCommand> {
        self.requests.pending_commands.drain(..).collect()
    }

    pub(in crate::tui) fn enqueue_pending_command(&mut self, command: AppCommand) {
        self.requests.pending_commands.push_back(command);
    }

    /// Ask the embed proxy about links in messages Discord did not preview.
    ///
    /// Only messages that came back with no embed at all: Discord's own
    /// unfurl is authoritative, and a second opinion beside the real one
    /// would have nothing to tell them apart.
    pub(super) fn queue_missing_embed_resolves(
        &mut self,
        channel_id: Id<ChannelMarker>,
        messages: &[concord::discord::MessageInfo],
    ) {
        let proxy = self.options.embed_options.proxy.trim().to_owned();
        if proxy.is_empty() {
            return;
        }

        let mut wanted = Vec::new();
        for message in messages {
            if !message.embeds.is_empty() {
                continue;
            }
            let Some(content) = message.content.as_deref() else {
                continue;
            };
            for url in concord::app::links_in(content) {
                if self.requests.requested_embeds.insert(url.clone()) {
                    wanted.push((message.message_id, url));
                }
            }
        }

        for (message_id, url) in wanted {
            self.enqueue_pending_command(AppCommand::ResolveEmbed {
                channel_id,
                message_id,
                url,
                proxy: proxy.clone(),
            });
        }
    }

    pub(super) fn queue_application_command_load(&mut self, guild_id: Option<Id<GuildMarker>>) {
        self.enqueue_pending_command(AppCommand::LoadApplicationCommands { guild_id });
    }

    pub(super) fn queue_ack_channel_command(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        self.enqueue_pending_command(AppCommand::AckChannel {
            channel_id,
            message_id,
        });
    }

    pub(super) fn queue_scheduled_ack_channel_command(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        self.enqueue_pending_command(AppCommand::ScheduleAckChannel {
            channel_id,
            message_id,
        });
    }

    pub(super) fn queue_ack_channels_command(
        &mut self,
        targets: Vec<(Id<ChannelMarker>, Id<MessageMarker>)>,
    ) {
        self.enqueue_pending_command(AppCommand::AckChannels { targets });
    }

    pub(super) fn record_latest_message_history_loaded(&mut self, channel_id: Id<ChannelMarker>) {
        self.requests
            .latest_message_history
            .insert(channel_id, LatestMessageHistoryState::Loaded);
    }

    pub(super) fn record_latest_message_history_loading(&mut self, channel_id: Id<ChannelMarker>) {
        self.requests
            .latest_message_history
            .insert(channel_id, LatestMessageHistoryState::Loading);
    }

    pub(super) fn record_latest_message_history_failed(&mut self, channel_id: Id<ChannelMarker>) {
        self.requests
            .latest_message_history
            .insert(channel_id, LatestMessageHistoryState::Failed);
    }

    pub(super) fn latest_message_history_state(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> LatestMessageHistoryState {
        self.requests
            .latest_message_history
            .get(&channel_id)
            .copied()
            .unwrap_or(LatestMessageHistoryState::Loading)
    }
}
