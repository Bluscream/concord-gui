use super::*;
use crate::discord::AppCommand;
use crate::discord::test_builders::{ForumPostsLoadedFixture, forum_posts_loaded_event};
use crate::tui::state::ServerPanelTab;

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

/// Find a channel action by what it is, not where it sits.
///
/// Positional assertions here broke every time a row was inserted above them,
/// and an index says nothing about which action was meant.
fn channel_action(
    actions: &[crate::tui::state::ChannelActionItem],
    kind: ChannelActionKind,
) -> &crate::tui::state::ChannelActionItem {
    actions
        .iter()
        .find(|action| action.kind == kind)
        .unwrap_or_else(|| panic!("{kind:?} must be offered"))
}

#[test]
fn leader_message_action_copy_closes_action_popup() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.open_focused_pane_actions();

    assert!(state.is_message_action_menu_active());

    let command = state.activate_message_action_kind(MessageActionKind::CopyContent);

    assert_eq!(command, None);
    assert!(!state.is_leader_active());
    assert!(!state.is_message_action_menu_active());
    assert_eq!(
        state.take_copy_text_request(),
        Some(("msg 1".to_owned(), "Message copied"))
    );
}

#[test]
fn channel_action_menu_show_threads_opens_thread_list_view() {
    use crate::tui::state::MessagePaneSource;

    let parent_id = Id::new(2);
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();

    assert!(state.is_channel_action_menu_active());
    let actions = state.selected_channel_action_items();
    // No length assertion: this broke every time a row was added and taught
    // nothing each time. What matters is which rows are offered, below.
    assert_eq!(actions[0].kind, ChannelActionKind::JoinVoice);
    assert_eq!(actions[0].label, "Join voice");
    assert!(!actions[0].is_enabled());
    assert_eq!(actions[0].disabled_reason(), Some("not a voice channel"));
    assert_eq!(actions[1].kind, ChannelActionKind::LeaveVoice);
    let leave = channel_action(&actions, ChannelActionKind::LeaveVoice);
    assert_eq!(leave.label, "Leave voice");
    assert!(!leave.is_enabled());
    assert_eq!(leave.disabled_reason(), Some("not connected here"));
    assert!(!channel_action(&actions, ChannelActionKind::ToggleStream).is_enabled());

    let pins = channel_action(&actions, ChannelActionKind::ShowPinnedMessages);
    assert_eq!(pins.label, "Show pinned messages");
    assert!(pins.is_enabled());
    assert!(channel_action(&actions, ChannelActionKind::ShowThreads).is_enabled());
    assert_eq!(
        channel_action(&actions, ChannelActionKind::MarkAsRead).label,
        "Mark as read"
    );
    assert_eq!(
        channel_action(&actions, ChannelActionKind::ToggleMute).label,
        "Mute channel"
    );

    // "Show threads" opens the thread-list view in the message pane, not a submenu.
    let command = state.activate_channel_action_shortcut("t".parse().expect("t should parse"));
    assert_eq!(command, None);
    assert!(!state.is_channel_action_menu_active());
    assert!(state.is_channel_thread_list_view());
    assert_eq!(
        state.message_pane_source(),
        Some(MessagePaneSource::ChannelThreads {
            channel_id: parent_id
        })
    );

    // The gateway-cached child thread shows immediately, before the
    // `/threads/search` fetch for the channel completes.
    let cards = state.selected_thread_card_items();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].channel_id, Id::new(10));
    assert_eq!(cards[0].label, "release notes");
}

#[test]
fn channel_thread_list_view_fetches_and_sections_active_and_archived_threads() {
    use crate::discord::ForumPostArchiveState;
    use crate::tui::state::MessagePaneSource;

    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = state_with_messages(1);

    // The action is offered even with no threads cached: opening the view is what
    // triggers the fetch that fills the list.
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    let show_threads = state
        .selected_channel_action_items()
        .into_iter()
        .find(|action| action.kind == ChannelActionKind::ShowThreads)
        .expect("show threads action is present");
    assert!(show_threads.is_enabled());

    assert_eq!(
        state.activate_channel_action_shortcut("t".parse().expect("t parses")),
        None
    );
    assert!(state.is_channel_thread_list_view());
    assert_eq!(
        state.message_pane_source(),
        Some(MessagePaneSource::ChannelThreads { channel_id })
    );
    // The open view is now the fetch target, so the scheduler issues the
    // `/threads/search` request for this non-forum channel.
    assert_eq!(
        state
            .selected_forum_channel_with_load_more()
            .map(|(guild, channel, _)| (guild, channel)),
        Some((guild_id, channel_id))
    );

    for (archive_state, thread_id, name, archived) in [
        (ForumPostArchiveState::Active, 30, "active thread", false),
        (ForumPostArchiveState::Archived, 31, "archived thread", true),
    ] {
        state.push_event(forum_posts_loaded_event(ForumPostsLoadedFixture {
            channel_id,
            archive_state,
            next_offset: 1,
            threads: vec![forum_thread_info(
                guild_id, channel_id, thread_id, name, None, archived,
            )],
            ..ForumPostsLoadedFixture::new()
        }));
    }

    let cards = state.selected_thread_card_items();
    assert_eq!(
        cards
            .iter()
            .map(|card| (card.label.as_str(), card.section_label.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            ("active thread", Some("Active threads")),
            ("archived thread", Some("Archived threads")),
        ]
    );
}

#[test]
fn show_threads_opens_a_highlighted_but_unopened_channel() {
    use crate::tui::state::MessagePaneSource;

    let guild_id: Id<GuildMarker> = Id::new(1);
    let opened: Id<ChannelMarker> = Id::new(2);
    let highlighted: Id<ChannelMarker> = Id::new(3);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![
            ChannelInfo {
                position: Some(0),
                ..text_channel_info(guild_id, opened, "opened")
            },
            ChannelInfo {
                position: Some(1),
                ..text_channel_info(guild_id, highlighted, "highlighted")
            },
        ],
    ));
    state.activate_guild(super::ActiveGuildScope::Guild(guild_id));
    state.activate_channel(opened);

    // Highlight the second channel in the pane without opening it.
    state.focus_pane(FocusPane::Channels);
    state.move_down();
    state.open_selected_channel_actions();

    assert_eq!(
        state.activate_channel_action_shortcut("t".parse().expect("t parses")),
        None
    );

    // Show threads makes the highlighted channel active and switches the message
    // pane to its thread list, rather than silently staying on `opened`.
    assert_eq!(state.selected_channel_id(), Some(highlighted));
    assert!(state.is_channel_thread_list_view());
    assert_eq!(
        state.message_pane_source(),
        Some(MessagePaneSource::ChannelThreads {
            channel_id: highlighted
        })
    );
}

#[test]
fn mark_as_read_action_enablement_is_scoped_to_action_channel() {
    let guild_id: Id<GuildMarker> = Id::new(1);
    let unread_channel: Id<ChannelMarker> = Id::new(2);
    let read_channel: Id<ChannelMarker> = Id::new(3);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![
            ChannelInfo {
                position: Some(0),
                last_message_id: Some(Id::new(20)),
                ..text_channel_info(guild_id, unread_channel, "unread")
            },
            ChannelInfo {
                position: Some(1),
                last_message_id: Some(Id::new(30)),
                ..text_channel_info(guild_id, read_channel, "read")
            },
        ],
    ));
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            read_state_info(unread_channel, Some(Id::new(10)), 0),
            read_state_info(read_channel, Some(Id::new(30)), 0),
        ],
    });
    state.activate_guild(super::ActiveGuildScope::Guild(guild_id));
    state.activate_channel(unread_channel);
    assert_eq!(state.unread_divider_last_acked_id(), Some(Id::new(10)));

    state.focus_pane(FocusPane::Channels);
    state.move_down();
    state.open_selected_channel_actions();

    let actions = state.selected_channel_action_items();
    let mark_as_read = actions
        .iter()
        .find(|action| action.kind == ChannelActionKind::MarkAsRead)
        .expect("channel actions include Mark as read");
    assert!(!mark_as_read.is_enabled());
}

#[test]
fn channel_thread_list_card_opens_thread_and_subscribes() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    state.activate_channel_action_shortcut("t".parse().expect("t should parse"));
    assert!(state.is_channel_thread_list_view());

    let command = state.activate_selected_message_pane_item();

    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));
    assert!(!state.is_channel_thread_list_view());
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(10),
        })
    );
}

#[test]
fn channel_thread_list_view_esc_restores_previous_channel_view() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    state.activate_channel_action_shortcut("t".parse().expect("t should parse"));
    assert!(state.is_channel_thread_list_view());

    assert!(state.return_from_channel_thread_list_view());
    assert!(!state.is_channel_thread_list_view());
    assert_eq!(
        state.message_pane_source(),
        Some(crate::tui::state::MessagePaneSource::ChannelMessages {
            channel_id: Id::new(2)
        })
    );
}

#[test]
fn guild_action_menu_lists_disabled_mark_server_read_when_guild_is_read() {
    let mut state = state_with_many_guilds(1);
    state.focus_pane(FocusPane::Guilds);
    state.open_selected_guild_actions();

    assert!(state.is_guild_action_menu_active());
    let actions = state.selected_guild_action_items();
    // Looked up by kind rather than by row. Positional assertions here broke
    // three separate times as rows were added above them, which taught
    // nothing each time.
    let find = |kind: GuildActionKind| {
        actions
            .iter()
            .find(|action| action.kind == kind)
            .unwrap_or_else(|| panic!("{kind:?} should be offered"))
    };

    let mark_read = find(GuildActionKind::MarkAsRead);
    assert_eq!(mark_read.label, "Mark server as read");
    assert!(!mark_read.is_enabled());
    assert_eq!(mark_read.disabled_reason(), Some("no unread messages"));

    assert_eq!(find(GuildActionKind::ToggleMute).label, "Mute server");
    assert_eq!(find(GuildActionKind::JoinServer).label, "Join a server");
    assert_eq!(find(GuildActionKind::ViewBans).label, "View bans");
    assert_eq!(find(GuildActionKind::LeaveServer).label, "Leave server");
    assert_eq!(state.activate_selected_guild_action(), None);
}

#[test]
fn folder_leader_action_opens_settings() {
    let mut state = state_with_folder(Some(42));
    state.focus_pane(FocusPane::Guilds);
    state.open_selected_guild_actions();

    let actions = state.selected_guild_action_items();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind, GuildActionKind::FolderSettings);
    assert_eq!(actions[0].label, "Folder settings");
    assert!(actions[0].is_enabled());

    assert_eq!(state.activate_selected_guild_action(), None);
    assert!(state.is_folder_settings_open());
    assert_eq!(state.folder_settings_name_value(), Some("folder"));
    assert_eq!(state.folder_settings_color_value(), Some(""));
}

#[test]
fn channel_action_menu_toggle_mute_opens_duration_then_dispatches_command() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    state.move_down();
    state.open_selected_channel_actions();
    // Looked up rather than counted: this broke three times as rows were
    // inserted above it, and a positional index says nothing about intent.
    let mute_row = state
        .selected_channel_action_items()
        .iter()
        .position(|action| action.kind == ChannelActionKind::ToggleMute)
        .expect("mute must be offered");
    state.select_channel_action_row(mute_row);

    assert_eq!(state.activate_selected_channel_action(), None);
    assert!(state.is_channel_action_mute_duration_phase());

    let command = state.activate_selected_channel_action();

    assert_eq!(
        command,
        Some(AppCommand::SetChannelMuted {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(11),
            muted: true,
            duration: Some(crate::discord::MuteDuration::Minutes(15)),
            label: "#general".to_owned(),
        })
    );
    assert!(!state.is_channel_action_menu_active());
}

#[test]
fn category_leader_action_lists_disabled_rows_and_dispatches_mute_command() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    state.move_up();
    state.open_selected_channel_actions();

    assert!(state.is_channel_action_menu_active());
    let actions = state.selected_channel_action_items();
    // No length assertion: this broke every time a row was added and taught
    // nothing each time. What matters is which rows are offered, below.
    // Everything but muting is refused on a category, and each says why.
    for kind in [
        ChannelActionKind::JoinVoice,
        ChannelActionKind::LeaveVoice,
        ChannelActionKind::ToggleStream,
        ChannelActionKind::ShowPinnedMessages,
        ChannelActionKind::ShowThreads,
        ChannelActionKind::MarkAsRead,
        ChannelActionKind::CreateInvite,
        ChannelActionKind::ChannelPermissions,
    ] {
        assert!(
            !channel_action(&actions, kind).is_enabled(),
            "{kind:?} should be refused on a category"
        );
    }

    let mute = channel_action(&actions, ChannelActionKind::ToggleMute);
    assert_eq!(mute.label, "Mute category");
    assert!(mute.is_enabled());

    assert_eq!(state.activate_selected_channel_action(), None);
    assert!(state.is_channel_action_menu_active());
    // Looked up rather than counted: this broke three times as rows were
    // inserted above it, and a positional index says nothing about intent.
    let mute_row = state
        .selected_channel_action_items()
        .iter()
        .position(|action| action.kind == ChannelActionKind::ToggleMute)
        .expect("mute must be offered");
    state.select_channel_action_row(mute_row);
    assert_eq!(state.activate_selected_channel_action(), None);
    assert!(state.is_channel_action_mute_duration_phase());

    let command = state.activate_selected_channel_action();

    assert_eq!(
        command,
        Some(AppCommand::SetChannelMuted {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(10),
            muted: true,
            duration: Some(crate::discord::MuteDuration::Minutes(15)),
            label: "Text Channels".to_owned(),
        })
    );
    assert!(!state.is_channel_action_menu_active());
}

#[test]
fn guild_action_menu_toggle_mute_opens_duration_then_dispatches_command() {
    let mut state = state_with_many_guilds(1);
    state.focus_pane(FocusPane::Guilds);
    state.open_selected_guild_actions();
    state.select_guild_action_row(1);

    assert_eq!(state.activate_selected_guild_action(), None);
    assert!(state.is_guild_action_mute_duration_phase());

    let command = state.activate_selected_guild_action();

    assert_eq!(
        command,
        Some(AppCommand::SetGuildMuted {
            guild_id: Id::new(1),
            muted: true,
            duration: Some(crate::discord::MuteDuration::Minutes(15)),
            label: "guild 1".to_owned(),
        })
    );
    assert!(!state.is_guild_action_menu_active());
}

#[test]
fn guild_leave_confirmation_targets_the_active_guild_or_the_cursor() {
    let mut active = state_with_many_guilds(1);
    active.activate_guild(super::ActiveGuildScope::Guild(Id::new(1)));

    active.open_current_guild_leave_confirmation();

    assert!(
        active
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::GuildLeaveConfirmation)
    );
    assert_eq!(
        active.guild_leave_confirmation_name(),
        Some("guild 1".to_owned())
    );
    assert_eq!(
        {
            let held = active.confirm_guild_leave();
            past_the_risk_warning(&mut active, held)
        },
        Some(AppCommand::LeaveGuild {
            guild_id: Id::new(1),
            label: "guild 1".to_owned(),
        })
    );
    assert!(
        !active
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::GuildLeaveConfirmation)
    );

    // Nothing is open yet: the highlighted guild in the pane is enough.
    let mut cursor_only = state_with_many_guilds(1);
    cursor_only.focus_pane(FocusPane::Guilds);
    cursor_only.move_down();

    cursor_only.open_current_guild_leave_confirmation();

    assert!(
        cursor_only
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::GuildLeaveConfirmation)
    );
    assert_eq!(
        {
            let held = cursor_only.confirm_guild_leave();
            past_the_risk_warning(&mut cursor_only, held)
        },
        Some(AppCommand::LeaveGuild {
            guild_id: Id::new(1),
            label: "guild 1".to_owned(),
        })
    );
}

#[test]
fn guild_action_menu_leave_server_opens_confirmation() {
    let mut state = state_with_many_guilds(1);
    state.focus_pane(FocusPane::Guilds);
    state.move_down();
    state.open_selected_guild_actions();
    // Found by kind, so adding a row above no longer breaks this.
    let row = state
        .selected_guild_action_items()
        .iter()
        .position(|action| action.kind == GuildActionKind::LeaveServer)
        .expect("leaving should be offered");
    state.select_guild_action_row(row);

    assert_eq!(state.activate_selected_guild_action(), None);

    assert!(!state.is_guild_action_menu_active());
    assert!(
        state
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::GuildLeaveConfirmation)
    );
    assert_eq!(
        {
            let held = state.confirm_guild_leave();
            past_the_risk_warning(&mut state, held)
        },
        Some(AppCommand::LeaveGuild {
            guild_id: Id::new(1),
            label: "guild 1".to_owned(),
        })
    );
}

#[test]
fn direct_messages_do_not_open_guild_leave_confirmation() {
    let mut state = state_with_many_guilds(1);
    state.activate_guild(super::ActiveGuildScope::DirectMessages);
    state.focus_pane(FocusPane::Messages);

    state.open_current_guild_leave_confirmation();

    assert!(
        !state
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::GuildLeaveConfirmation)
    );
}

#[test]
fn guild_action_menu_marks_unread_server_channels_as_read() {
    let guild_id: Id<GuildMarker> = Id::new(1);
    let mut state = DashboardState::new();
    state.push_event(guild_create_event(
        guild_id,
        "guild",
        vec![
            ChannelInfo {
                position: Some(0),
                last_message_id: Some(Id::new(20)),
                ..text_channel_info(guild_id, Id::new(2), "unread-a")
            },
            ChannelInfo {
                position: Some(1),
                last_message_id: Some(Id::new(30)),
                ..text_channel_info(guild_id, Id::new(3), "read")
            },
            ChannelInfo {
                position: Some(2),
                last_message_id: Some(Id::new(40)),
                ..text_channel_info(guild_id, Id::new(4), "unread-b")
            },
        ],
    ));
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            read_state_info(Id::new(2), Some(Id::new(10)), 0),
            read_state_info(Id::new(3), Some(Id::new(30)), 0),
            read_state_info(Id::new(4), Some(Id::new(35)), 0),
        ],
    });
    state.focus_pane(FocusPane::Guilds);
    state.open_selected_guild_actions();

    let actions = state.selected_guild_action_items();
    assert_eq!(actions[0].kind, GuildActionKind::MarkAsRead);
    assert!(actions[0].is_enabled());

    let command = state.activate_selected_guild_action();
    let ack_commands = command.clone().into_iter().collect::<Vec<_>>();
    apply_optimistic_ack_commands(&mut state, &ack_commands);

    assert_eq!(
        state.sidebar_guild_unread(guild_id),
        ChannelUnreadState::Seen
    );
    assert!(!state.is_guild_action_menu_active());
    let Some(AppCommand::AckChannels { mut targets }) = command else {
        panic!("expected bulk channel ack command");
    };
    targets.sort_by_key(|(channel_id, _)| channel_id.get());
    assert_eq!(
        targets,
        vec![(Id::new(2), Id::new(20)), (Id::new(4), Id::new(40))]
    );
}

#[test]
fn guild_action_menu_skips_hidden_channels_when_marking_server_read() {
    let mut state = state_with_hidden_and_visible_channels();
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            read_state_info(Id::new(2), Some(Id::new(10)), 0),
            read_state_info(Id::new(3), Some(Id::new(10)), 0),
        ],
    });
    state.push_event(notification_message_event(Id::new(2), "hidden"));
    state.push_event(notification_message_event(Id::new(3), "visible"));
    state.focus_pane(FocusPane::Guilds);
    state.move_down();
    state.open_selected_guild_actions();
    let command = state.activate_selected_guild_action();
    let ack_commands = command.clone().into_iter().collect::<Vec<_>>();
    apply_optimistic_ack_commands(&mut state, &ack_commands);

    let Some(AppCommand::AckChannels { targets }) = command else {
        panic!("expected bulk channel ack command");
    };
    assert_eq!(targets, vec![(Id::new(3), Id::new(50))]);
    assert_ne!(state.channel_unread(Id::new(2)), ChannelUnreadState::Seen);
    assert_eq!(state.channel_unread(Id::new(3)), ChannelUnreadState::Seen);
}

#[test]
fn direct_messages_offer_joining_a_server() {
    let mut state = DashboardState::new();
    state.focus_pane(FocusPane::Guilds);
    state.move_up();
    state.open_selected_guild_actions();

    // The DM list used to show a disabled placeholder. Joining does not depend
    // on which server is selected, so it is offered here rather than requiring
    // the user to select a server they may not have yet.
    let actions = state.selected_guild_action_items();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind, GuildActionKind::JoinServer);
    assert_eq!(actions[0].label, "Join a server");
    assert!(actions[0].is_enabled());
}

#[test]
fn joining_a_server_previews_the_invite_before_accepting() {
    use crate::discord::InvitePreview;

    let mut state = state_with_many_guilds(1);
    state.open_join_server();

    // Nothing is joined on the first submit: an invite code says nothing about
    // where it leads, so it is resolved and shown first.
    state.insert_join_server_str("https://discord.gg/aBc-123");
    let command = state.submit_join_server();
    assert!(
        matches!(command, Some(AppCommand::ResolveInvite { ref code }) if code == "aBc-123"),
        "first submit must resolve, not join"
    );

    state.apply_resolved_invite(InvitePreview {
        code: "aBc-123".to_owned(),
        guild_name: "Rust Community".to_owned(),
        already_joined: false,
        ..InvitePreview::default()
    });
    assert!(
        state
            .join_server_state()
            .is_some_and(|join| join.is_joinable())
    );

    // Only now does submitting join - through the warning, since joining is
    // the action most likely to get a third-party client flagged.
    let held = state.submit_join_server();
    let command = past_the_risk_warning(&mut state, held);
    assert!(
        matches!(command, Some(AppCommand::AcceptInvite { ref code }) if code == "aBc-123"),
        "second submit must accept the resolved invite"
    );
}

#[test]
fn an_invite_to_a_server_already_joined_cannot_be_accepted() {
    use crate::discord::InvitePreview;

    let mut state = state_with_many_guilds(1);
    state.open_join_server();
    state.apply_resolved_invite(InvitePreview {
        code: "aBc-123".to_owned(),
        guild_name: "Rust Community".to_owned(),
        already_joined: true,
        ..InvitePreview::default()
    });

    assert!(
        !state
            .join_server_state()
            .is_some_and(|join| join.is_joinable())
    );
    // Submitting must do nothing rather than send a join for a guild the
    // account is already in.
    assert_eq!(state.submit_join_server(), None);
}

#[test]
fn text_that_is_not_an_invite_is_refused_without_a_request() {
    let mut state = state_with_many_guilds(1);
    state.open_join_server();
    state.insert_join_server_str("hello there");

    assert_eq!(state.submit_join_server(), None);
    assert!(
        state
            .join_server_state()
            .and_then(|join| join.error())
            .is_some(),
        "the prompt must say why rather than silently doing nothing"
    );
}

#[test]
fn the_server_panel_only_fetches_a_tab_it_has_not_seen() {
    // Refetching on every tab switch would spend requests for no new
    // information and make the panel flicker; reload is what refetching is
    // for.
    let mut state = state_with_many_guilds(1);
    let guild_id = Id::new(1);

    assert_eq!(
        state.open_server_management(guild_id, ServerPanelTab::Invites),
        Some(AppCommand::LoadGuildInvites { guild_id })
    );
    state.apply_guild_invites(
        guild_id,
        vec![crate::discord::GuildInviteInfo {
            code: "aBc-123".to_owned(),
            channel_id: None,
            channel_name: None,
            inviter: None,
            uses: 0,
            max_uses: None,
            max_age_seconds: None,
            temporary: false,
        }],
    );

    // Roles come next and fetch nothing: they arrive with the guild.
    assert_eq!(state.next_server_tab(), None);

    // Then emoji, which have never been loaded and so do fetch.
    assert_eq!(
        state.next_server_tab(),
        Some(AppCommand::LoadGuildEmojis { guild_id })
    );
    state.apply_guild_emojis(guild_id, Vec::new());

    // Then the guild's own sounds.
    assert_eq!(
        state.next_server_tab(),
        Some(AppCommand::LoadSoundboardSounds {
            guild_id: Some(guild_id)
        })
    );
    state.apply_panel_sounds(Some(guild_id), Vec::new());

    // Then the AutoMod rules.
    assert_eq!(
        state.next_server_tab(),
        Some(AppCommand::LoadAutoModRules { guild_id })
    );
    state.apply_automod_rules(guild_id, Vec::new());

    assert_eq!(
        state.next_server_tab(),
        Some(AppCommand::LoadGuildAuditLog { guild_id })
    );
    state.apply_guild_audit_log(guild_id, Vec::new());

    // Membership needs three fetches: the first is returned, the rest queued.
    assert_eq!(
        state.next_server_tab(),
        Some(AppCommand::LoadWelcomeScreen { guild_id })
    );
    assert!(
        state
            .drain_pending_commands()
            .iter()
            .any(|command| matches!(command, AppCommand::LoadGuildWidget { .. })),
        "the membership tab queued nothing beyond its first fetch"
    );
    state.set_welcome_screen(crate::discord::WelcomeScreen::default());
    state.set_guild_widget(crate::discord::GuildWidget::default());

    assert_eq!(
        state.next_server_tab(),
        Some(AppCommand::LoadScheduledEvents { guild_id })
    );
    state.set_scheduled_events(Vec::new());
    assert_eq!(
        state.next_server_tab(),
        Some(AppCommand::LoadGuildTemplates { guild_id })
    );
    state.set_guild_templates(Vec::new());

    // Wrapping round lands on settings, which reads the snapshot and so
    // fetches nothing either.
    assert_eq!(state.next_server_tab(), None);
    assert_eq!(
        state.server_management_state().map(|p| p.tab()),
        Some(ServerPanelTab::Settings)
    );

    // And invites, already held, still do not refetch.
    assert_eq!(state.next_server_tab(), None);
    assert_eq!(
        state.server_management_state().map(|p| p.tab()),
        Some(ServerPanelTab::Invites)
    );

    // Reload always asks again - that is the point of it.
    assert_eq!(
        state.reload_server_management(),
        Some(AppCommand::LoadGuildInvites { guild_id })
    );
}

#[test]
fn the_audit_log_offers_no_row_action() {
    // History is a record. Offering "enter to delete" over it would suggest
    // the client can edit what happened, which it cannot and should not.
    let mut state = state_with_many_guilds(1);
    let guild_id = Id::new(1);
    state.open_server_management(guild_id, ServerPanelTab::AuditLog);
    state.apply_guild_audit_log(
        guild_id,
        vec![crate::discord::AuditLogEntryInfo {
            id: Id::new(7001),
            actor: Some("ferris".to_owned()),
            action: crate::discord::AuditLogAction::MemberBanAdd,
            target: Some("spammer".to_owned()),
            reason: None,
        }],
    );

    assert_eq!(state.activate_selected_server_row(), None);
    // And the entry is still there afterwards.
    assert_eq!(
        state.server_management_state().map(|p| p.audit_log().len()),
        Some(1)
    );
}

#[test]
fn revoking_an_invite_takes_the_row_out_straight_away() {
    // The list is a snapshot. Leaving a revoked code on screen invites a
    // second revoke for one that no longer exists.
    let mut state = state_with_many_guilds(1);
    let guild_id = Id::new(1);
    state.open_server_management(guild_id, ServerPanelTab::Invites);
    state.apply_guild_invites(
        guild_id,
        vec![crate::discord::GuildInviteInfo {
            code: "aBc-123".to_owned(),
            channel_id: None,
            channel_name: None,
            inviter: None,
            uses: 0,
            max_uses: None,
            max_age_seconds: None,
            temporary: false,
        }],
    );

    assert_eq!(
        state.activate_selected_server_row(),
        Some(AppCommand::RevokeInvite {
            code: "aBc-123".to_owned()
        })
    );
    assert_eq!(
        state.server_management_state().map(|p| p.invites().len()),
        Some(0)
    );
    // A second activation has nothing left to revoke.
    assert_eq!(state.activate_selected_server_row(), None);
}

#[test]
fn a_reply_for_another_guild_does_not_fill_this_panel() {
    // The panel can be closed and reopened elsewhere while a fetch is out.
    let mut state = state_with_many_guilds(2);
    state.open_server_management(Id::new(1), ServerPanelTab::Invites);

    state.apply_guild_invites(
        Id::new(2),
        vec![crate::discord::GuildInviteInfo {
            code: "wrong".to_owned(),
            channel_id: None,
            channel_name: None,
            inviter: None,
            uses: 0,
            max_uses: None,
            max_age_seconds: None,
            temporary: false,
        }],
    );

    let panel = state.server_management_state().expect("panel is open");
    assert!(panel.invites().is_empty());
    assert!(panel.is_loading(), "the real fetch is still outstanding");
}

#[test]
fn renaming_an_emoji_seeds_the_field_and_applies_locally() {
    let mut state = state_with_many_guilds(1);
    let guild_id = Id::new(1);
    state.open_server_management(guild_id, ServerPanelTab::Emoji);
    state.apply_guild_emojis(
        guild_id,
        vec![crate::discord::GuildEmojiInfo {
            id: Id::new(8001),
            name: "ferris".to_owned(),
            animated: false,
            role_restricted: false,
        }],
    );

    state.start_emoji_rename();
    // Seeded rather than blank: a rename is usually a correction, and
    // retyping the whole name to fix one letter is busywork.
    assert_eq!(
        state
            .server_management_state()
            .and_then(|p| p.renaming())
            .map(|(_, input)| input.value()),
        Some("ferris")
    );

    state.insert_emoji_rename_char('2');
    assert_eq!(
        state.submit_emoji_rename(),
        Some(AppCommand::RenameEmoji {
            guild_id,
            emoji_id: Id::new(8001),
            name: "ferris2".to_owned(),
        })
    );

    // Applied locally too: leaving the old name showing makes a successful
    // rename look like it failed.
    assert_eq!(
        state
            .server_management_state()
            .map(|p| p.emojis()[0].name.clone()),
        Some("ferris2".to_owned())
    );
}

#[test]
fn an_unchanged_or_empty_emoji_name_sends_nothing() {
    let mut state = state_with_many_guilds(1);
    let guild_id = Id::new(1);
    state.open_server_management(guild_id, ServerPanelTab::Emoji);
    state.apply_guild_emojis(
        guild_id,
        vec![crate::discord::GuildEmojiInfo {
            id: Id::new(8001),
            name: "ferris".to_owned(),
            animated: false,
            role_restricted: false,
        }],
    );

    // Confirming without changing anything is not a rename.
    state.start_emoji_rename();
    assert_eq!(state.submit_emoji_rename(), None);

    // Nor is clearing it: Discord would reject an empty name, so this reads
    // as a cancel rather than costing a request to be told so.
    state.start_emoji_rename();
    state.edit_emoji_rename(crate::tui::text_input::TextEditAction::DeleteToLineStart);
    assert_eq!(state.submit_emoji_rename(), None);
}

#[test]
fn renaming_is_refused_on_the_tabs_that_have_no_emoji() {
    // 'n' is a plain letter; on the invite or audit tab it must do nothing
    // rather than seed a field from whatever row happens to be highlighted.
    let mut state = state_with_many_guilds(1);
    state.open_server_management(Id::new(1), ServerPanelTab::Invites);

    state.start_emoji_rename();
    assert!(
        state
            .server_management_state()
            .and_then(|p| p.renaming())
            .is_none()
    );
}

#[test]
fn tabs_keep_a_draft_per_channel() {
    // The point of tabs: returning to one does not lose what was typed there.
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    // Row 0 is the category, which is not a channel and opens no tab.
    state.move_down();
    state.open_selected_channel_in_new_tab();

    state.start_composer();
    for value in "first".chars() {
        state.push_composer_char(value);
    }

    // A second tab starts empty rather than inheriting the first one's draft.
    state.move_down();
    state.open_selected_channel_in_new_tab();
    assert_eq!(state.composer_input(), "");

    for value in "second".chars() {
        state.push_composer_char(value);
    }

    state.activate_channel_tab(0);
    assert_eq!(state.composer_input(), "first");
    state.activate_channel_tab(1);
    assert_eq!(state.composer_input(), "second");
}

#[test]
fn opening_a_channel_already_in_a_tab_switches_rather_than_duplicating() {
    // Two tabs onto one channel would give it two drafts, and no way to tell
    // which one a message would be sent from.
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    state.move_down();

    state.open_selected_channel_in_new_tab();
    state.move_down();
    state.open_selected_channel_in_new_tab();
    state.move_up();
    state.open_selected_channel_in_new_tab();

    assert_eq!(state.channel_tabs().len(), 2);
    assert_eq!(state.active_channel_tab(), 0);
}

#[test]
fn cycling_tabs_wraps_at_both_ends() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    state.move_down();
    state.open_selected_channel_in_new_tab();
    state.move_down();
    state.open_selected_channel_in_new_tab();

    assert_eq!(state.active_channel_tab(), 1);
    state.cycle_channel_tab(true);
    assert_eq!(state.active_channel_tab(), 0);
    state.cycle_channel_tab(false);
    assert_eq!(state.active_channel_tab(), 1);
}

#[test]
fn closing_a_tab_falls_back_to_the_one_on_its_left() {
    // Where attention was before the closed tab existed.
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    state.move_down();
    state.open_selected_channel_in_new_tab();
    state.move_down();
    state.open_selected_channel_in_new_tab();

    state.close_active_channel_tab();
    assert_eq!(state.channel_tabs().len(), 1);
    assert_eq!(state.active_channel_tab(), 0);

    // Closing the last one leaves none, without panicking on the empty list.
    state.close_active_channel_tab();
    assert!(state.channel_tabs().is_empty());
    state.cycle_channel_tab(true);
    state.close_active_channel_tab();
}

#[test]
fn adding_an_emoji_names_it_from_the_filename() {
    let mut state = state_with_many_guilds(1);
    let guild_id = Id::new(1);
    state.open_server_management(guild_id, ServerPanelTab::Emoji);
    state.apply_guild_emojis(guild_id, Vec::new());

    state.start_emoji_upload();
    for value in "/tmp/party parrot.gif".chars() {
        state.insert_emoji_rename_char(value);
    }

    let command = state.submit_emoji_rename();
    // The name comes from the filename, with the space replaced: a space is
    // an ordinary thing in a filename and not a legal emoji name.
    assert!(matches!(
        command,
        Some(AppCommand::CreateEmoji { ref name, .. }) if name == "party_parrot"
    ));
}

#[test]
fn a_filename_with_no_usable_name_is_refused_before_sending() {
    // Otherwise it costs a request and an upload to be told Discord will not
    // accept the name.
    let mut state = state_with_many_guilds(1);
    let guild_id = Id::new(1);
    state.open_server_management(guild_id, ServerPanelTab::Emoji);
    state.apply_guild_emojis(guild_id, Vec::new());

    state.start_emoji_upload();
    for value in "/tmp/!.png".chars() {
        state.insert_emoji_rename_char(value);
    }

    assert_eq!(state.submit_emoji_rename(), None);
}

#[test]
fn uploading_is_refused_on_the_tabs_that_have_no_emoji() {
    let mut state = state_with_many_guilds(1);
    state.open_server_management(Id::new(1), ServerPanelTab::AuditLog);

    state.start_emoji_upload();
    assert!(
        state
            .server_management_state()
            .and_then(|p| p.renaming())
            .is_none()
    );
}

#[test]
fn a_channel_settings_form_only_offers_the_fields_that_kind_has() {
    // A topic on a category, or a user limit on a text channel, would be a
    // control that does nothing.
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    state.move_down();

    let channel_id = state
        .channel_pane_entries()
        .get(state.selected_channel())
        .and_then(|entry| match entry {
            crate::tui::state::ChannelPaneEntry::Channel { state, .. } => Some(state.id),
            _ => None,
        })
        .expect("the cursor should be on a channel");

    state.open_channel_settings(channel_id);
    let fields: Vec<_> = state
        .channel_edit_state()
        .expect("settings should be open")
        .fields()
        .to_vec();

    assert!(fields.contains(&crate::tui::state::ChannelField::Name));
    assert!(fields.contains(&crate::tui::state::ChannelField::Topic));
    assert!(fields.contains(&crate::tui::state::ChannelField::Slowmode));
    // Text channels have no occupancy cap.
    assert!(!fields.contains(&crate::tui::state::ChannelField::UserLimit));
}

#[test]
fn saving_an_unchanged_channel_sends_nothing() {
    // It would spend a request and write an audit log entry saying that
    // nothing happened.
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    state.move_down();

    let channel_id = state
        .channel_pane_entries()
        .get(state.selected_channel())
        .and_then(|entry| match entry {
            crate::tui::state::ChannelPaneEntry::Channel { state, .. } => Some(state.id),
            _ => None,
        })
        .expect("the cursor should be on a channel");

    state.open_channel_settings(channel_id);
    assert_eq!(state.submit_channel_edit(), None);
}

#[test]
fn the_roles_tab_reads_the_snapshot_rather_than_fetching() {
    // Roles arrive with the guild, so asking for them would spend a request
    // that fetches nothing.
    let mut state = state_with_many_guilds(1);
    let guild_id = Id::new(1);

    assert_eq!(
        state.open_server_management(guild_id, ServerPanelTab::Roles),
        None
    );
    assert_eq!(state.reload_server_management(), None);
}

#[test]
fn everyone_cannot_be_deleted() {
    // Discord refuses, because @everyone is the guild itself. Saying so beats
    // a round trip that fails.
    // state_with_channel_tree is the fixture that has an @everyone role;
    // state_with_many_guilds has none at all.
    let mut state = state_with_channel_tree();
    let guild_id = Id::new(1);
    state.open_server_management(guild_id, ServerPanelTab::Roles);

    let has_everyone = state.server_management_state().is_some_and(|panel| {
        panel
            .roles()
            .iter()
            .any(|role| role.id.get() == guild_id.get())
    });
    assert!(has_everyone, "the fixture should have @everyone");

    assert_eq!(state.activate_selected_server_row(), None);
    // Still there: refused, not removed.
    assert!(
        state
            .server_management_state()
            .is_some_and(|panel| !panel.roles().is_empty())
    );
}

#[test]
fn only_the_guilds_own_sounds_are_manageable() {
    // The default sounds arrive on the same event and belong to the picker,
    // where they can be played but not renamed or deleted by anyone.
    let mut state = state_with_many_guilds(1);
    let guild_id = Id::new(1);
    state.open_server_management(guild_id, ServerPanelTab::Sounds);

    state.apply_panel_sounds(
        None,
        vec![crate::discord::SoundboardSound {
            sound_id: 1,
            name: "default".to_owned(),
            volume: 1.0,
            emoji_id: None,
            emoji_name: None,
            guild_id: None,
            available: true,
        }],
    );

    assert!(
        state
            .server_management_state()
            .is_some_and(|panel| panel.sounds().is_empty()),
        "the defaults must not appear in the guild's own list"
    );
}

#[test]
fn renaming_a_sound_to_something_discord_rejects_sends_nothing() {
    // Two characters minimum, so a one-character name costs a request to be
    // told what could be checked here.
    let mut state = state_with_many_guilds(1);
    let guild_id = Id::new(1);
    state.open_server_management(guild_id, ServerPanelTab::Sounds);
    state.apply_panel_sounds(
        Some(guild_id),
        vec![crate::discord::SoundboardSound {
            sound_id: 7,
            name: "airhorn".to_owned(),
            volume: 1.0,
            emoji_id: None,
            emoji_name: None,
            guild_id: Some(guild_id),
            available: true,
        }],
    );

    state.start_emoji_rename();
    state.edit_emoji_rename(crate::tui::text_input::TextEditAction::DeleteToLineStart);
    state.insert_emoji_rename_char('a');
    assert_eq!(state.submit_emoji_rename(), None);
}

#[test]
fn a_guild_name_discord_would_reject_sends_nothing() {
    // Two characters minimum. A one-character name costs a request to be told
    // what could be checked here.
    let mut state = state_with_channel_tree();
    state.open_server_management(Id::new(1), ServerPanelTab::Settings);

    // Row 0 is the name; activating it opens the field.
    assert_eq!(state.activate_selected_server_row(), None);
    state.edit_emoji_rename(crate::tui::text_input::TextEditAction::DeleteToLineStart);
    state.insert_emoji_rename_char('a');

    assert_eq!(state.submit_emoji_rename(), None);
}

#[test]
fn guild_direct_message_action_reads_unknown_until_the_list_arrives() {
    // The restricted-guild list comes with READY. A guild absent from a list
    // nobody received is not a guild Discord confirmed is unrestricted, and
    // labelling it "block" would assert a setting never seen.
    let mut state = state_with_many_guilds(1);
    state.focus_pane(FocusPane::Guilds);
    state.open_selected_guild_actions();

    let label = state
        .selected_guild_action_items()
        .into_iter()
        .find(|action| action.kind == GuildActionKind::ToggleGuildDirectMessages)
        .expect("the action should be offered")
        .label;

    assert!(label.contains("unknown"), "got {label:?}");
}

#[test]
fn blocking_one_guilds_direct_messages_keeps_the_other_restrictions() {
    // The endpoint replaces the whole list, so an edit carrying only this
    // guild would unrestrict every other one.
    let mut state = state_with_many_guilds(2);
    state.focus_pane(FocusPane::Guilds);
    let other = Id::new(9_999);
    state.push_event(AppEvent::UserSettingsUpdate {
        settings: crate::discord::UserSettingsInfo {
            restricted_guilds: Some(vec![other]),
            ..Default::default()
        },
    });
    state.open_selected_guild_actions();

    let selected = state
        .selected_guild_cursor_id()
        .expect("a guild should be selected");
    let index = state
        .selected_guild_action_items()
        .iter()
        .position(|action| action.kind == GuildActionKind::ToggleGuildDirectMessages)
        .expect("the action should be offered");
    assert!(state.select_guild_action_row(index));

    let Some(AppCommand::ModifyPrivacySettings { edit }) = state.activate_selected_guild_action()
    else {
        panic!("no privacy edit sent");
    };
    let guilds = edit.restricted_guilds.expect("no list sent");

    assert!(guilds.contains(&selected), "the guild was not restricted");
    assert!(guilds.contains(&other), "the other restriction was dropped");
}

mod membership {
    use super::*;
    use crate::tui::state::popups::ServerPanelTab;

    fn opened() -> DashboardState {
        let mut state = state_with_many_guilds(1);
        state.focus_pane(FocusPane::Guilds);
        let guild_id = state.selected_guild_cursor_id().expect("a guild");
        state.open_server_management(guild_id, ServerPanelTab::Membership);
        state.drain_pending_commands();
        state
    }

    fn select(state: &mut DashboardState, label: &str) {
        let index = state
            .membership_rows()
            .iter()
            .position(|(row, _)| row == label)
            .unwrap_or_else(|| panic!("{label} should be a row"));
        while state.selected_server_row() != Some(index) {
            state.move_server_selection_down();
        }
    }

    #[test]
    fn opening_asks_for_all_three_things_the_tab_shows() {
        // Fetching one on demand would leave two thirds of the tab empty
        // until it was touched.
        let mut state = state_with_many_guilds(1);
        state.focus_pane(FocusPane::Guilds);
        let guild_id = state.selected_guild_cursor_id().expect("a guild");
        // The first fetch is returned for the caller to send; the rest are
        // queued. Both halves count as having asked.
        let mut commands: Vec<AppCommand> = Vec::new();
        commands.extend(state.open_server_management(guild_id, ServerPanelTab::Membership));
        commands.extend(state.drain_pending_commands());

        assert!(
            commands
                .iter()
                .any(|c| matches!(c, AppCommand::LoadWelcomeScreen { .. }))
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, AppCommand::LoadGuildWidget { .. }))
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, AppCommand::LoadPruneCount { .. }))
        );
    }

    #[test]
    fn a_value_that_never_arrived_reads_as_unknown_rather_than_off() {
        // A welcome screen that has not loaded is not one Discord confirmed is
        // off, and showing "off" would describe the server wrongly.
        let state = opened();
        let rows = state.membership_rows();

        assert_eq!(rows[0].1, "unknown");
        assert_eq!(rows[2].1, "unknown");
    }

    #[test]
    fn cycling_the_prune_window_clears_the_count_it_no_longer_describes() {
        // The old count was for the old window. Leaving it would state a
        // number for a prune nobody is about to run.
        let mut state = opened();
        state.set_prune_count(12);
        select(&mut state, "Prune inactive after");
        state.activate_selected_server_row();

        assert_eq!(
            state
                .membership_rows()
                .iter()
                .find(|(label, _)| label == "Prune")
                .map(|(_, value)| value.clone()),
            Some("counting".to_owned())
        );
    }

    #[test]
    fn a_prune_that_would_remove_nobody_is_not_offered() {
        // Discord exempts every member who has any role at all, so zero is the
        // commonest answer - and a warning about removing nobody teaches the
        // wrong lesson about the warning.
        let mut state = opened();
        state.set_prune_count(0);
        select(&mut state, "Prune");

        assert!(state.pending_prune().is_none());
    }

    #[test]
    fn a_prune_with_members_to_remove_is_offered_for_the_chosen_window() {
        let mut state = opened();
        state.set_prune_count(4);
        select(&mut state, "Prune");

        let Some(AppCommand::PruneGuild { days, .. }) = state.pending_prune() else {
            panic!("no prune offered");
        };
        assert!(crate::discord::PRUNE_DAYS.contains(&days));
    }

    #[test]
    fn the_prune_row_is_the_only_one_that_offers_a_prune() {
        // The row list is positional, so a row inserted above it would
        // otherwise silently move the destructive action under another label.
        let mut state = opened();
        state.set_prune_count(4);
        for (label, _) in state.membership_rows() {
            select(&mut state, &label);
            assert_eq!(
                state.pending_prune().is_some(),
                label == "Prune",
                "{label} offered the wrong thing"
            );
        }
    }
}

#[test]
fn every_server_tab_that_needs_a_fetch_asks_for_one() {
    // The cycle test above walks the tabs in order and breaks whenever one is
    // added, which teaches nothing each time. This states the actual rule, so
    // a tab added without a fetch fails here rather than by rendering empty
    // forever - which reads as a server that has none of whatever it shows.
    use crate::tui::state::popups::ServerPanelTab;

    let mut state = state_with_many_guilds(1);
    state.focus_pane(FocusPane::Guilds);
    let guild_id = state.selected_guild_cursor_id().expect("a guild");

    for tab in ServerPanelTab::ALL {
        let mut asked: Vec<AppCommand> = Vec::new();
        asked.extend(state.open_server_management(guild_id, tab));
        asked.extend(state.drain_pending_commands());

        // Settings and roles arrive with the guild and are read from the
        // snapshot, so they are the only two that ask for nothing.
        let snapshot_tab = matches!(tab, ServerPanelTab::Settings | ServerPanelTab::Roles);
        assert_eq!(
            asked.is_empty(),
            snapshot_tab,
            "{tab:?} asked for {} commands",
            asked.len()
        );
    }
}

#[test]
fn an_ambiguous_channel_name_does_not_aim_the_widget_at_a_guess() {
    // Discord allows two channels with the same name. Resolving to whichever
    // came first would point the widget's invite at a channel nobody chose,
    // and the result looks exactly like success.
    use crate::tui::state::popups::ServerPanelTab;

    let mut state = state_with_many_guilds(1);
    state.focus_pane(FocusPane::Guilds);
    let guild_id = state.selected_guild_cursor_id().expect("a guild");
    state.open_server_management(guild_id, ServerPanelTab::Membership);
    state.set_guild_widget(crate::discord::GuildWidget::default());
    state.drain_pending_commands();

    let names: Vec<String> = state
        .discord_cache_channel_names(guild_id)
        .into_iter()
        .collect();
    let Some(unique) = names.first().cloned() else {
        // A fixture with no channels proves nothing either way.
        return;
    };

    state.start_membership_edit_for_widget_channel();
    state.set_membership_edit_text(&unique);
    let resolved = state.submit_emoji_rename();
    assert!(
        matches!(resolved, Some(AppCommand::ModifyGuildWidget { .. })),
        "a name that exactly one channel has should resolve"
    );

    state.start_membership_edit_for_widget_channel();
    state.set_membership_edit_text("definitely-not-a-channel");
    assert!(
        state.submit_emoji_rename().is_none(),
        "an unknown name silently cleared the invite"
    );
}

mod event_line {
    use crate::discord::parse_new_event;
    use crate::discord::{NewEventLocation, NewEventProblem};

    #[test]
    fn a_complete_line_becomes_a_creatable_event() {
        let event =
            parse_new_event("Games night | 2026-09-01T19:00:00Z | 2026-09-01T22:00:00Z | The pub")
                .expect("should parse");

        assert_eq!(event.name, "Games night");
        assert_eq!(event.starts_at, "2026-09-01T19:00:00Z");
        assert_eq!(event.problem(), None);
    }

    #[test]
    fn a_place_containing_a_separator_survives() {
        // Bar names contain pipes about as often as anything else does, and
        // truncating at the fourth would silently drop half the address.
        let event = parse_new_event(
            "Quiz | 2026-09-01T19:00:00Z | 2026-09-01T22:00:00Z | The Cat | and Fiddle",
        )
        .expect("should parse");

        let NewEventLocation::External(place) = event.location else {
            panic!("should be external");
        };
        assert_eq!(place, "The Cat | and Fiddle");
    }

    #[test]
    fn a_half_typed_line_says_which_field_is_missing() {
        // Discord's own message does not, which is the reason for checking
        // here at all.
        let event = parse_new_event("Games night").expect("should parse");
        assert_eq!(event.problem(), Some(NewEventProblem::StartMissing));

        let event = parse_new_event("Games night | 2026-09-01T19:00:00Z").expect("should parse");
        assert_eq!(
            event.problem(),
            Some(NewEventProblem::ExternalNeedsLocation)
        );

        let event = parse_new_event("Games night | 2026-09-01T19:00:00Z |  | The pub")
            .expect("should parse");
        assert_eq!(event.problem(), Some(NewEventProblem::ExternalNeedsEnd));
    }

    #[test]
    fn an_empty_line_is_refused_rather_than_creating_a_nameless_event() {
        let event = parse_new_event("").expect("should parse");
        assert_eq!(event.problem(), Some(NewEventProblem::NameMissing));
    }
}
