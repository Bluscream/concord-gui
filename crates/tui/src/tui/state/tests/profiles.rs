use super::*;
use crate::tui::state::MemberActionKind;
use crate::tui::state::UserProfileSettingsField;
use crate::tui::state::popups::{MemberActionMenuState, ModalPopup};
use crate::tui::text_input::TextEditAction;
use concord::discord::{
    ActivityInfo, AppCommand, AppEvent, FriendStatus, GlobalUserProfileUpdate,
    GuildUserProfileUpdate, MessageAttachmentUpload, ProfileAvatarUpload, RelationshipInfo,
    UserProfileUpdate,
};
use concord_fixtures::events::{
    UserProfileLoadFailedFixture, guild_create_event, user_profile_load_failed_event,
};

/// Carry out something that now raises a risk warning first.
///
/// The warning is part of the path rather than an extra step these tests can
/// skip: asserting through it is what proves it is actually there.
fn past_the_risk_warning(
    state: &mut DashboardState,
    immediate: Option<AppCommand>,
) -> Option<AppCommand> {
    assert!(
        immediate.is_none(),
        "the action should be held by a warning, not sent straight away"
    );
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::RiskWarning));
    state.confirm_risk_warning()
}

#[test]
fn opening_profile_uses_cache_for_same_guild() {
    let user_id: Id<UserMarker> = Id::new(10);
    let guild_id: Id<GuildMarker> = Id::new(1);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::UserProfileLoaded {
        guild_id: Some(guild_id),
        profile: profile_info(user_id.get(), Some("guild nick")),
    });

    assert_eq!(
        state.open_user_profile_popup(user_id, Some(guild_id)),
        Some(AppCommand::LoadUserProfile {
            user_id,
            guild_id: Some(guild_id),
        })
    );
    assert_eq!(
        state
            .user_profile_popup_data()
            .and_then(|profile| profile.guild_nick.as_deref()),
        Some("guild nick")
    );
}

#[test]
fn opening_profile_refetches_when_cached_for_different_guild() {
    let user_id: Id<UserMarker> = Id::new(10);
    let cached_guild: Id<GuildMarker> = Id::new(1);
    let popup_guild: Id<GuildMarker> = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::UserProfileLoaded {
        guild_id: Some(cached_guild),
        profile: profile_info(user_id.get(), Some("cached nick")),
    });

    assert_eq!(
        state.open_user_profile_popup(user_id, Some(popup_guild)),
        Some(AppCommand::LoadUserProfile {
            user_id,
            guild_id: Some(popup_guild),
        })
    );
    assert!(state.user_profile_popup_data().is_none());
}

#[test]
fn user_profile_load_failure_marks_open_popup_failed() {
    let user_id: Id<UserMarker> = Id::new(10);
    let guild_id: Id<GuildMarker> = Id::new(1);
    let mut state = DashboardState::new();

    state.open_user_profile_popup(user_id, Some(guild_id));
    state.push_event(user_profile_load_failed_event(
        UserProfileLoadFailedFixture {
            user_id,
            guild_id: Some(guild_id),
            message: "network failed".to_owned(),
        },
    ));

    assert_eq!(
        state.user_profile_popup_load_error(),
        Some("network failed")
    );
}

#[test]
fn user_profile_load_failure_ignores_stale_popup() {
    let user_id: Id<UserMarker> = Id::new(10);
    let open_guild: Id<GuildMarker> = Id::new(1);
    let stale_guild: Id<GuildMarker> = Id::new(2);
    let mut state = DashboardState::new();

    state.open_user_profile_popup(user_id, Some(open_guild));
    state.push_event(user_profile_load_failed_event(
        UserProfileLoadFailedFixture {
            user_id,
            guild_id: Some(stale_guild),
            message: "stale failure".to_owned(),
        },
    ));

    assert_eq!(state.user_profile_popup_load_error(), None);
}

#[test]
fn user_profile_popup_status_resolves_from_the_best_available_source() {
    let user_id: Id<UserMarker> = Id::new(10);
    let guild_id: Id<GuildMarker> = Id::new(1);

    let cached_presence = |state: &mut DashboardState| {
        state.push_event(AppEvent::PresenceUpdate {
            guild_id: None,
            presence: concord::discord::PresenceEventFields {
                user_id,
                status: PresenceStatus::Idle,
                activities: Vec::new(),
            },
        });
    };
    let dm_recipient = |state: &mut DashboardState, status: PresenceStatus| {
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            recipients: Some(vec![ChannelRecipientInfo {
                status: Some(status),
                ..ChannelRecipientInfo::test(user_id, "neo")
            }]),
            ..dm_channel_info(Id::new(20), "neo")
        }));
    };

    // Guild member presence, DM recipient status, and the standalone presence
    // cache each answer on their own. When a recipient only reports `Unknown`,
    // the cached presence has to win instead of showing an offline dot.
    let mut guild_member = DashboardState::new();
    guild_member.push_event(guild_create_event(GuildCreateFixture {
        members: vec![member_info(user_id, "neo")],
        presences: vec![(user_id, PresenceStatus::DoNotDisturb)],
        ..GuildCreateFixture::new(guild_id)
    }));
    guild_member.open_user_profile_popup(user_id, Some(guild_id));
    assert_eq!(
        guild_member.user_profile_popup_status(),
        PresenceStatus::DoNotDisturb
    );

    let mut recipient_only = DashboardState::new();
    dm_recipient(&mut recipient_only, PresenceStatus::Idle);
    recipient_only.open_user_profile_popup(user_id, None);
    assert_eq!(
        recipient_only.user_profile_popup_status(),
        PresenceStatus::Idle
    );

    let mut presence_only = DashboardState::new();
    cached_presence(&mut presence_only);
    presence_only.open_user_profile_popup(user_id, None);
    assert_eq!(
        presence_only.user_profile_popup_status(),
        PresenceStatus::Idle
    );

    let mut unknown_recipient = DashboardState::new();
    cached_presence(&mut unknown_recipient);
    dm_recipient(&mut unknown_recipient, PresenceStatus::Unknown);
    unknown_recipient.open_user_profile_popup(user_id, None);
    assert_eq!(
        unknown_recipient.user_profile_popup_status(),
        PresenceStatus::Idle
    );
}

#[test]
fn opening_current_user_profile_uses_active_user() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });

    assert_eq!(
        state.open_current_user_profile_popup(),
        Some(AppCommand::LoadUserProfile {
            user_id: Id::new(10),
            guild_id: None,
        })
    );
}

#[test]
fn profile_settings_save_dispatches_dirty_global_fields() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_current_user_profile_popup();

    let _ = state.start_or_commit_user_profile_edit();
    for value in "Neo".chars() {
        state.push_user_profile_edit_char(value);
    }
    let _ = state.start_or_commit_user_profile_edit();

    assert_eq!(
        {
            let held = state.save_user_profile_settings_command();
            past_the_risk_warning(&mut state, held)
        },
        Some(AppCommand::UpdateUserProfile {
            update: Box::new(UserProfileUpdate {
                user_id: Id::new(10),
                guild_id: None,
                global: GlobalUserProfileUpdate {
                    display_name: Some("Neo".to_owned()),
                    pronouns: None,
                    avatar: None,
                },
                guild: None,
            }),
        })
    );
}

#[test]
fn profile_settings_sign_out_dispatches_command_and_status() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_current_user_profile_popup();

    let _ = state.start_or_commit_user_profile_edit();
    state.push_user_profile_edit_char('x');

    assert_eq!(state.sign_out_command(), Some(AppCommand::SignOut));
    assert!(!state.is_user_profile_popup_editing());
    assert_eq!(state.user_profile_settings_status(), Some("Signing out..."));
}

#[test]
fn profile_settings_sign_out_ignores_other_profiles() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_user_profile_popup(Id::new(20), None);

    assert_eq!(state.sign_out_command(), None);
}

#[test]
fn profile_settings_text_editing_uses_cursor() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_current_user_profile_popup();

    let _ = state.start_or_commit_user_profile_edit();
    state.insert_user_profile_edit_text("hello world");
    state.edit_user_profile_text_input(TextEditAction::MoveCursorWordLeft);
    state.insert_user_profile_edit_text("brave ");

    assert_eq!(
        state.user_profile_settings_field_value(UserProfileSettingsField::GlobalDisplayName),
        "hello brave world"
    );

    state.edit_user_profile_text_input(TextEditAction::DeletePreviousWord);
    assert_eq!(
        state.user_profile_settings_field_value(UserProfileSettingsField::GlobalDisplayName),
        "hello world"
    );

    state.edit_user_profile_text_input(TextEditAction::MoveCursorHome);
    state.insert_user_profile_edit_text("Neo ");
    state.edit_user_profile_text_input(TextEditAction::MoveCursorEnd);
    state.edit_user_profile_text_input(TextEditAction::DeletePreviousChar);

    assert_eq!(
        state.user_profile_settings_field_value(UserProfileSettingsField::GlobalDisplayName),
        "Neo hello worl"
    );
}

#[test]
fn profile_settings_text_cursor_handles_graphemes() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_current_user_profile_popup();

    let _ = state.start_or_commit_user_profile_edit();
    state.insert_user_profile_edit_text("가🇰🇷나");
    state.edit_user_profile_text_input(TextEditAction::MoveCursorLeft);
    state.edit_user_profile_text_input(TextEditAction::DeletePreviousChar);

    assert_eq!(
        state.user_profile_settings_field_value(UserProfileSettingsField::GlobalDisplayName),
        "가나"
    );
}

#[test]
fn profile_settings_dirty_count_ignores_unchanged_text_and_empty_avatar_path() {
    let user_id = Id::new(10);
    let mut state = DashboardState::new();
    let mut profile = profile_info(user_id.get(), None);
    profile.global_name = Some("Neo".to_owned());
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    state.push_event(AppEvent::UserProfileLoaded {
        guild_id: None,
        profile,
    });
    state.open_current_user_profile_popup();

    let _ = state.start_or_commit_user_profile_edit();
    let _ = state.start_or_commit_user_profile_edit();
    state.next_user_profile_settings_field();
    state.next_user_profile_settings_field();
    let _ = state.start_or_commit_user_profile_edit();
    state.insert_user_profile_edit_text("   ");
    let _ = state.start_or_commit_user_profile_edit();

    assert_eq!(state.user_profile_settings_dirty_count(), 0);
    assert_eq!(state.save_user_profile_settings_command(), None);
}

#[test]
fn profile_settings_save_dispatches_pasted_avatar_upload() {
    let user_id = Id::new(10);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    state.open_current_user_profile_popup();
    state.next_user_profile_settings_field();
    state.next_user_profile_settings_field();

    assert!(
        state.set_user_profile_avatar_from_attachment(MessageAttachmentUpload::from_bytes(
            "avatar.png".to_owned(),
            vec![1, 2, 3]
        ),)
    );

    assert_eq!(
        {
            let held = state.save_user_profile_settings_command();
            past_the_risk_warning(&mut state, held)
        },
        Some(AppCommand::UpdateUserProfile {
            update: Box::new(UserProfileUpdate {
                user_id,
                guild_id: None,
                global: GlobalUserProfileUpdate {
                    display_name: None,
                    pronouns: None,
                    avatar: Some(ProfileAvatarUpload::from_bytes(
                        "avatar.png".to_owned(),
                        vec![1, 2, 3],
                    )),
                },
                guild: None,
            }),
        })
    );
}

#[test]
fn profile_settings_status_picker_dispatches_presence_update() {
    let user_id = Id::new(10);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    state.push_event(AppEvent::PresenceUpdate {
        guild_id: None,
        presence: concord::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Online,
            activities: vec![ActivityInfo::playing("Concord")],
        },
    });
    state.open_current_user_profile_popup();
    state.next_user_profile_settings_field();
    state.next_user_profile_settings_field();
    state.next_user_profile_settings_field();

    assert_eq!(
        state.user_profile_settings_field_value(UserProfileSettingsField::CurrentStatus),
        "Online"
    );

    let _ = state.start_or_commit_user_profile_edit();
    assert!(state.is_user_profile_status_picker_open());
    state.move_user_profile_status_picker_down();
    state.move_user_profile_status_picker_down();

    assert_eq!(
        state.activate_user_profile_status_picker(),
        Some(AppCommand::UpdateCurrentUserStatus {
            status: PresenceStatus::DoNotDisturb,
        })
    );
    assert!(!state.is_user_profile_status_picker_open());
    assert_eq!(
        state.user_profile_settings_field_value(UserProfileSettingsField::CurrentStatus),
        "Do Not Disturb"
    );
    assert_eq!(state.save_user_profile_settings_command(), None);
}

#[test]
fn profile_settings_activity_manual_entry_dispatches_presence_update() {
    let user_id = Id::new(10);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    state.push_event(AppEvent::PresenceUpdate {
        guild_id: None,
        presence: concord::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Online,
            activities: Vec::new(),
        },
    });
    state.open_current_user_profile_popup();
    for _ in 0..4 {
        state.next_user_profile_settings_field();
    }

    assert_eq!(state.start_or_commit_user_profile_edit(), None);
    assert!(state.is_user_profile_activity_picker_open());
    assert_eq!(state.activate_user_profile_activity_picker(), None);
    assert!(!state.is_user_profile_activity_picker_open());
    assert_eq!(
        state.user_profile_settings_editing_field(),
        Some(UserProfileSettingsField::ManualActivity),
        "manual entry should switch to the text editor"
    );
    for value in "Concord".chars() {
        state.push_user_profile_edit_char(value);
    }

    assert_eq!(
        state.start_or_commit_user_profile_edit(),
        Some(AppCommand::UpdateCurrentUserActivity {
            status: PresenceStatus::Online,
            activities: vec![ActivityInfo::playing("Concord")],
            track_client_id: None,
        })
    );
}

#[test]
fn profile_settings_activity_picker_selects_detected_app() {
    let user_id = Id::new(10);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    state.push_event(AppEvent::PresenceUpdate {
        guild_id: None,
        presence: concord::discord::PresenceEventFields {
            user_id,
            status: PresenceStatus::Online,
            activities: Vec::new(),
        },
    });
    let detected = ActivityInfo {
        application_id: Some("client-123".to_owned()),
        ..ActivityInfo::playing("Visual Studio Code")
    };
    state.set_detected_rich_presence(vec![detected.clone()]);
    state.open_current_user_profile_popup();
    for _ in 0..4 {
        state.next_user_profile_settings_field();
    }

    assert_eq!(state.start_or_commit_user_profile_edit(), None);
    let rows = state.user_profile_activity_picker_rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "Visual Studio Code");
    assert!(rows[0].1, "detected app is selected first");

    assert_eq!(
        state.activate_user_profile_activity_picker(),
        Some(AppCommand::UpdateCurrentUserActivity {
            status: PresenceStatus::Online,
            activities: vec![detected],
            track_client_id: Some("client-123".to_owned()),
        })
    );
    assert!(!state.is_user_profile_activity_picker_open());
}

#[test]
fn profile_settings_ignore_non_current_user_profile() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.open_user_profile_popup(Id::new(20), None);

    let _ = state.start_or_commit_user_profile_edit();
    state.push_user_profile_edit_char('x');

    assert!(!state.is_user_profile_popup_editing());
    assert_eq!(state.save_user_profile_settings_command(), None);
}

#[test]
fn profile_settings_save_dispatches_guild_fields() {
    let user_id = Id::new(10);
    let guild_id = Id::new(1);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    state.open_user_profile_popup(user_id, Some(guild_id));
    state.switch_user_profile_settings_to_guild();

    let _ = state.start_or_commit_user_profile_edit();
    for value in "server neo".chars() {
        state.push_user_profile_edit_char(value);
    }
    let _ = state.start_or_commit_user_profile_edit();

    assert_eq!(
        {
            let held = state.save_user_profile_settings_command();
            past_the_risk_warning(&mut state, held)
        },
        Some(AppCommand::UpdateUserProfile {
            update: Box::new(UserProfileUpdate {
                user_id,
                guild_id: Some(guild_id),
                global: GlobalUserProfileUpdate::default(),
                guild: Some(GuildUserProfileUpdate {
                    guild_id,
                    nickname: Some("server neo".to_owned()),
                    pronouns: None,
                    bio: None,
                    avatar: None,
                }),
            }),
        })
    );
}

#[test]
fn profile_settings_noop_edit_does_not_dispatch_update() {
    let user_id = Id::new(10);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    let mut profile = profile_info(user_id.get(), None);
    profile.global_name = Some("Neo".to_owned());
    state.push_event(AppEvent::UserProfileLoaded {
        guild_id: None,
        profile,
    });
    state.open_user_profile_popup(user_id, None);

    let _ = state.start_or_commit_user_profile_edit();
    let _ = state.start_or_commit_user_profile_edit();

    assert_eq!(state.save_user_profile_settings_command(), None);
    assert_eq!(
        state.user_profile_settings_status(),
        Some("No profile changes to save")
    );
}

#[test]
fn profile_reload_failure_after_save_clears_saving_state() {
    let user_id = Id::new(10);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(user_id),
    });
    state.open_current_user_profile_popup();
    let _ = state.start_or_commit_user_profile_edit();
    state.push_user_profile_edit_char('x');
    let _ = state.start_or_commit_user_profile_edit();
    let held = state.save_user_profile_settings_command();
    assert!(past_the_risk_warning(&mut state, held).is_some());
    assert!(state.user_profile_settings_saving());

    state.push_event(user_profile_load_failed_event(
        UserProfileLoadFailedFixture {
            user_id,
            message: "reload failed".to_owned(),
            ..UserProfileLoadFailedFixture::new()
        },
    ));

    assert!(!state.user_profile_settings_saving());
    assert_eq!(
        state.user_profile_settings_status(),
        Some("Save succeeded, but profile reload failed: reload failed"),
    );
}

/// A dashboard where the current user stands this way with user 42.
fn state_with_relationship(status: FriendStatus) -> DashboardState {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::RelationshipsLoaded {
        relationships: vec![RelationshipInfo {
            user_id: Id::new(42),
            status,
            nickname: None,
            display_name: Some("someone".to_owned()),
            username: Some("someone".to_owned()),
        }],
    });
    state
        .popups
        .set_modal(ModalPopup::MemberActionMenu(MemberActionMenuState {
            user_id: Id::new(42),
            guild_id: None,
            selection: Default::default(),
        }));
    state
}

#[test]
fn friend_actions_reflect_how_things_already_stand() {
    // "Add friend" and "accept request" are the same call, so offering both
    // would be a choice with no difference; only one is ever shown, named for
    // what it actually does from here.
    let cases = [
        (FriendStatus::None, MemberActionKind::AddFriend),
        (FriendStatus::IncomingRequest, MemberActionKind::AddFriend),
        (
            FriendStatus::OutgoingRequest,
            MemberActionKind::RemoveFriend,
        ),
        (FriendStatus::Friend, MemberActionKind::RemoveFriend),
    ];

    for (status, expected) in cases {
        let state = state_with_relationship(status);
        let kinds: Vec<_> = state
            .selected_member_action_items()
            .into_iter()
            .map(|item| item.kind)
            .collect();

        assert!(
            kinds.contains(&expected),
            "{status:?} should offer {expected:?}"
        );
        // Blocking is offered alongside, and unblocking only replaces it.
        assert!(kinds.contains(&MemberActionKind::Block));
        assert!(!kinds.contains(&MemberActionKind::Unblock));
    }
}

#[test]
fn a_blocked_user_is_offered_only_unblocking() {
    // Not "remove friend" as well: they are not one, and the endpoint behind
    // both is the same, so two entries would do the same thing under
    // different names.
    let state = state_with_relationship(FriendStatus::Blocked);
    let kinds: Vec<_> = state
        .selected_member_action_items()
        .into_iter()
        .map(|item| item.kind)
        .collect();

    assert!(kinds.contains(&MemberActionKind::Unblock));
    assert!(!kinds.contains(&MemberActionKind::Block));
    assert!(!kinds.contains(&MemberActionKind::AddFriend));
    assert!(!kinds.contains(&MemberActionKind::RemoveFriend));
}
