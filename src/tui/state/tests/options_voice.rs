use std::str::FromStr;

use super::*;
use crate::discord::test_builders::{
    VoiceConnectionStatusChangedFixture, guild_create_event, voice_connection_status_changed_event,
};
use crate::discord::{
    AppCommand, StreamCaptureTarget, StreamCaptureTargetKind, VoiceParticipantPlaybackSettings,
    VoiceParticipantVolumePercent, VoiceScope, VoiceVolumePercent,
};
use crate::tui::keybindings::{KeyChord, OptionsCategoryShortcut, UiAction};
use crate::tui::state::{ChannelActionKind, popups::OptionsCategory};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn private_voice_state(kind: &str) -> DashboardState {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(1)),
    });
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        last_message_id: Some(Id::new(200)),
        name: "private call".to_owned(),
        ..ChannelInfo::test(Id::new(20), kind)
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    state
}

fn current_voice_stream_leader_label(state: &mut DashboardState) -> String {
    state.open_leader();
    state.push_key_sequence_key(KeyChord::from_str("v").expect("voice prefix should parse"));
    state
        .key_sequence_shortcuts()
        .into_iter()
        .find(|item| item.action == Some(UiAction::ToggleStream))
        .expect("voice stream shortcut is present")
        .label
}

fn complete_voice_audio_source_load(
    state: &mut DashboardState,
    inputs: &[(&str, &str)],
    outputs: &[(&str, &str)],
) {
    let commands = state.drain_pending_commands();
    let [AppCommand::LoadVoiceAudioSources { request_id }] = commands.as_slice() else {
        panic!("opening voice options should request audio sources");
    };
    state.push_effect(AppEvent::VoiceAudioSourcesLoaded {
        request_id: *request_id,
        inputs: inputs
            .iter()
            .map(|(id, label)| ((*id).to_owned(), (*label).to_owned()))
            .collect(),
        outputs: outputs
            .iter()
            .map(|(id, label)| ((*id).to_owned(), (*label).to_owned()))
            .collect(),
        error: None,
    });
}

#[test]
fn voice_options_show_push_to_talk_toggle_and_shortcut() {
    let mut state = DashboardState::new_with_voice_options(VoiceOptions {
        push_to_talk: true,
        push_to_talk_shortcut: "control+F8".to_owned(),
        allow_microphone_transmit: true,
        ..VoiceOptions::default()
    });
    state.open_options_category(OptionsCategory::Voice);
    complete_voice_audio_source_load(
        &mut state,
        &[("mic-1", "Desk microphone")],
        &[("speaker-1", "Headphones")],
    );

    let items = state.display_option_items();

    assert_eq!(items[2].label, "Input source");
    assert_eq!(items[2].value.as_deref(), Some("System default"));
    assert_eq!(items[3].label, "Output source");
    assert_eq!(items[3].value.as_deref(), Some("System default"));
    assert_eq!(items[5].label, "Push to talk");
    assert!(items[5].enabled);
    assert_eq!(items[5].value, None);
    assert_eq!(items[6].value.as_deref(), Some("control+F8"));
    assert!(items[6].effective);
    assert!(!items[8].effective);
}

#[test]
fn voice_source_options_cycle_and_queue_updates_while_disconnected() {
    let mut state = DashboardState::new();
    state.open_options_category(OptionsCategory::Voice);
    complete_voice_audio_source_load(
        &mut state,
        &[("mic-1", "Desk microphone")],
        &[("speaker-1", "Headphones")],
    );
    state.move_option_down();
    state.move_option_down();

    state.adjust_selected_display_option(1);
    assert_eq!(state.voice_options().input_source.as_deref(), Some("mic-1"));
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceAudioSources {
            input_source: Some("mic-1".to_owned()),
            output_source: None,
        }]
    );

    state.move_option_down();
    state.toggle_selected_display_option();
    assert_eq!(
        state.voice_options().output_source.as_deref(),
        Some("speaker-1")
    );
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceAudioSources {
            input_source: Some("mic-1".to_owned()),
            output_source: Some("speaker-1".to_owned()),
        }]
    );
}

#[test]
fn voice_option_toggles_queue_current_voice_state_update_when_joined() {
    let mut state = DashboardState::new();
    state.push_effect(voice_connection_status_changed_event(
        VoiceConnectionStatusChangedFixture {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Some(Id::new(11)),
            status: VoiceConnectionStatus::Connecting,
            ..VoiceConnectionStatusChangedFixture::new()
        },
    ));
    state.open_options_category_picker();
    state.open_options_category_from_shortcut(OptionsCategoryShortcut::Voice);
    complete_voice_audio_source_load(&mut state, &[], &[]);

    state.toggle_selected_display_option();
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceState {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            self_mute: true,
            self_deaf: false,
        }]
    );

    state.move_option_down();
    state.toggle_selected_display_option();
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceState {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            self_mute: true,
            self_deaf: true,
        }]
    );

    state.move_option_down();
    state.move_option_down();
    state.move_option_down();
    state.toggle_selected_display_option();
    assert!(state.voice_options().allow_microphone_transmit);
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceCapturePermission {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            allow_microphone_transmit: true,
            noise_suppression: true,
            microphone_sensitivity: Default::default(),
            microphone_volume: Default::default(),
            voice_output_volume: Default::default(),
        }]
    );

    state.move_option_down();
    state.toggle_selected_display_option();
    assert!(state.voice_options().push_to_talk);
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceCapturePermission {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            allow_microphone_transmit: true,
            noise_suppression: true,
            microphone_sensitivity: Default::default(),
            microphone_volume: Default::default(),
            voice_output_volume: Default::default(),
        }]
    );

    state.move_option_down();
    state.move_option_down();
    state.toggle_selected_display_option();
    assert!(!state.voice_options().noise_suppression);
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceCapturePermission {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            allow_microphone_transmit: true,
            noise_suppression: false,
            microphone_sensitivity: Default::default(),
            microphone_volume: Default::default(),
            voice_output_volume: Default::default(),
        }]
    );

    state.move_option_down();
    state.adjust_selected_display_option(10);
    assert_eq!(
        state.voice_options().microphone_sensitivity.label(),
        "-20 dB"
    );
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceCapturePermission {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            allow_microphone_transmit: true,
            noise_suppression: false,
            microphone_sensitivity: state.voice_options().microphone_sensitivity,
            microphone_volume: Default::default(),
            voice_output_volume: Default::default(),
        }]
    );

    state.move_option_down();
    state.adjust_selected_display_option(100);
    assert_eq!(
        state.voice_options().microphone_volume,
        VoiceVolumePercent::new(200)
    );
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceCapturePermission {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            allow_microphone_transmit: true,
            noise_suppression: false,
            microphone_sensitivity: state.voice_options().microphone_sensitivity,
            microphone_volume: VoiceVolumePercent::new(200),
            voice_output_volume: Default::default(),
        }]
    );

    state.move_option_down();
    state.adjust_selected_display_option(100);
    assert_eq!(
        state.voice_options().voice_output_volume,
        VoiceVolumePercent::new(200)
    );
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceCapturePermission {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            allow_microphone_transmit: true,
            noise_suppression: false,
            microphone_sensitivity: state.voice_options().microphone_sensitivity,
            microphone_volume: VoiceVolumePercent::new(200),
            voice_output_volume: VoiceVolumePercent::new(200),
        }]
    );
}

#[test]
fn unavailable_saved_voice_sources_are_kept_and_read_as_system_default() {
    let mut state = DashboardState::new_with_voice_options(VoiceOptions {
        input_source: Some("missing-mic".to_owned()),
        output_source: Some("missing-speaker".to_owned()),
        ..VoiceOptions::default()
    });
    state.open_options_category(OptionsCategory::Voice);
    let loading_items = state.display_option_items();
    assert_eq!(
        loading_items[2].value.as_deref(),
        Some("Loading sources...")
    );
    assert_eq!(
        loading_items[3].value.as_deref(),
        Some("Loading sources...")
    );

    complete_voice_audio_source_load(
        &mut state,
        &[("mic-1", "Desk microphone")],
        &[("speaker-1", "Headphones")],
    );

    assert_eq!(
        state.voice_options().input_source.as_deref(),
        Some("missing-mic")
    );
    assert_eq!(
        state.voice_options().output_source.as_deref(),
        Some("missing-speaker")
    );
    assert!(state.drain_pending_commands().is_empty());
    assert!(state.take_options_save_request().is_none());
    let items = state.display_option_items();
    assert_eq!(items[2].value.as_deref(), Some("System default"));
    assert_eq!(items[3].value.as_deref(), Some("System default"));
}

#[test]
fn failed_voice_source_enumeration_clears_the_previous_device_list() {
    let mut state = DashboardState::new();
    state.open_options_category(OptionsCategory::Voice);
    complete_voice_audio_source_load(
        &mut state,
        &[("mic-1", "Desk microphone")],
        &[("speaker-1", "Headphones")],
    );
    state.move_option_down();
    state.move_option_down();
    state.adjust_selected_display_option(1);
    assert_eq!(
        state.display_option_items()[2].value.as_deref(),
        Some("Desk microphone")
    );
    let _ = state.drain_pending_commands();
    let _ = state.take_options_save_request();

    state.close_options_popup();
    state.open_options_category(OptionsCategory::Voice);
    let commands = state.drain_pending_commands();
    let [AppCommand::LoadVoiceAudioSources { request_id }] = commands.as_slice() else {
        panic!("reopening voice options should request audio sources");
    };
    state.push_effect(AppEvent::VoiceAudioSourcesLoaded {
        request_id: *request_id,
        inputs: Vec::new(),
        outputs: Vec::new(),
        error: Some("voice input source enumeration failed".to_owned()),
    });

    assert_eq!(state.voice_options().input_source.as_deref(), Some("mic-1"));
    assert_eq!(
        state.display_option_items()[2].value.as_deref(),
        Some("System default")
    );
    assert!(state.take_options_save_request().is_none());
}

#[test]
fn failed_voice_source_change_restores_the_active_sources() {
    let mut state = DashboardState::new_with_voice_options(VoiceOptions {
        input_source: Some("new-mic".to_owned()),
        output_source: Some("new-speaker".to_owned()),
        ..VoiceOptions::default()
    });

    state.push_effect(AppEvent::VoiceAudioSourcesApplyFailed {
        requested_input_source: Some("new-mic".to_owned()),
        requested_output_source: Some("new-speaker".to_owned()),
        active_input_source: Some("old-mic".to_owned()),
        active_output_source: Some("old-speaker".to_owned()),
        message: "Could not switch audio sources".to_owned(),
    });

    assert_eq!(
        state.voice_options().input_source.as_deref(),
        Some("old-mic")
    );
    assert_eq!(
        state.voice_options().output_source.as_deref(),
        Some("old-speaker")
    );
    let saved = state
        .take_options_save_request()
        .expect("restored active sources should be saved");
    assert_eq!(saved.voice.input_source.as_deref(), Some("old-mic"));
    assert_eq!(saved.voice.output_source.as_deref(), Some("old-speaker"));
    assert!(state.drain_pending_commands().is_empty());
}

#[test]
fn stale_voice_source_failure_does_not_replace_a_newer_selection() {
    let mut state = DashboardState::new_with_voice_options(VoiceOptions {
        input_source: Some("newer-mic".to_owned()),
        output_source: Some("newer-speaker".to_owned()),
        ..VoiceOptions::default()
    });

    state.push_effect(AppEvent::VoiceAudioSourcesApplyFailed {
        requested_input_source: Some("failed-mic".to_owned()),
        requested_output_source: Some("failed-speaker".to_owned()),
        active_input_source: Some("old-mic".to_owned()),
        active_output_source: Some("old-speaker".to_owned()),
        message: "Could not switch audio sources".to_owned(),
    });

    assert_eq!(
        state.voice_options().input_source.as_deref(),
        Some("newer-mic")
    );
    assert_eq!(
        state.voice_options().output_source.as_deref(),
        Some("newer-speaker")
    );
    assert!(state.take_options_save_request().is_none());
}

#[test]
fn voice_channel_participant_audio_controls_persist() {
    let mut state = state_with_voice_channel_participant();
    state.focus_pane(FocusPane::Channels);
    state.set_channel_view_height(10);

    assert!(state.select_visible_pane_row(FocusPane::Channels, 2));
    assert_eq!(state.navigation.channels.list.selected, 2);
    assert_eq!(state.confirm_selected_channel_command(), None);
    assert_eq!(
        crate::tui::input::handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
        ),
        None
    );
    assert_eq!(
        state
            .voice_participant_audio_popup_view()
            .expect("participant audio popup should open")
            .settings,
        Default::default()
    );

    let volume_settings = VoiceParticipantPlaybackSettings {
        volume: VoiceParticipantVolumePercent::new(101),
        muted: false,
        video_hidden: false,
    };
    assert_eq!(
        crate::tui::input::handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        ),
        Some(AppCommand::UpdateVoiceParticipantPlayback {
            user_id: Id::new(20),
            settings: volume_settings,
        })
    );
    assert_eq!(
        crate::tui::input::handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        ),
        None
    );
    assert_eq!(
        crate::tui::input::handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        ),
        Some(AppCommand::UpdateVoiceParticipantPlayback {
            user_id: Id::new(20),
            settings: VoiceParticipantPlaybackSettings {
                muted: true,
                video_hidden: false,
                ..volume_settings
            },
        })
    );
    let saved = state
        .take_ui_state_save_request()
        .expect("participant audio changes should request a state save");
    assert_eq!(saved.voice_participant_playback.len(), 1);
    assert_eq!(saved.voice_participant_playback[0].user_id, Id::new(20));
    assert_eq!(
        saved.voice_participant_playback[0].settings,
        VoiceParticipantPlaybackSettings {
            muted: true,
            video_hidden: false,
            ..volume_settings
        }
    );
}

#[test]
fn streaming_voice_participant_action_emits_watch_command_when_joined() {
    let mut state = state_with_voice_channel_participant();
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(1)),
    });
    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            session_id: Some("my-voice-session".to_owned()),
            ..voice_state(Id::new(1), Some(Id::new(11)), Id::new(1))
        },
    });
    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            self_stream: true,
            ..voice_state(Id::new(1), Some(Id::new(11)), Id::new(20))
        },
    });
    state.push_effect(voice_connection_status_changed_event(
        VoiceConnectionStatusChangedFixture {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Some(Id::new(11)),
            status: VoiceConnectionStatus::Connected,
            ..VoiceConnectionStatusChangedFixture::new()
        },
    ));
    state.focus_pane(FocusPane::Channels);
    state.set_channel_view_height(10);

    assert!(state.select_visible_pane_row(FocusPane::Channels, 2));
    assert_eq!(state.confirm_selected_channel_command(), None);
    let actions = state.selected_channel_action_items();
    assert_eq!(actions[0].kind, ChannelActionKind::WatchStream);
    assert!(actions[0].is_enabled());

    assert_eq!(
        crate::tui::input::handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE),
        ),
        Some(AppCommand::WatchVoiceStream {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            user_id: Id::new(20),
            display_name: "Alice".to_owned(),
        })
    );
}

#[test]
fn voice_channel_action_emits_join_then_leave_command() {
    let mut state = DashboardState::new_with_voice_options(VoiceOptions {
        self_mute: true,
        self_deaf: true,
        input_source: None,
        output_source: None,
        allow_microphone_transmit: false,
        push_to_talk: false,
        push_to_talk_shortcut: "F8".to_owned(),
        noise_suppression: true,
        microphone_sensitivity: Default::default(),
        microphone_volume: Default::default(),
        voice_output_volume: Default::default(),
    });
    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![voice_channel_info(Id::new(1), Id::new(11), "Lobby")],
        ..GuildCreateFixture::new(Id::new(1))
    }));
    state.activate_guild(super::ActiveGuildScope::Guild(Id::new(1)));
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    let command = state.activate_selected_channel_action();
    assert_eq!(
        command,
        Some(AppCommand::JoinVoiceChannel {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            self_mute: true,
            self_deaf: true,
            input_source: None,
            output_source: None,
            allow_microphone_transmit: false,
            noise_suppression: true,
            microphone_sensitivity: Default::default(),
            microphone_volume: Default::default(),
            voice_output_volume: Default::default(),
            participant_playback_settings: Vec::new(),
        })
    );

    state.push_effect(voice_connection_status_changed_event(
        VoiceConnectionStatusChangedFixture {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Some(Id::new(11)),
            status: VoiceConnectionStatus::Connecting,
            ..VoiceConnectionStatusChangedFixture::new()
        },
    ));
    state.open_selected_channel_actions();
    let actions = state.selected_channel_action_items();
    assert_eq!(actions[0].kind, ChannelActionKind::JoinVoice);
    assert!(!actions[0].is_enabled());
    assert_eq!(actions[1].kind, ChannelActionKind::LeaveVoice);
    assert!(actions[1].is_enabled());

    state.select_channel_action_row(1);
    let command = state.activate_selected_channel_action();
    assert_eq!(
        command,
        Some(AppCommand::LeaveVoiceChannel {
            scope: VoiceScope::Guild(Id::new(1)),
            self_mute: true,
            self_deaf: true,
        })
    );
}

#[test]
fn joined_voice_channel_can_select_a_stream_target_and_stop_sharing() {
    let me = Id::new(10);
    let guild_id = Id::new(1);
    let channel_id = Id::new(11);
    let target = StreamCaptureTarget {
        kind: StreamCaptureTargetKind::Window,
        id: 7,
        title: "Window: Terminal".to_owned(),
    };
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(me),
    });
    state.push_event(guild_create_event(GuildCreateFixture {
        member_count: Some(1),
        owner_id: Some(Id::new(99)),
        channels: vec![voice_channel_info(guild_id, channel_id, "Lobby")],
        members: vec![member_with_username(me, "me", "me")],
        roles: vec![role_info(
            Id::new(guild_id.get()),
            "@everyone",
            PERM_VIEW_CHANNEL | PERM_CONNECT | PERM_STREAM,
        )],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            session_id: Some("voice-session".to_owned()),
            ..voice_state(guild_id, Some(channel_id), me)
        },
    });
    state.push_effect(voice_connection_status_changed_event(
        VoiceConnectionStatusChangedFixture {
            scope: VoiceScope::Guild(guild_id),
            channel_id: Some(channel_id),
            status: VoiceConnectionStatus::Connected,
            ..VoiceConnectionStatusChangedFixture::new()
        },
    ));
    state.activate_guild(super::ActiveGuildScope::Guild(guild_id));
    state.focus_pane(FocusPane::Channels);
    assert_eq!(
        current_voice_stream_leader_label(&mut state),
        "Share screen"
    );
    assert_eq!(
        state.toggle_current_voice_stream_command(),
        Some(AppCommand::LoadStreamCaptureTargets {
            request_id: crate::discord::StreamCaptureTargetsRequestId::new(0),
            scope: VoiceScope::Guild(guild_id),
            channel_id,
        })
    );
    assert_eq!(
        state
            .toast_message()
            .expect("capture target loading toast is visible")
            .text,
        "Loading screens and windows..."
    );
    state.open_selected_channel_actions();

    let actions = state.selected_channel_action_items();
    assert!(actions[2].is_enabled());
    assert_eq!(actions[2].kind, ChannelActionKind::ToggleStream);
    assert_eq!(actions[2].label, "Share screen");
    state.select_channel_action_row(2);
    assert_eq!(
        state.activate_selected_channel_action(),
        Some(AppCommand::LoadStreamCaptureTargets {
            request_id: crate::discord::StreamCaptureTargetsRequestId::new(1),
            scope: VoiceScope::Guild(guild_id),
            channel_id,
        })
    );

    state.push_effect(AppEvent::StreamCaptureTargetsLoaded {
        request_id: crate::discord::StreamCaptureTargetsRequestId::new(0),
        scope: VoiceScope::Guild(guild_id),
        channel_id,
        targets: vec![target.clone()],
        error: None,
    });
    assert!(!state.is_channel_action_stream_target_phase());
    assert_eq!(
        state
            .toast_message()
            .expect("newer capture target request remains visible")
            .text,
        "Loading screens and windows..."
    );

    state.push_effect(AppEvent::StreamCaptureTargetsLoaded {
        request_id: crate::discord::StreamCaptureTargetsRequestId::new(1),
        scope: VoiceScope::Guild(guild_id),
        channel_id,
        targets: vec![target.clone()],
        error: None,
    });
    assert!(state.is_channel_action_stream_target_phase());
    assert!(state.toast_message().is_none());
    assert_eq!(
        state.selected_stream_capture_targets(),
        std::slice::from_ref(&target)
    );
    assert_eq!(
        state.activate_selected_channel_action(),
        Some(AppCommand::StartVoiceStream {
            scope: VoiceScope::Guild(guild_id),
            channel_id,
            target,
        })
    );
    assert_eq!(
        state
            .toast_message()
            .expect("screen share preparing toast is visible")
            .text,
        "Preparing screen share..."
    );

    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            session_id: Some("voice-session".to_owned()),
            self_stream: true,
            ..voice_state(guild_id, Some(channel_id), me)
        },
    });
    assert_eq!(
        current_voice_stream_leader_label(&mut state),
        "Stop sharing"
    );
    state.open_selected_channel_actions();
    let actions = state.selected_channel_action_items();
    assert!(actions[2].is_enabled());
    assert_eq!(actions[2].kind, ChannelActionKind::ToggleStream);
    assert_eq!(actions[2].label, "Stop sharing");
    state.select_channel_action_row(2);
    assert_eq!(
        state.activate_selected_channel_action(),
        Some(AppCommand::StopVoiceStream {
            scope: VoiceScope::Guild(guild_id),
            channel_id,
        })
    );
}

#[test]
fn voice_direct_actions_toggle_state_and_leave_current_voice() {
    let mut state = DashboardState::new();
    state.push_effect(voice_connection_status_changed_event(
        VoiceConnectionStatusChangedFixture {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Some(Id::new(11)),
            status: VoiceConnectionStatus::Connecting,
            ..VoiceConnectionStatusChangedFixture::new()
        },
    ));

    state.toggle_voice_mute();
    assert!(state.voice_options().self_mute);
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceState {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            self_mute: true,
            self_deaf: false,
        }]
    );

    state.toggle_voice_deafen();
    assert!(state.voice_options().self_deaf);
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceState {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            self_mute: true,
            self_deaf: true,
        }]
    );

    let command = state.leave_current_voice_channel_command();
    assert_eq!(
        command,
        Some(AppCommand::LeaveVoiceChannel {
            scope: VoiceScope::Guild(Id::new(1)),
            self_mute: true,
            self_deaf: true,
        })
    );
}

#[test]
fn other_client_voice_state_shows_header_only() {
    let mut state = DashboardState::new_with_voice_options(VoiceOptions {
        self_mute: true,
        self_deaf: true,
        input_source: None,
        output_source: None,
        allow_microphone_transmit: false,
        push_to_talk: false,
        push_to_talk_shortcut: "F8".to_owned(),
        noise_suppression: false,
        microphone_sensitivity: Default::default(),
        microphone_volume: Default::default(),
        voice_output_volume: Default::default(),
    });
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![voice_channel_info(Id::new(1), Id::new(11), "Lobby")],
        ..GuildCreateFixture::new(Id::new(1))
    }));
    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            session_id: Some("other-client-voice-session".to_owned()),
            self_deaf: true,
            self_mute: true,
            ..voice_state(Id::new(1), Some(Id::new(11)), Id::new(10))
        },
    });

    assert_eq!(
        state.active_voice_connection_label().as_deref(),
        Some("guild - Lobby (other client)")
    );
    assert!(!state.is_joined_voice_channel(Id::new(11)));

    state.activate_guild(super::ActiveGuildScope::Guild(Id::new(1)));
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    let actions = state.selected_channel_action_items();
    assert_eq!(actions[0].kind, ChannelActionKind::JoinVoice);
}

#[test]
fn voice_join_action_reflects_scope_permissions_and_participation() {
    let me = Id::new(10);
    let owner = Id::new(11);
    let guild_id = Id::new(1);
    let voice_id = Id::new(11);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(me),
    });
    state.push_event(guild_create_event(GuildCreateFixture {
        member_count: Some(1),
        owner_id: Some(owner),
        channels: vec![voice_channel_info(guild_id, voice_id, "Lobby")],
        members: vec![member_with_username(me, "me", "me")],
        roles: vec![role_info(
            Id::new(guild_id.get()),
            "@everyone",
            PERM_VIEW_CHANNEL,
        )],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.activate_guild(super::ActiveGuildScope::Guild(guild_id));
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();

    let actions = state.selected_channel_action_items();
    assert_eq!(actions[0].kind, ChannelActionKind::JoinVoice);
    assert!(!actions[0].is_enabled());
    assert_eq!(actions[0].disabled_reason(), Some("Connect required"));
    assert_eq!(state.activate_selected_channel_action(), None);

    for kind in ["dm", "group-dm"] {
        let mut state = private_voice_state(kind);
        assert_eq!(
            state.composer_lock(),
            Some(ComposerLock::LoadingMessages),
            "{kind}"
        );
        let join = &state.selected_channel_action_items()[0];
        assert!(join.is_enabled(), "{kind}");
        assert_eq!(join.disabled_reason(), None, "{kind}");
        assert_eq!(
            state.activate_selected_channel_action(),
            Some(AppCommand::JoinVoiceChannel {
                scope: VoiceScope::Private(Id::new(20)),
                channel_id: Id::new(20),
                self_mute: false,
                self_deaf: false,
                input_source: None,
                output_source: None,
                allow_microphone_transmit: false,
                noise_suppression: true,
                microphone_sensitivity: Default::default(),
                microphone_volume: Default::default(),
                voice_output_volume: Default::default(),
                participant_playback_settings: Vec::new(),
            }),
            "{kind}"
        );
    }

    let me = Id::new(10);
    let guild_id = Id::new(1);
    let voice_id = Id::new(11);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(GuildCreateFixture {
        member_count: Some(1),
        owner_id: Some(Id::new(99)),
        channels: vec![voice_channel_info(guild_id, voice_id, "Lobby")],
        members: vec![member_with_username(me, "me", "me")],
        roles: vec![role_info(
            Id::new(guild_id.get()),
            "@everyone",
            PERM_VIEW_CHANNEL | PERM_CONNECT,
        )],
        ..GuildCreateFixture::new(guild_id)
    }));
    apply_incomplete_community_onboarding(&mut state, guild_id, me);
    state.activate_guild(super::ActiveGuildScope::Guild(guild_id));
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();

    let actions = state.selected_channel_action_items();
    let action = |kind| {
        actions
            .iter()
            .find(|action| action.kind == kind)
            .expect("channel action should exist")
    };
    assert!(!action(ChannelActionKind::JoinVoice).is_enabled());
    assert_eq!(
        action(ChannelActionKind::JoinVoice).disabled_reason(),
        Some("onboarding incomplete")
    );
    assert!(action(ChannelActionKind::ToggleMute).is_enabled());
}

mod connections {
    use super::*;
    use crate::discord::{Connection, ConnectionVisibility};

    fn connection(name: &str) -> Connection {
        Connection {
            id: "1".to_owned(),
            kind: "github".to_owned(),
            name: name.to_owned(),
            verified: true,
            show_activity: false,
            visibility: ConnectionVisibility::Hidden,
        }
    }

    fn opened() -> DashboardState {
        let mut state = DashboardState::new();
        state.open_options_category(OptionsCategory::Connections);
        state.set_connections(vec![connection("someone")]);
        state
    }

    #[test]
    fn opening_the_category_asks_for_the_list() {
        let mut state = DashboardState::new();
        state.open_options_category(OptionsCategory::Connections);

        assert!(
            state
                .drain_pending_commands()
                .iter()
                .any(|command| matches!(command, AppCommand::LoadConnections))
        );
    }

    #[test]
    fn an_empty_list_still_has_a_row_saying_why() {
        // A blank panel reads as a broken fetch rather than as no accounts.
        let mut state = DashboardState::new();
        state.open_options_category(OptionsCategory::Connections);
        state.set_connections(Vec::new());

        assert_eq!(state.display_option_items().len(), 1);
    }

    #[test]
    fn a_failed_fetch_stops_the_panel_saying_it_is_loading() {
        let mut state = DashboardState::new();
        state.open_options_category(OptionsCategory::Connections);
        state.mark_connections_load_failed();

        let label = state.display_option_items()[0].label;
        assert_eq!(label, "No linked accounts", "still claims to be loading");
    }

    #[test]
    fn toggling_visibility_keeps_the_activity_flag() {
        // Both go in the same request, so sending a default for the one not
        // being changed would quietly turn activity sharing off.
        let mut state = opened();
        state.set_connections(vec![Connection {
            show_activity: true,
            ..connection("someone")
        }]);
        state.drain_pending_commands();
        state.toggle_selected_display_option();

        let commands = state.drain_pending_commands();
        let AppCommand::ModifyConnection {
            visibility,
            show_activity,
            ..
        } = commands
            .iter()
            .find(|command| matches!(command, AppCommand::ModifyConnection { .. }))
            .expect("no modify sent")
        else {
            unreachable!()
        };

        assert_eq!(*visibility, ConnectionVisibility::Everyone);
        assert!(*show_activity, "activity sharing was turned off as well");
    }

    #[test]
    fn toggling_activity_keeps_the_visibility() {
        let mut state = opened();
        state.set_connections(vec![Connection {
            visibility: ConnectionVisibility::Everyone,
            ..connection("someone")
        }]);
        state.drain_pending_commands();
        state.toggle_selected_connection_activity();

        let commands = state.drain_pending_commands();
        let AppCommand::ModifyConnection {
            visibility,
            show_activity,
            ..
        } = commands
            .iter()
            .find(|command| matches!(command, AppCommand::ModifyConnection { .. }))
            .expect("no modify sent")
        else {
            unreachable!()
        };

        assert!(*show_activity);
        assert_eq!(
            *visibility,
            ConnectionVisibility::Everyone,
            "the connection was hidden from the profile as a side effect"
        );
    }

    #[test]
    fn the_connection_keys_do_nothing_in_another_category() {
        // They are taken before the shared router, so a category that does not
        // have connection rows must not be affected by them.
        let mut state = DashboardState::new();
        state.open_options_category(OptionsCategory::Display);
        state.drain_pending_commands();
        state.toggle_selected_connection_activity();
        state.delete_selected_connection();

        assert!(state.drain_pending_commands().is_empty());
    }

    #[test]
    fn unlinking_removes_the_row_and_sends_the_delete() {
        let mut state = opened();
        state.drain_pending_commands();
        state.delete_selected_connection();

        assert!(
            state
                .drain_pending_commands()
                .iter()
                .any(|command| matches!(command, AppCommand::DeleteConnection { .. }))
        );
        assert_eq!(state.display_option_items().len(), 1, "row still listed");
        assert_eq!(state.display_option_items()[0].label, "No linked accounts");
    }
}

mod privacy {
    use super::*;
    use crate::discord::{DmScanLevel, FriendSources, PrivacyEdit};

    fn edit_from(state: &mut DashboardState) -> PrivacyEdit {
        state
            .drain_pending_commands()
            .into_iter()
            .find_map(|command| match command {
                AppCommand::ModifyPrivacySettings { edit } => Some(edit),
                _ => None,
            })
            .expect("no privacy edit sent")
    }

    fn opened() -> DashboardState {
        let mut state = DashboardState::new();
        state.open_privacy();
        state.drain_pending_commands();
        state
    }

    #[test]
    fn a_setting_that_never_arrived_reads_as_unknown_rather_than_off() {
        // Showing a default would describe the account as more exposed than it
        // may be, which is the wrong way to be wrong about a privacy setting.
        let state = opened();
        let items = state.display_option_items();

        assert_eq!(items[0].value.as_deref(), Some("Unknown"));
        for item in &items {
            assert!(
                !item.effective,
                "{} claims to know a setting that never arrived",
                item.label
            );
        }
    }

    #[test]
    fn toggling_one_friend_source_carries_the_others() {
        // All three share one field. Sending a default for the two not being
        // changed would silently clear them.
        let mut state = opened();
        state.push_event(AppEvent::UserSettingsUpdate {
            settings: crate::discord::UserSettingsInfo {
                friend_source_flags: Some(crate::discord::UserFriendSourceFlagsInfo {
                    all: None,
                    mutual_friends: Some(true),
                    mutual_guilds: Some(true),
                }),
                ..Default::default()
            },
        });

        // Row 2 is "everyone"; the two mutual flags must survive it.
        state.move_option_down();
        state.move_option_down();
        state.toggle_selected_display_option();

        assert_eq!(
            edit_from(&mut state).friend_sources,
            Some(FriendSources {
                everyone: true,
                mutual_friends: true,
                mutual_guilds: true,
            })
        );
    }

    #[test]
    fn only_the_touched_field_is_sent() {
        // This endpoint replaces what it is given, so an edit naming a field
        // nobody touched would reset it.
        let mut state = opened();
        state.toggle_selected_display_option();

        let edit = edit_from(&mut state);
        assert_eq!(edit.dm_scan_level, Some(DmScanLevel::NonFriends));
        assert!(edit.friend_sources.is_none());
        assert!(edit.default_guilds_restricted.is_none());
    }

    #[test]
    fn the_new_server_row_is_phrased_as_the_permission_not_the_restriction() {
        // Discord stores the negative. A row showing the stored flag directly
        // would be checked exactly when direct messages are turned off.
        let mut state = opened();
        state.push_event(AppEvent::UserSettingsUpdate {
            settings: crate::discord::UserSettingsInfo {
                default_guilds_restricted: Some(true),
                ..Default::default()
            },
        });

        let item = &state.display_option_items()[1];
        assert!(item.effective, "the setting did arrive");
        assert!(!item.enabled, "restricted must read as may-not-send");

        state.move_option_down();
        state.toggle_selected_display_option();
        assert_eq!(edit_from(&mut state).default_guilds_restricted, Some(false));
    }

    #[test]
    fn a_partial_settings_update_leaves_the_other_fields_alone() {
        // Discord sends partial updates. Treating an absent field as a change
        // would reset a privacy setting nobody touched.
        let mut state = opened();
        state.push_event(AppEvent::UserSettingsUpdate {
            settings: crate::discord::UserSettingsInfo {
                explicit_content_filter: Some(2),
                ..Default::default()
            },
        });
        state.push_event(AppEvent::UserSettingsUpdate {
            settings: crate::discord::UserSettingsInfo {
                default_guilds_restricted: Some(true),
                ..Default::default()
            },
        });

        let items = state.display_option_items();
        assert_eq!(
            items[0].value.as_deref(),
            Some(DmScanLevel::Everyone.label())
        );
    }
}

mod access {
    use super::*;
    use crate::discord::{AuthSession, AuthorisedApp};

    fn session(id: &str, current: bool) -> AuthSession {
        AuthSession {
            id_hash: id.to_owned(),
            os: "Linux".to_owned(),
            platform: "Desktop".to_owned(),
            location: Some("Berlin".to_owned()),
            last_used: None,
            current,
        }
    }

    fn app(id: &str) -> AuthorisedApp {
        AuthorisedApp {
            id: id.to_owned(),
            name: format!("App {id}"),
            scopes: vec!["identify".to_owned()],
        }
    }

    fn opened() -> DashboardState {
        let mut state = DashboardState::new();
        state.open_access();
        state.set_auth_sessions(vec![session("a", true), session("b", false)]);
        state.set_authorised_apps(vec![app("1")]);
        state.drain_pending_commands();
        state
    }

    #[test]
    fn opening_asks_for_both_lists() {
        // One panel showing both, so fetching one on demand would leave half
        // of it empty until it was touched.
        let mut state = DashboardState::new();
        state.open_access();
        let commands = state.drain_pending_commands();

        assert!(
            commands
                .iter()
                .any(|c| matches!(c, AppCommand::LoadAuthSessions))
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, AppCommand::LoadAuthorisedApps))
        );
    }

    #[test]
    fn the_panel_says_it_is_loading_before_anything_arrives() {
        // Distinct from "nothing else has access", which is what an unset
        // loading flag would show for an account still fetching - and which
        // reads as a finished, empty answer.
        let mut state = DashboardState::new();
        state.open_access();

        assert_eq!(state.display_option_items()[0].label, "Loading");
    }

    #[test]
    fn an_empty_account_still_has_a_row_saying_why() {
        let mut state = DashboardState::new();
        state.open_access();
        state.set_auth_sessions(Vec::new());
        state.set_authorised_apps(Vec::new());

        assert_eq!(state.display_option_items().len(), 1);
    }

    #[test]
    fn revoking_an_app_addresses_the_app_not_the_session_at_that_row() {
        // Apps are listed after sessions, so the row index is offset by them.
        // Forgetting that would revoke the wrong thing, or nothing.
        let mut state = opened();
        for _ in 0..2 {
            state.move_option_down();
        }
        state.revoke_selected_authorised_app();

        let Some(AppCommand::RevokeAuthorisedApp { id, .. }) = state
            .drain_pending_commands()
            .into_iter()
            .find(|c| matches!(c, AppCommand::RevokeAuthorisedApp { .. }))
        else {
            panic!("no revoke sent");
        };
        assert_eq!(id, "1");
    }

    #[test]
    fn revoking_does_nothing_while_a_session_row_is_highlighted() {
        // The offset subtraction must not wrap into the app list.
        let mut state = opened();
        state.revoke_selected_authorised_app();

        assert!(state.drain_pending_commands().is_empty());
    }

    #[test]
    fn a_logout_needs_a_selection_before_it_asks_for_a_password() {
        let mut state = opened();
        state.start_session_logout();

        assert!(
            !state.is_session_password_prompt_open(),
            "asked for a password with nothing selected"
        );
    }

    #[test]
    fn the_typed_password_is_never_drawn_and_never_debug_printed() {
        let mut state = opened();
        state.toggle_selected_display_option();
        state.start_session_logout();
        for character in "hunter2".chars() {
            state.insert_session_password_char(character);
        }

        let shown = state.session_password_display().expect("no prompt open");
        assert!(!shown.contains("hunter2"), "the password was drawn");
        assert_eq!(shown.chars().count(), 7);
        assert!(!format!("{state:?}").contains("hunter2"));
    }

    #[test]
    fn confirming_sends_the_selected_hashes_and_drops_the_password() {
        let mut state = opened();
        state.toggle_selected_display_option();
        state.start_session_logout();
        for character in "hunter2".chars() {
            state.insert_session_password_char(character);
        }
        state.confirm_session_logout();

        let Some(AppCommand::RevokeAuthSessions {
            id_hashes,
            password,
        }) = state
            .drain_pending_commands()
            .into_iter()
            .find(|c| matches!(c, AppCommand::RevokeAuthSessions { .. }))
        else {
            panic!("no logout sent");
        };

        assert_eq!(id_hashes, vec!["a".to_owned()]);
        assert_eq!(password.expose(), "hunter2");
        // The prompt is gone, so there is no window in which it is still held.
        assert!(!state.is_session_password_prompt_open());
        assert!(!state.has_session_logout_targets());
    }

    #[test]
    fn an_empty_password_is_not_sent() {
        // Discord would reject it, and the round trip would read as a wrong
        // password rather than as an empty one.
        let mut state = opened();
        state.toggle_selected_display_option();
        state.start_session_logout();
        state.confirm_session_logout();

        assert!(state.drain_pending_commands().is_empty());
    }

    #[test]
    fn a_refetch_drops_selections_for_sessions_that_are_gone() {
        // A logout aimed at a session that no longer exists would fail the
        // whole request, taking the still-valid targets with it.
        let mut state = opened();
        state.toggle_selected_display_option();
        assert!(state.has_session_logout_targets());

        state.set_auth_sessions(vec![session("b", false)]);
        assert!(!state.has_session_logout_targets());
    }
}

mod account {
    use super::*;
    use crate::discord::AccountField;

    fn opened() -> DashboardState {
        let mut state = DashboardState::new();
        state.push_event(AppEvent::Ready {
            user: "someone".to_owned(),
            user_id: Some(Id::new(1)),
        });
        assert_eq!(
            state.current_user(),
            Some("someone"),
            "fixture did not sign in"
        );
        state.open_account();
        state.drain_pending_commands();
        state
    }

    fn type_into(state: &mut DashboardState, row: usize, value: &str) {
        while state.selected_option_index() != Some(row) {
            state.move_option_down();
        }
        for character in value.chars() {
            state.type_account_character(character);
        }
    }

    #[test]
    fn the_form_opens_on_the_current_username() {
        // Seeded, so an untouched field compares equal and is left out of the
        // edit rather than sent back unchanged.
        let state = opened();
        assert_eq!(
            state.display_option_items()[0].value.as_deref(),
            Some("someone")
        );
        assert_eq!(
            state.account_form_problem().as_deref(),
            Some("Nothing to change")
        );
    }

    #[test]
    fn passwords_are_drawn_as_bullets_and_never_printed() {
        let mut state = opened();
        type_into(&mut state, 4, "hunter2");

        let shown = state.display_option_items()[4].value.clone().unwrap();
        assert!(!shown.contains("hunter2"), "the password was drawn");
        assert_eq!(shown.chars().count(), 7);
        assert!(!format!("{state:?}").contains("hunter2"));
    }

    #[test]
    fn a_change_without_the_current_password_is_not_sent() {
        let mut state = opened();
        type_into(&mut state, 0, "x");
        state.submit_account_form();

        assert!(state.drain_pending_commands().is_empty());
        assert!(
            state
                .account_form_problem()
                .is_some_and(|problem| problem.contains("current password"))
        );
    }

    #[test]
    fn submitting_sends_the_change_and_leaves_no_password_behind() {
        let mut state = opened();
        type_into(&mut state, 0, "x");
        type_into(&mut state, 4, "hunter2");
        state.submit_account_form();

        let Some(AppCommand::ModifyAccount { edit, .. }) = state
            .drain_pending_commands()
            .into_iter()
            .find(|c| matches!(c, AppCommand::ModifyAccount { .. }))
        else {
            panic!("no account change sent");
        };
        assert_eq!(edit.username.as_deref(), Some("someonex"));

        // The form is consumed by submitting, so no copy of the password is
        // left in popup state.
        assert!(state.display_option_items()[4].value.as_deref() == Some(""));
        assert!(!format!("{state:?}").contains("hunter2"));
    }

    #[test]
    fn enrolment_generates_a_secret_and_cancelling_drops_it() {
        let mut state = opened();
        let totp_row = AccountField::ALL.len();
        type_into(&mut state, totp_row, "");
        state.toggle_selected_display_option();

        let uri = state.totp_enrolment_uri().expect("no enrolment started");
        assert!(uri.starts_with("otpauth://totp/Discord:someone?"));

        state.toggle_selected_display_option();
        assert!(state.totp_enrolment_uri().is_none(), "the secret survived");
    }

    #[test]
    fn a_code_typed_before_enrolment_starts_goes_nowhere() {
        // Otherwise the code field would fill for an enrolment that has no
        // secret, and submitting would send a code against nothing.
        let mut state = opened();
        type_into(&mut state, AccountField::ALL.len(), "123456");

        assert_eq!(state.totp_code(), Some(""));
    }

    #[test]
    fn enrolment_needs_the_current_password_too() {
        // Discord asks for it here as well, and it is the field the form
        // already has rather than a second prompt.
        let mut state = opened();
        let totp_row = AccountField::ALL.len();
        type_into(&mut state, totp_row, "");
        state.toggle_selected_display_option();
        type_into(&mut state, totp_row, "123456");
        state.submit_totp_enrolment();

        assert!(state.drain_pending_commands().is_empty());
    }

    #[test]
    fn enrolment_sends_the_generated_secret_not_a_new_one() {
        // Sending a freshly generated secret would enrol something the
        // authenticator app has never seen, and the code would never match.
        let mut state = opened();
        type_into(&mut state, 4, "hunter2");
        let totp_row = AccountField::ALL.len();
        type_into(&mut state, totp_row, "");
        state.toggle_selected_display_option();

        let shown = state.display_option_items()[totp_row]
            .value
            .clone()
            .unwrap()
            .replace(' ', "");
        type_into(&mut state, totp_row, "123456");
        state.submit_totp_enrolment();

        let Some(AppCommand::EnableTotp { secret, code, .. }) = state
            .drain_pending_commands()
            .into_iter()
            .find(|c| matches!(c, AppCommand::EnableTotp { .. }))
        else {
            panic!("no enrolment sent");
        };
        assert_eq!(secret, shown);
        assert_eq!(code, "123456");
    }
}
