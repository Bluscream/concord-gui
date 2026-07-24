use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::Semaphore;
use tokio::task::AbortHandle;

use crate::{DiscordClient, discord::AppCommand};

use super::{
    gateway_commands, history_commands, inbox_commands, media_commands, message_commands,
    notification_commands, read_state_commands, session_commands, user_commands, voice_commands,
};

const MAX_CONCURRENT_ATTACHMENT_PREVIEWS: usize = 4;
const MAX_CONCURRENT_ATTACHMENT_DOWNLOADS: usize = 2;
const APPLICATION_COMMAND_AUTOCOMPLETE_DEBOUNCE: Duration = Duration::from_millis(150);

#[derive(Clone, Default)]
struct AutocompleteRequestScheduler {
    pending: Arc<Mutex<Option<AbortHandle>>>,
}

impl AutocompleteRequestScheduler {
    fn replace(&self, request: impl Future<Output = ()> + Send + 'static) {
        let mut pending = self
            .pending
            .lock()
            .expect("autocomplete request lock is not poisoned");
        if let Some(previous) = pending.take() {
            previous.abort();
        }
        let task = tokio::spawn(async move {
            tokio::time::sleep(APPLICATION_COMMAND_AUTOCOMPLETE_DEBOUNCE).await;
            request.await;
        });
        *pending = Some(task.abort_handle());
    }
}

#[derive(Clone)]
pub(super) struct CommandDispatcher {
    client: DiscordClient,
    attachment_preview_permits: Arc<Semaphore>,
    attachment_download_permits: Arc<Semaphore>,
    autocomplete_requests: AutocompleteRequestScheduler,
}

impl CommandDispatcher {
    pub(super) fn new(client: DiscordClient) -> Self {
        Self {
            client,
            attachment_preview_permits: Arc::new(Semaphore::new(
                MAX_CONCURRENT_ATTACHMENT_PREVIEWS,
            )),
            attachment_download_permits: Arc::new(Semaphore::new(
                MAX_CONCURRENT_ATTACHMENT_DOWNLOADS,
            )),
            autocomplete_requests: AutocompleteRequestScheduler::default(),
        }
    }

    pub(super) async fn dispatch(&self, command: AppCommand) {
        if matches!(
            &command,
            AppCommand::RequestApplicationCommandAutocomplete { .. }
        ) {
            let dispatcher = self.clone();
            self.autocomplete_requests.replace(async move {
                dispatcher.handle(command).await;
            });
            return;
        }
        if runs_inline(&command) {
            self.handle(command).await;
        } else {
            let dispatcher = self.clone();
            tokio::spawn(async move {
                dispatcher.handle(command).await;
            });
        }
    }

    async fn handle(&self, command: AppCommand) {
        match command {
            command @ (AppCommand::LoadMessageHistory { .. }
            | AppCommand::RefreshMessageHistory { .. }
            | AppCommand::LoadMessageHistoryAfter { .. }
            | AppCommand::LoadMessageHistoryAround { .. }
            | AppCommand::LoadThreadPreview { .. }
            | AppCommand::LoadForumPosts { .. }
            | AppCommand::LoadInboxChannelHistory { .. }
            | AppCommand::SearchMessages { .. }) => {
                history_commands::handle(self.client.clone(), command).await;
            }
            command @ (AppCommand::LoadInboxMentions { .. }
            | AppCommand::DeleteInboxMention { .. }) => {
                inbox_commands::handle(self.client.clone(), command).await;
            }
            command @ (AppCommand::LoadGuildMembersByIds { .. }
            | AppCommand::SearchGuildMembers { .. }
            | AppCommand::SetSelectedGuild { .. }
            | AppCommand::SetSelectedMessageChannel { .. }
            | AppCommand::SubscribeDirectMessage { .. }
            | AppCommand::SubscribeGuildChannel { .. }
            | AppCommand::UpdateMemberListSubscription { .. }) => {
                gateway_commands::handle(self.client.clone(), command).await;
            }
            command @ (AppCommand::JoinVoiceChannel { .. }
            | AppCommand::UpdateVoiceState { .. }
            | AppCommand::UpdateVoiceCapturePermission { .. }
            | AppCommand::UpdateVoiceParticipantPlayback { .. }
            | AppCommand::LeaveVoiceChannel { .. }) => {
                voice_commands::handle(self.client.clone(), command).await;
            }
            command @ (AppCommand::LoadAttachmentPreview { .. }
            | AppCommand::LoadProfileAvatarPreview { .. }
            | AppCommand::OpenUrl { .. }
            | AppCommand::PlayMedia { .. }
            | AppCommand::DownloadAttachment { .. }) => {
                media_commands::handle(
                    self.client.clone(),
                    command,
                    self.attachment_preview_permits.clone(),
                    self.attachment_download_permits.clone(),
                )
                .await;
            }
            command @ (AppCommand::SendMessage { .. }
            | AppCommand::TriggerTyping { .. }
            | AppCommand::CreateForumPost { .. }
            | AppCommand::SetThreadArchived { .. }
            | AppCommand::SetThreadLocked { .. }
            | AppCommand::SetThreadPinned { .. }
            | AppCommand::DeleteThread { .. }
            | AppCommand::EditThread { .. }
            | AppCommand::SendTtsMessage { .. }
            | AppCommand::LoadApplicationCommands { .. }
            | AppCommand::RunApplicationCommand { .. }
            | AppCommand::RequestApplicationCommandAutocomplete { .. }
            | AppCommand::EditMessage { .. }
            | AppCommand::DeleteMessage { .. }
            | AppCommand::RemoveMessageEmbeds { .. }
            | AppCommand::LeaveGuild { .. }
            | AppCommand::AddReaction { .. }
            | AppCommand::RemoveReaction { .. }
            | AppCommand::LoadReactionUsers { .. }
            | AppCommand::LoadPinnedMessages { .. }
            | AppCommand::SetMessagePinned { .. }
            | AppCommand::VotePoll { .. }) => {
                message_commands::handle(self.client.clone(), command).await;
            }
            command @ (AppCommand::LoadUserProfile { .. }
            | AppCommand::LoadUserNote { .. }
            | AppCommand::UpdateUserProfile { .. }
            | AppCommand::UpdateCurrentUserStatus { .. }
            | AppCommand::UpdateGuildFolderSettings { .. }
            | AppCommand::UpdateCurrentUserActivity { .. }) => {
                user_commands::handle(self.client.clone(), command).await;
            }
            command @ (AppCommand::AckChannel { .. }
            | AppCommand::ScheduleAckChannel { .. }
            | AppCommand::AckChannels { .. }) => {
                read_state_commands::handle(self.client.clone(), command).await;
            }
            command @ (AppCommand::SetGuildMuted { .. }
            | AppCommand::SetChannelMuted { .. }
            | AppCommand::SetThreadMuted { .. }
            | AppCommand::SetThreadFollowed { .. }
            | AppCommand::SetThreadNotificationLevel { .. }) => {
                notification_commands::handle(self.client.clone(), command).await;
            }
            command @ AppCommand::SignOut => {
                session_commands::handle(self.client.clone(), command).await;
            }
        }
    }
}

fn runs_inline(command: &AppCommand) -> bool {
    matches!(
        command,
        AppCommand::SetSelectedGuild { .. }
            | AppCommand::SetSelectedMessageChannel { .. }
            | AppCommand::JoinVoiceChannel { .. }
            | AppCommand::UpdateVoiceState { .. }
            | AppCommand::UpdateVoiceCapturePermission { .. }
            | AppCommand::UpdateVoiceParticipantPlayback { .. }
            | AppCommand::LeaveVoiceChannel { .. }
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::discord::{
        MicrophoneSensitivityDb, VoiceParticipantPlaybackSettings, VoiceScope, VoiceVolumePercent,
        ids::Id,
    };

    use super::*;

    #[test]
    fn only_order_sensitive_control_commands_run_inline() {
        assert!(runs_inline(&AppCommand::SetSelectedGuild {
            guild_id: Some(Id::new(1)),
        }));
        assert!(runs_inline(&AppCommand::SetSelectedMessageChannel {
            channel_id: Some(Id::new(2)),
        }));
        assert!(runs_inline(&AppCommand::UpdateVoiceCapturePermission {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(2),
            allow_microphone_transmit: true,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::default(),
            voice_output_volume: VoiceVolumePercent::default(),
        }));
        assert!(runs_inline(&AppCommand::UpdateVoiceParticipantPlayback {
            user_id: Id::new(3),
            settings: VoiceParticipantPlaybackSettings::default(),
        }));
        assert!(runs_inline(&AppCommand::JoinVoiceChannel {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(2),
            self_mute: false,
            self_deaf: false,
            allow_microphone_transmit: true,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::default(),
            voice_output_volume: VoiceVolumePercent::default(),
            participant_playback_settings: Vec::new(),
        }));
        assert!(runs_inline(&AppCommand::UpdateVoiceState {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(2),
            self_mute: true,
            self_deaf: false,
        }));
        assert!(runs_inline(&AppCommand::LeaveVoiceChannel {
            scope: VoiceScope::Guild(Id::new(1)),
            self_mute: false,
            self_deaf: false,
        }));

        assert!(!runs_inline(&AppCommand::LoadMessageHistory {
            channel_id: Id::new(2),
            before: None,
        }));
        assert!(!runs_inline(&AppCommand::LoadAttachmentPreview {
            url: "https://cdn.discordapp.com/avatar.png".to_owned(),
        }));
    }

    #[tokio::test]
    async fn autocomplete_scheduler_runs_only_the_latest_request() {
        let scheduler = AutocompleteRequestScheduler::default();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let first_tx = result_tx.clone();
        scheduler.replace(async move {
            first_tx.send(1).expect("result receiver stays open");
        });
        scheduler.replace(async move {
            result_tx.send(2).expect("result receiver stays open");
        });

        let result = tokio::time::timeout(
            APPLICATION_COMMAND_AUTOCOMPLETE_DEBOUNCE + Duration::from_millis(100),
            result_rx.recv(),
        )
        .await
        .expect("latest autocomplete request runs after the debounce");
        assert_eq!(result, Some(2));
        let superseded = tokio::time::timeout(Duration::from_millis(25), result_rx.recv()).await;
        assert!(
            !matches!(superseded, Ok(Some(_))),
            "superseded autocomplete request must not run"
        );
    }
}
