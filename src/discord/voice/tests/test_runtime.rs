use super::*;

fn requested_voice() -> CurrentVoiceConnectionState {
    CurrentVoiceConnectionState {
        self_mute: true,
        ..CurrentVoiceConnectionState::test(Id::new(1), Id::new(10))
    }
}

fn voice_state(user_id: u64, channel_id: Option<Id<ChannelMarker>>) -> VoiceStateInfo {
    VoiceStateInfo {
        session_id: Some("voice-session".to_owned()),
        ..VoiceStateInfo::test(Id::new(1), channel_id, Id::new(user_id))
    }
}

fn voice_server() -> VoiceServerInfo {
    VoiceServerInfo {
        guild_id: Some(Id::new(1)),
        channel_id: None,
        endpoint: Some("voice.example.com".to_owned()),
        token: "secret-token".to_owned(),
    }
}

#[test]
fn voice_runtime_assembles_local_voice_session() {
    let mut state = VoiceRuntimeState::default();

    assert_eq!(
        state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10)))),
        None
    );
    assert_eq!(
        state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice()))),
        None
    );
    assert_eq!(
        state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
            10,
            Some(Id::new(10))
        ))),
        None
    );
    let action = state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));

    match action {
        Some(VoiceRuntimeAction::Connect(session)) => {
            assert_eq!(session.scope, VoiceScope::Guild(Id::new(1)));
            assert_eq!(session.channel_id, Id::new(10));
            assert_eq!(session.user_id, Id::new(10));
            assert_eq!(session.endpoint, "voice.example.com");
        }
        other => panic!("expected connect action, got {other:?}"),
    }
}

#[test]
fn voice_runtime_restores_active_sources_only_for_the_latest_failed_selection() {
    let mut state = VoiceRuntimeState::default();
    state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));
    state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice())));
    state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
        10,
        Some(Id::new(10)),
    )));
    let connection_id = match state.apply(VoiceRuntimeEvent::VoiceServer(voice_server())) {
        Some(VoiceRuntimeAction::Connect(session)) => session.connection_id,
        action => panic!("voice server should start a connection, got {action:?}"),
    };
    let requested_sources = VoiceAudioSources {
        input: Some("new-mic".to_owned()),
        output: Some("new-speaker".to_owned()),
    };
    state.apply(VoiceRuntimeEvent::AudioSourcesChanged(
        requested_sources.clone(),
    ));
    let selection = state.audio_source_selection();

    state.apply(VoiceRuntimeEvent::AudioSourcesApplyFailed {
        connection_id,
        generation: selection.generation.saturating_sub(1),
        requested_sources: requested_sources.clone(),
        active_sources: VoiceAudioSources::default(),
        message: "stale failure".to_owned(),
    });
    assert_eq!(state.audio_source_selection().sources, requested_sources);

    state.apply(VoiceRuntimeEvent::AudioSourcesApplyFailed {
        connection_id,
        generation: selection.generation,
        requested_sources: requested_sources.clone(),
        active_sources: VoiceAudioSources::default(),
        message: "current failure".to_owned(),
    });
    assert_eq!(
        state.audio_source_selection().sources,
        VoiceAudioSources::default()
    );
}

#[test]
fn voice_runtime_capture_gate_requires_allowed_active_unmuted_voice() {
    let mut state = VoiceRuntimeState::default();
    state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));

    let mut requested = requested_voice();
    requested.allow_microphone_transmit = true;
    requested.noise_suppression = true;
    requested.self_mute = false;
    requested.microphone_volume = VoiceVolumePercent::new(40);
    requested.voice_output_volume = VoiceVolumePercent::new(65);
    state.apply(VoiceRuntimeEvent::Requested(Some(requested)));
    assert_eq!(state.capture_gate(), None);

    state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
        10,
        Some(Id::new(10)),
    )));
    state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));
    assert_eq!(
        state.capture_gate(),
        Some(VoiceCaptureGate {
            capture_enabled: true,
            transmit_enabled: true,
            use_voice_activity: true,
            noise_suppression: true,
            microphone_buffer_ms: None,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::new(40),
        })
    );
    assert_eq!(
        state.playback_gate(),
        Some(VoicePlaybackGate {
            enabled: true,
            volume: VoiceVolumePercent::new(65),
        })
    );

    requested.self_mute = true;
    state.apply(VoiceRuntimeEvent::Requested(Some(requested)));
    assert_eq!(
        state.capture_gate(),
        Some(VoiceCaptureGate {
            capture_enabled: false,
            transmit_enabled: false,
            use_voice_activity: true,
            noise_suppression: true,
            microphone_buffer_ms: None,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::new(40),
        })
    );
    assert_eq!(
        state.playback_gate(),
        Some(VoicePlaybackGate {
            enabled: true,
            volume: VoiceVolumePercent::new(65),
        })
    );

    requested.self_deaf = true;
    state.apply(VoiceRuntimeEvent::Requested(Some(requested)));
    assert_eq!(
        state.capture_gate(),
        Some(VoiceCaptureGate {
            capture_enabled: false,
            transmit_enabled: false,
            use_voice_activity: true,
            noise_suppression: true,
            microphone_buffer_ms: None,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::new(40),
        })
    );
    assert_eq!(
        state.playback_gate(),
        Some(VoicePlaybackGate {
            enabled: false,
            volume: VoiceVolumePercent::new(65),
        })
    );

    requested.self_mute = false;
    requested.allow_microphone_transmit = false;
    requested.self_deaf = false;
    state.apply(VoiceRuntimeEvent::Requested(Some(requested)));
    assert_eq!(
        state.capture_gate(),
        Some(VoiceCaptureGate {
            capture_enabled: false,
            transmit_enabled: false,
            use_voice_activity: true,
            noise_suppression: true,
            microphone_buffer_ms: None,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::new(40),
        })
    );
    assert_eq!(
        state.playback_gate(),
        Some(VoicePlaybackGate {
            enabled: true,
            volume: VoiceVolumePercent::new(65),
        })
    );

    let mut other_channel = requested;
    other_channel.channel_id = Id::new(11);
    other_channel.allow_microphone_transmit = true;
    state.apply(VoiceRuntimeEvent::Requested(Some(other_channel)));
    assert_eq!(state.capture_gate(), None);
    assert_eq!(state.playback_gate(), None);
}

#[test]
#[cfg(feature = "voice-playback")]
fn voice_runtime_push_to_talk_transmits_only_while_pressed() {
    let mut state = VoiceRuntimeState::default();
    state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));
    let mut requested = requested_voice();
    requested.allow_microphone_transmit = true;
    requested.self_mute = false;
    state.apply(VoiceRuntimeEvent::Requested(Some(requested)));
    state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
        10,
        Some(Id::new(10)),
    )));
    state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));
    state.apply(VoiceRuntimeEvent::PushToTalkEnabledChanged(true));

    let released_gate = state.capture_gate().expect("capture gate exists");
    assert!(released_gate.capture_enabled);
    assert!(!released_gate.transmit_enabled);
    assert_eq!(
        state.capture_gate(),
        Some(VoiceCaptureGate {
            capture_enabled: true,
            transmit_enabled: false,
            use_voice_activity: false,
            noise_suppression: false,
            microphone_buffer_ms: None,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::default(),
        })
    );

    state.apply(VoiceRuntimeEvent::PushToTalkPressed(true));
    assert_eq!(
        state.capture_gate(),
        Some(VoiceCaptureGate {
            capture_enabled: true,
            transmit_enabled: true,
            use_voice_activity: false,
            noise_suppression: false,
            microphone_buffer_ms: None,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::default(),
        })
    );

    state.apply(VoiceRuntimeEvent::PushToTalkPressed(false));
    assert!(
        !state
            .capture_gate()
            .expect("capture gate exists")
            .transmit_enabled
    );
}

#[test]
fn voice_runtime_ignores_other_user_voice_state() {
    let mut state = VoiceRuntimeState::default();
    state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));
    state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice())));
    state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));

    assert_eq!(
        state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
            99,
            Some(Id::new(10))
        ))),
        None
    );
}

#[test]
fn voice_runtime_closes_on_leave() {
    let mut state = VoiceRuntimeState::default();
    state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));
    state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice())));
    state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
        10,
        Some(Id::new(10)),
    )));
    state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));

    assert_eq!(
        state.apply(VoiceRuntimeEvent::Requested(None)),
        Some(VoiceRuntimeAction::Close)
    );
}

#[test]
fn voice_runtime_respects_connection_end_outcome() {
    let mut state = VoiceRuntimeState::default();
    state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));
    state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice())));
    state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
        10,
        Some(Id::new(10)),
    )));
    let connected = state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));
    let Some(VoiceRuntimeAction::Connect(session)) = connected else {
        panic!("expected initial voice connect action, got {connected:?}");
    };

    let reconnected = state.apply(session.connection_ended_event(VoiceConnectionEnd::Reconnect));
    let Some(VoiceRuntimeAction::Connect(active)) = reconnected else {
        panic!("recoverable end should reconnect, got {reconnected:?}");
    };
    assert_eq!(
        state.apply(active.connection_ended_event(VoiceConnectionEnd::Stop)),
        None
    );
    assert_eq!(
        state.apply(VoiceRuntimeEvent::VoiceServer(voice_server())),
        None
    );
    assert!(matches!(
        state.apply(VoiceRuntimeEvent::ManualRetry(requested_voice())),
        Some(VoiceRuntimeAction::Connect(_))
    ));
}

#[test]
fn voice_runtime_limits_reconnects_and_resets_after_success() {
    let mut state = VoiceRuntimeState::default();
    state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));
    state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice())));
    state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
        10,
        Some(Id::new(10)),
    )));
    let connected = state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));
    let Some(VoiceRuntimeAction::Connect(mut active)) = connected else {
        panic!("expected initial voice connect action, got {connected:?}");
    };

    for _ in 0..super::runtime::MAX_VOICE_RECONNECT_ATTEMPTS {
        let reconnected = state.apply(active.connection_ended_event(VoiceConnectionEnd::Reconnect));
        let Some(VoiceRuntimeAction::Connect(next)) = reconnected else {
            panic!("retry within the limit should reconnect, got {reconnected:?}");
        };
        active = next;
    }
    assert_eq!(
        state.apply(active.connection_ended_event(VoiceConnectionEnd::Reconnect)),
        None
    );
    let manual_retry = state.apply(VoiceRuntimeEvent::ManualRetry(requested_voice()));
    assert!(
        matches!(manual_retry, Some(VoiceRuntimeAction::Connect(_))),
        "manual retry should reset the reconnect limit, got {manual_retry:?}"
    );

    let mut rotated = voice_server();
    rotated.token = "rotated-token".to_owned();
    let reset = state.apply(VoiceRuntimeEvent::VoiceServer(rotated));
    let Some(VoiceRuntimeAction::Connect(mut active)) = reset else {
        panic!("new voice session should reset the retry limit, got {reset:?}");
    };
    assert_eq!(
        state.apply(active.connection_established_event()),
        None,
        "a healthy connection should reset consecutive retries"
    );
    for _ in 0..super::runtime::MAX_VOICE_RECONNECT_ATTEMPTS {
        let reconnected = state.apply(active.connection_ended_event(VoiceConnectionEnd::Reconnect));
        let Some(VoiceRuntimeAction::Connect(next)) = reconnected else {
            panic!("retry after a healthy connection should reconnect, got {reconnected:?}");
        };
        active = next;
    }
}

#[test]
fn voice_runtime_ignores_stale_end_after_server_token_rotation() {
    let mut state = VoiceRuntimeState::default();
    state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));
    state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice())));
    state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
        10,
        Some(Id::new(10)),
    )));
    let connected = state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));
    let Some(VoiceRuntimeAction::Connect(previous)) = connected else {
        panic!("expected initial voice connect action, got {connected:?}");
    };

    let mut rotated = voice_server();
    rotated.token = "rotated-token".to_owned();
    let replaced = state.apply(VoiceRuntimeEvent::VoiceServer(rotated));
    let Some(VoiceRuntimeAction::Connect(replacement)) = replaced else {
        panic!("token rotation should replace the voice task, got {replaced:?}");
    };
    assert_ne!(previous.connection_id, replacement.connection_id);

    assert_eq!(
        state.apply(previous.connection_ended_event(VoiceConnectionEnd::Stop)),
        None
    );
    assert!(matches!(
        state.apply(replacement.connection_ended_event(VoiceConnectionEnd::Reconnect)),
        Some(VoiceRuntimeAction::Connect(_))
    ));
}

#[test]
fn voice_close_codes_follow_reconnect_policy() {
    assert_eq!(voice_close_action(4013), VoiceCloseAction::Resume);
    assert_eq!(voice_close_action(4015), VoiceCloseAction::Resume);
    assert_eq!(voice_close_action(4006), VoiceCloseAction::Reconnect);
    assert_eq!(voice_close_action(4009), VoiceCloseAction::Reconnect);
    for code in [4014, 4021, 4022] {
        assert_eq!(voice_close_action(code), VoiceCloseAction::Stop);
    }
}

#[test]
fn voice_gateway_session_debug_redacts_secrets() {
    let session = VoiceGatewaySession {
        connection_id: 0,
        scope: VoiceScope::Guild(Id::new(1)),
        channel_id: Id::new(10),
        user_id: Id::new(20),
        session_id: "secret-session".to_owned(),
        endpoint: "voice.example.com".to_owned(),
        token: "secret-token".to_owned(),
    };

    let debug = format!("{session:?}");

    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("secret-session"));
    assert!(!debug.contains("secret-token"));
}

#[test]
fn voice_state_debug_redacts_session_id() {
    let state = voice_state(10, Some(Id::new(10)));

    let debug = format!("{state:?}");

    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("voice-session"));
}
