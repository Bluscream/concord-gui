use std::collections::HashSet;

use concord::discord::ids::marker::{GuildMarker, UserMarker};

use concord::discord::{
    AppCommand, AppEvent, ForumPostArchiveState, MentionInfo, MessageSnapshotInfo,
};
mod channel_rows;
mod channel_tree;
mod channels;
mod composer;
mod dashboard;
mod diagnostics;
mod discord_ui;
mod emoji;
mod events;
mod guilds;
mod layout_cache;
mod local_upload_preview;
mod member_grouping;
mod message_history_refresh;
mod message_layout;
mod message_render;
mod message_viewport;
mod model;
mod navigation;
mod options;
mod pane_filter;
mod pending_messages;
mod popups;
mod presentation;
mod request_tracking;
mod runtime_state;
mod scroll;
mod stream_info;
mod subscriptions;
mod tabs;
mod text_completion;
mod toast;
mod user;
mod voice_actions;

use composer::ComposerUiState;
use discord_ui::DiscordUiState;
use layout_cache::{LayoutCacheState, MessageRowContentMetrics, MessageRowContentMetricsCacheKey};
use message_history_refresh::MessageHistoryRefreshState;
use message_render::{add_literal_mention_highlights, normalize_text_highlights};
use message_viewport::{MessageViewportState, ThreadReturnTarget};
use model::ChannelPaneCursor;
use navigation::{ActiveGuildScope, FolderKey, FolderSettingsState, NavigationState};
use options::SettingsState;
use pane_filter::PaneFilterState;
use pending_messages::PendingMessageUiState;
use popups::PopupUiState;
use request_tracking::RequestTrackingState;
use runtime_state::{
    MediaPlaybackPreparingUiState, RuntimeUiState, StreamBroadcastUiTarget, StreamPlaybackUiTarget,
    ToastMessage, VoiceConnectionUiState,
};
pub(in crate::tui) use scroll::SCROLL_OFF;
use scroll::clamp_selected_index;

pub(in crate::tui) const MINIMUM_ESTABLISHED_DM_MESSAGES: usize = 5;

pub(in crate::tui) use channel_rows::ChannelPaneRow;
pub use composer::{
    CommandPickerEntry, ComposerLock, EmojiPickerEntry, MAX_MENTION_PICKER_VISIBLE,
    MentionPickerEntry, MentionPickerTarget,
};
pub use dashboard::DashboardState;
pub use member_grouping::{MemberEntry, MemberGroup};
pub use message_viewport::MessagePaneSource;
#[cfg(test)]
pub(crate) use model::ActionAvailability;
/// Re-exported wholesale rather than named one by one. The list lives in
/// `concord-ui` now, and a second copy here would drift from it every time a
/// type is added.
pub use model::*;
pub use options::{DisplayOptionGauge, DisplayOptionItem};
pub(in crate::tui) use popups::{
    ActiveModalPopupKind, ConfirmationButton, JoinServerState, MessageConfirmationKind,
    PopupInputMode, PopupKeymapContext, SelectablePopupSnapshot, SelectablePopupTarget,
    VoiceParticipantAudioField,
};
// ChannelField is named only by this crate's own tests, which is why clippy
// reads the re-export as unused - removing it breaks them.
#[allow(unused_imports)]
pub use popups::{
    AttachmentViewerZoom, ChannelEditPurpose, ChannelField, EmojiEdit, EmojiReactionPickerState,
    MessageActionMenuState, MessageUrlPickerState, NotificationInboxChannelLoad,
    NotificationInboxItem, NotificationInboxLoad, NotificationInboxMessage, NotificationInboxTab,
    NotificationInboxUnreadItem, PollVotePickerState, ReactionUsersEntry, ReactionUsersPopupState,
    ServerPanelTab, UserProfileSettingsField, UserProfileSettingsTab,
};
pub(in crate::tui) use presentation::primary_compact_activity;
pub use presentation::{
    apply_discord_foreground, discord_role_mention_background, folder_style, normal_text_style,
    presence_marker, presence_style,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToastView<'a> {
    pub text: &'a str,
    pub kind: ToastKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopNotification {
    pub title: String,
    pub body: String,
}

fn message_notification_body(
    content: Option<&str>,
    sticker_count: usize,
    attachment_count: usize,
    embed_count: usize,
) -> String {
    let content = content.unwrap_or_default().trim();
    if !content.is_empty() {
        let single_line = content.split_whitespace().collect::<Vec<_>>().join(" ");
        return truncate_notification_text(&single_line, 200);
    }
    if attachment_count > 0 {
        return format!("sent {attachment_count} attachment(s)");
    }
    if sticker_count > 0 {
        return format!("sent {sticker_count} sticker(s)");
    }
    if embed_count > 0 {
        return format!("sent {embed_count} embed(s)");
    }
    "sent a message".to_owned()
}

fn truncate_notification_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

impl DashboardState {
    /// Drives `event` the way the running app does: the client applies it to
    /// its own state and advances the affected revisions, the TUI reattaches
    /// the moved snapshot areas, and only then does the event reach the UI as
    /// an effect. Tests call this so they exercise the snapshot path rather
    /// than a shortcut that writes the TUI's cache directly.
    #[cfg(test)]
    pub fn push_event(&mut self, event: AppEvent) {
        if let Some(areas) = event.snapshot_areas() {
            let previous_revision = self.discord.authoritative_revision;
            self.discord.authoritative_revision =
                self.discord
                    .authoritative
                    .apply_event_advancing(&event, areas, previous_revision);
            let snapshot = self
                .discord
                .authoritative
                .snapshot(self.discord.authoritative_revision);
            self.restore_discord_snapshot_areas(&snapshot, previous_revision);
        }

        if event.needs_effect_delivery() {
            self.push_effect(event);
        }
    }

    pub fn push_effect(&mut self, event: AppEvent) {
        if let AppEvent::ChannelUpsert(channel) = &event {
            self.record_thread_channel_upserted(channel);
            return;
        }
        self.push_event_inner(event);
    }
}

#[cfg(test)]
mod tests;
