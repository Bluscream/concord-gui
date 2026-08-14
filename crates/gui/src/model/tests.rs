//! Projection tests against the core's fixture state.
//!
//! These run headlessly, so the whole core -> projection -> view-model path is
//! verified in CI without a Discord account, a token, or a display.

use concord::discord::fixtures::demo_state;

use crate::model::message::project_messages;
use crate::model::projection::{Navigation, Selection, project, typing_names};
use crate::theme::Presence;
use crate::ui::workspace::ChannelKind;

fn guild_nav(guild: u64, channel: Option<u64>) -> Navigation {
    Navigation {
        selection: Selection::Guild(concord::discord::Id::new(guild)),
        channel: channel.map(concord::discord::Id::new),
    }
}

#[test]
fn projects_guilds_with_dm_pseudo_guild_first() {
    let state = demo_state();
    let model = project(&state, &Navigation::default(), true);

    assert_eq!(model.guilds[0].name, "Direct Messages");
    assert!(model.guilds[0].id.is_none());

    let names: Vec<_> = model.guilds.iter().map(|g| g.name.as_str()).collect();
    assert!(names.contains(&"RostFaden"), "got {names:?}");
    assert!(names.contains(&"Rust Community"), "got {names:?}");
}

#[test]
fn projects_channels_for_the_selected_guild_only() {
    let state = demo_state();
    let model = project(&state, &guild_nav(10, None), true);

    let names: Vec<_> = model.channels.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"general"), "got {names:?}");
    assert!(names.contains(&"gui-rewrite"), "got {names:?}");
    // Belongs to guild 20 and must not leak in.
    assert!(!names.contains(&"showcase"), "got {names:?}");
}

#[test]
fn categories_are_uppercased_and_kinds_resolved() {
    let state = demo_state();
    let model = project(&state, &guild_nav(10, None), true);

    let category = model
        .channels
        .iter()
        .find(|c| c.kind == ChannelKind::Category)
        .expect("fixture defines categories");
    assert_eq!(category.name, category.name.to_uppercase());

    assert!(
        model
            .channels
            .iter()
            .any(|c| c.kind == ChannelKind::Voice && c.name == "Standup"),
        "voice channels should be classified as Voice"
    );
}

#[test]
fn unread_and_mentions_reach_the_view_model() {
    let state = demo_state();
    let model = project(&state, &guild_nav(10, None), true);

    let mentioned = model
        .channels
        .iter()
        .find(|c| c.name == "gui-rewrite")
        .expect("fixture defines #gui-rewrite");
    assert!(
        mentioned.unread,
        "channel with mentions must read as unread"
    );
    assert!(mentioned.mentions > 0, "mention badge count must survive");

    // ci-logs has unread messages but no mentions: unread, no badge.
    let noisy = model
        .channels
        .iter()
        .find(|c| c.name == "ci-logs")
        .expect("fixture defines #ci-logs");
    assert!(noisy.unread);
    assert_eq!(noisy.mentions, 0, "unread without mentions shows no number");
}

#[test]
fn direct_messages_project_as_channels() {
    let state = demo_state();
    let model = project(&state, &Navigation::default(), true);

    let names: Vec<_> = model.channels.iter().map(|c| c.name.as_str()).collect();
    assert!(names.iter().any(|n| n.contains("ferris")), "got {names:?}");
    assert!(
        names.iter().any(|n| n.contains(',')),
        "group DM should join recipient names, got {names:?}"
    );
}

#[test]
fn member_list_preserves_groups_and_order() {
    let state = demo_state();
    let model = project(&state, &guild_nav(10, Some(111)), true);

    assert!(!model.members.is_empty(), "member list should populate");
    assert!(
        model.members[0].is_group,
        "server order starts with a group header"
    );

    let blu = model
        .members
        .iter()
        .find(|m| m.name == "blu")
        .expect("fixture defines blu");
    assert!(!blu.is_group);
    assert!(blu.color.is_some(), "role colour should resolve");

    assert!(
        model.members.iter().any(|m| m.is_bot),
        "bot flag should survive projection"
    );
    assert!(
        model.members.iter().any(|m| m.presence == Presence::Idle),
        "varied presence should survive projection"
    );
}

#[test]
fn messages_project_in_order_with_grouping() {
    let state = demo_state();
    let rows = project_messages(
        &state,
        concord::discord::Id::new(111),
        state.current_user_id(),
    );

    assert!(rows.len() >= 8, "fixture defines a full conversation");

    // Oldest first.
    assert!(
        rows.windows(2).all(|w| w[0].timestamp <= w[1].timestamp),
        "messages must be ordered oldest-first"
    );

    // First row of a block is never a continuation.
    assert!(!rows[0].continues);

    // ferris posts three in a row; the later two group.
    assert!(
        rows.iter().any(|r| r.continues),
        "consecutive same-author messages should group"
    );
}

#[test]
fn replies_break_grouping_and_carry_context() {
    let state = demo_state();
    let rows = project_messages(
        &state,
        concord::discord::Id::new(111),
        state.current_user_id(),
    );

    let reply = rows
        .iter()
        .find(|r| r.reply_to.is_some())
        .expect("fixture defines a reply");

    assert!(
        !reply.continues,
        "a reply must start its own block even from the same author"
    );
    let (author, content) = reply.reply_to.as_ref().unwrap();
    assert_eq!(author, "turing");
    assert!(!content.is_empty());
}

#[test]
fn reactions_and_edits_project() {
    let state = demo_state();
    let rows = project_messages(
        &state,
        concord::discord::Id::new(111),
        state.current_user_id(),
    );

    let reacted = rows
        .iter()
        .find(|r| !r.reactions.is_empty())
        .expect("fixture defines reactions");

    let (glyph, count, mine) = &reacted.reactions[0];
    assert!(!glyph.is_empty(), "unicode emoji should resolve to a glyph");
    assert!(*count > 0);
    assert!(*mine, "the me-reacted flag should survive");

    // Custom emoji render as :name: until image loading lands.
    assert!(
        reacted.reactions.iter().any(|(g, _, _)| g.starts_with(':')),
        "custom emoji should fall back to :name:"
    );

    assert!(
        rows.iter().any(|r| r.edited),
        "edited marker should survive projection"
    );
}

#[test]
fn message_timestamps_are_recent_not_epoch() {
    let state = demo_state();
    let rows = project_messages(
        &state,
        concord::discord::Id::new(111),
        state.current_user_id(),
    );

    // Snowflakes encode time; a naive small id would render as 2015.
    let newest = rows.last().expect("fixture defines messages");
    let age = chrono::Local::now() - newest.timestamp;
    assert!(
        age.num_days() < 1,
        "fixture timestamps should be recent, got {}",
        newest.timestamp
    );
}

#[test]
fn typing_indicator_resolves_display_names() {
    let state = demo_state();
    let names = typing_names(
        &state,
        concord::discord::Id::new(112),
        Some(concord::discord::Id::new(10)),
    );

    assert_eq!(names, vec!["ferris".to_string()]);
}

#[test]
fn selection_is_identity_based_not_positional() {
    let state = demo_state();
    let model = project(&state, &guild_nav(10, Some(112)), true);

    let selected = &model.channels[model.selected_channel];
    assert_eq!(selected.name, "gui-rewrite");
    assert_eq!(selected.id, Some(concord::discord::Id::new(112)));
}

#[test]
fn voice_participants_project_with_state_flags() {
    let state = demo_state();
    let model = project(&state, &guild_nav(10, Some(111)), true);

    let standup = model
        .channels
        .iter()
        .find(|c| c.name == "Standup")
        .expect("fixture defines a voice channel");

    assert_eq!(standup.kind, ChannelKind::Voice);
    assert_eq!(standup.voice.len(), 3, "occupants should project");

    assert!(
        standup.voice.iter().any(|p| p.speaking),
        "speaking flag should survive"
    );
    assert!(
        standup.voice.iter().any(|p| p.muted),
        "muted flag should survive"
    );
    assert!(
        standup.voice.iter().any(|p| p.streaming),
        "streaming flag should survive"
    );
}

#[test]
fn text_channels_have_no_voice_occupants() {
    let state = demo_state();
    let model = project(&state, &guild_nav(10, None), true);

    let general = model
        .channels
        .iter()
        .find(|c| c.name == "general")
        .expect("fixture defines #general");
    assert!(general.voice.is_empty());
}

#[test]
fn threads_nest_under_their_parent_channel() {
    let state = demo_state();
    let model = project(&state, &guild_nav(10, None), true);

    let names: Vec<_> = model.channels.iter().map(|c| c.name.as_str()).collect();

    let parent = names
        .iter()
        .position(|n| *n == "gui-rewrite")
        .expect("fixture defines #gui-rewrite");
    let thread = names
        .iter()
        .position(|n| *n == "avatar-loading")
        .expect("fixture defines a thread");

    assert!(
        thread > parent,
        "threads must follow their parent, got {names:?}"
    );
    assert_eq!(model.channels[thread].kind, ChannelKind::Thread);
}

#[test]
fn archived_threads_are_marked_not_hidden() {
    let state = demo_state();
    let model = project(&state, &guild_nav(10, None), true);

    let archived = model
        .channels
        .iter()
        .find(|c| c.name == "old-discussion")
        .expect("archived threads must still project");

    assert!(archived.archived, "archived flag should survive projection");
    assert_eq!(archived.kind, ChannelKind::Thread);
}
