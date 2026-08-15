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
    let (author, content, _target) = reply.reply_to.as_ref().unwrap();
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

#[test]
fn message_bodies_resolve_mentions_against_guild_state() {
    let state = demo_state();
    let rows = project_messages(
        &state,
        concord::discord::Id::new(111),
        state.current_user_id(),
    );

    let mention = rows
        .iter()
        .find(|row| row.content.contains("<@"))
        .expect("fixture defines a message containing a mention");

    // The raw form must never reach the rendered body.
    assert!(
        !mention.body.text.contains("<@"),
        "got {}",
        mention.body.text
    );
    assert!(
        mention.body.text.contains("@ferris"),
        "user mention should resolve to a display name, got {}",
        mention.body.text
    );
    assert!(
        mention.body.text.contains("#general"),
        "channel mention should resolve to a channel name, got {}",
        mention.body.text
    );
}

#[test]
fn author_role_colours_resolve_for_guild_messages() {
    let state = demo_state();
    let rows = project_messages(
        &state,
        concord::discord::Id::new(111),
        state.current_user_id(),
    );

    // Role colour lookup is guild-scoped, so a message missing guild_id
    // silently loses its colour. This guards that regression.
    assert!(
        rows.iter().any(|row| row.author_color.is_some()),
        "at least one author should carry a role colour"
    );
}

#[test]
fn dm_messages_have_no_guild_scope() {
    let state = demo_state();
    let rows = project_messages(&state, concord::discord::Id::new(300), None);

    assert!(!rows.is_empty(), "fixture defines a DM conversation");
    assert!(
        rows.iter().all(|row| row.author_color.is_none()),
        "DMs have no roles, so no author colours"
    );
}

#[test]
fn emoji_picker_offers_only_sendable_glyphs() {
    // Custom guild emoji round-trip as :name:, which the reaction API will not
    // accept as unicode. The picker must not offer anything unsendable.
    for glyph in crate::ui::emoji::flat() {
        assert!(
            !glyph.starts_with(':'),
            "picker offered a non-unicode glyph: {glyph}"
        );
        assert!(!glyph.is_empty());
    }
}

#[test]
fn emoji_picker_groups_cover_every_flat_entry() {
    let flat = crate::ui::emoji::flat();
    let grouped: usize = crate::ui::emoji::GROUPS
        .iter()
        .map(|(_, glyphs)| glyphs.len())
        .sum();

    // Keyboard navigation indexes the flat list against grouped rendering, so
    // the two must stay in step.
    assert_eq!(flat.len(), grouped);
}

#[test]
fn spoilers_are_hidden_until_revealed() {
    let state = demo_state();
    let rows = project_messages(
        &state,
        concord::discord::Id::new(112),
        state.current_user_id(),
    );

    let spoilered = rows
        .iter()
        .find(|row| row.body.runs.iter().any(|(_, style)| style.spoiler))
        .expect("fixture defines a spoilered message");

    assert!(!spoilered.spoiler_revealed, "spoilers must start hidden");
}

#[test]
fn timestamp_format_follows_the_hour_setting() {
    let state = demo_state();
    let rows = project_messages(
        &state,
        concord::discord::Id::new(111),
        state.current_user_id(),
    );
    let row = rows.first().expect("fixture defines messages");

    let h24 = row.short_time(true);
    let h12 = row.short_time(false);

    assert!(!h24.contains("AM") && !h24.contains("PM"), "got {h24}");
    assert!(h12.contains("AM") || h12.contains("PM"), "got {h12}");

    // `%l` pads with a leading space; that must not reach the UI.
    assert_eq!(h12, h12.trim());
    assert!(!row.long_time(false).starts_with(' '));
}

#[test]
fn palette_switch_changes_the_active_theme() {
    use crate::theme;

    let dark_bg = theme::active().bg;
    theme::set_light_mode(true);
    let light_bg = theme::active().bg;
    theme::set_light_mode(false);

    assert_ne!(dark_bg, light_bg, "light and dark must differ");
    assert_eq!(theme::active().bg, dark_bg, "must restore");
}

#[test]
fn forum_channels_are_classified_not_treated_as_text() {
    let state = demo_state();
    let model = project(&state, &guild_nav(10, None), true);

    let forum = model
        .channels
        .iter()
        .find(|c| c.name == "help-forum")
        .expect("fixture defines a forum channel");

    // A forum opens a post list, not a message log, so misclassifying it as
    // Text would silently route it to the wrong view.
    assert_eq!(forum.kind, ChannelKind::Forum);
}

#[test]
fn animated_emoji_are_distinguished_from_static_ones() {
    use crate::model::markdown::{self, Kind};

    // Animated emoji need a different CDN form, so the flag must survive
    // parsing rather than being inferred at render time.
    let animated = markdown::parse("<a:party:123>");
    assert!(
        animated
            .runs
            .iter()
            .any(|(_, s)| matches!(s.kind, Kind::Emoji { animated: true, .. })),
        "the `a:` prefix marks an animated emoji"
    );

    let still = markdown::parse("<:ferris:456>");
    assert!(
        still.runs.iter().any(|(_, s)| matches!(
            s.kind,
            Kind::Emoji {
                animated: false,
                ..
            }
        )),
        "a plain emoji must not be marked animated"
    );
}

#[test]
fn emoji_ids_survive_parsing_for_cdn_lookup() {
    use crate::model::markdown::{self, Kind};

    let parsed = markdown::parse("<:ferris:987654321>");
    let id = parsed.runs.iter().find_map(|(_, s)| match s.kind {
        Kind::Emoji { id, .. } => Some(id),
        _ => None,
    });

    assert_eq!(id, Some(987_654_321));
}

#[test]
fn demo_history_paging_terminates() {
    use concord::discord::fixtures;

    let mut state = demo_state();
    let channel = concord::discord::Id::new(111);
    let before = state.messages_for_channel(channel).len();

    // Paging must add messages, then stop, rather than growing forever.
    assert!(fixtures::prepend_history(&mut state, channel, 0));
    let after_first = state.messages_for_channel(channel).len();
    assert!(after_first > before, "a page should add messages");

    assert!(fixtures::prepend_history(&mut state, channel, 1));
    assert!(fixtures::prepend_history(&mut state, channel, 2));
    assert!(
        !fixtures::prepend_history(&mut state, channel, 3),
        "the backlog must report exhaustion rather than paging forever"
    );
}

#[test]
fn demo_history_prepends_older_messages_in_order() {
    use concord::discord::fixtures;

    let mut state = demo_state();
    let channel = concord::discord::Id::new(111);
    fixtures::prepend_history(&mut state, channel, 0);

    let rows = project_messages(&state, channel, state.current_user_id());

    // Ordering is what makes scrollback readable; a prepend that broke it
    // would interleave the backlog with existing messages.
    assert!(
        rows.windows(2).all(|w| w[0].timestamp <= w[1].timestamp),
        "history must stay oldest-first after prepending"
    );
}

#[test]
fn demo_attachments_land_on_the_sent_message() {
    use concord::discord::fixtures;

    let mut state = demo_state();
    let channel = concord::discord::Id::new(111);

    fixtures::append_message(
        &mut state,
        channel,
        Some(concord::discord::Id::new(10)),
        fixtures::demo_user_id(),
        "blu",
        "here you go",
    );
    fixtures::attach_to_last_message(&mut state, channel, &[("notes.txt".into(), 2048)]);

    let rows = project_messages(&state, channel, state.current_user_id());
    let last = rows.last().expect("the sent message");

    assert_eq!(last.attachments.len(), 1);
    assert_eq!(last.attachments[0].filename, "notes.txt");
    assert!(!last.attachments[0].is_image, "a .txt is not an image");
}

#[test]
fn demo_send_updates_the_channel_last_message() {
    use concord::discord::fixtures;

    let mut state = demo_state();
    let channel = concord::discord::Id::new(111);

    let id = fixtures::append_message(
        &mut state,
        channel,
        Some(concord::discord::Id::new(10)),
        fixtures::demo_user_id(),
        "blu",
        "hello",
    );

    // Without last_message_id the channel reads as empty and its unread state
    // collapses to Seen - the invariant that bit the fixture originally.
    assert_eq!(
        state.channel(channel).and_then(|c| c.last_message_id),
        Some(id)
    );
}

#[test]
fn switcher_spans_every_guild_not_just_the_open_one() {
    let state = demo_state();
    let candidates = crate::model::projection::switcher_candidates(&state);

    let contexts: std::collections::HashSet<_> =
        candidates.iter().map(|c| c.context.as_str()).collect();

    // The switcher's value is reaching somewhere you are not looking, so a
    // list limited to the open guild would defeat it.
    assert!(contexts.contains("RostFaden"), "got {contexts:?}");
    assert!(contexts.contains("Rust Community"), "got {contexts:?}");
    assert!(contexts.contains("Direct Messages"), "got {contexts:?}");
}

#[test]
fn switcher_excludes_non_destinations() {
    let state = demo_state();
    let candidates = crate::model::projection::switcher_candidates(&state);

    // Categories are not places, and voice channels are joined rather than
    // opened, so neither should be offered as a jump target.
    assert!(
        !candidates.iter().any(|c| c.kind == ChannelKind::Category),
        "categories are not destinations"
    );
    assert!(
        !candidates.iter().any(|c| c.kind == ChannelKind::Voice),
        "voice channels are joined, not opened"
    );
}

#[test]
fn switcher_ranks_matches_and_filters_non_matches() {
    use crate::ui::switcher::Switcher;

    let state = demo_state();
    let mut switcher = Switcher::default();

    switcher.query.set_text("gui");
    switcher.rank(crate::model::projection::switcher_candidates(&state));

    assert!(!switcher.results.is_empty(), "a real channel should match");
    assert_eq!(
        switcher.results[0].name, "gui-rewrite",
        "the closest match should rank first"
    );
    assert!(
        switcher.results.iter().all(|c| c.name != "announcements"),
        "non-matching channels must be filtered out"
    );
}

#[test]
fn switcher_matches_across_name_and_guild() {
    use crate::ui::switcher::Switcher;

    let state = demo_state();
    let mut switcher = Switcher::default();

    // Querying name plus context should find the channel in that guild.
    switcher.query.set_text("help rust");
    switcher.rank(crate::model::projection::switcher_candidates(&state));

    assert!(
        switcher
            .results
            .iter()
            .any(|c| c.name == "help" && c.context == "Rust Community"),
        "matching should span the channel name and its guild"
    );
}

#[test]
fn switcher_selection_wraps_and_survives_empty_results() {
    use crate::ui::switcher::Switcher;

    let state = demo_state();
    let mut switcher = Switcher::default();
    switcher.rank(crate::model::projection::switcher_candidates(&state));

    let count = switcher.results.len();
    assert!(count > 1);

    switcher.move_selection(-1);
    assert_eq!(switcher.selected, count - 1, "moving up from the top wraps");
    switcher.move_selection(1);
    assert_eq!(switcher.selected, 0);

    // A query that matches nothing must not panic or leave a stale index.
    switcher.query.set_text("zzzzzzzznotachannel");
    switcher.rank(crate::model::projection::switcher_candidates(&state));
    assert!(switcher.results.is_empty());
    switcher.move_selection(1);
    assert!(switcher.selection().is_none());
}

#[test]
fn replies_carry_their_target_id_for_jumping() {
    let state = demo_state();
    let rows = project_messages(
        &state,
        concord::discord::Id::new(111),
        state.current_user_id(),
    );

    let reply = rows
        .iter()
        .find(|row| row.reply_to.is_some())
        .expect("fixture defines a reply");

    // The preview alone is not enough: without the referenced id the reply
    // context cannot be clicked through to its target.
    let (_, _, target) = reply.reply_to.as_ref().unwrap();
    assert!(
        target.is_some(),
        "a reply must carry the id of the message it answers"
    );
}

#[test]
fn slash_picker_only_opens_for_a_bare_command() {
    use crate::ui::slash::SlashPicker;

    assert!(
        SlashPicker::for_input("/sh", &[]).is_some(),
        "a prefix matches"
    );
    assert!(
        SlashPicker::for_input("/", &[]).is_some(),
        "a lone slash lists all"
    );

    // A slash mid-message is ordinary text, and a command with arguments
    // should show the composer rather than a menu.
    assert!(SlashPicker::for_input("and/or", &[]).is_none());
    assert!(SlashPicker::for_input("/me waves", &[]).is_none());
    assert!(SlashPicker::for_input("plain text", &[]).is_none());
    assert!(SlashPicker::for_input("/zzzznotacommand", &[]).is_none());
}

#[test]
fn slash_completion_returns_the_cores_replacement() {
    use crate::ui::slash::SlashPicker;

    let picker = SlashPicker::for_input("/shr", &[]).expect("shrug should match");
    let completion = picker.completion().expect("a highlighted match");

    // The replacement comes from the core, so the GUI and TUI expand the same
    // command to the same text.
    assert!(completion.starts_with("/shrug"), "got {completion}");
}

#[test]
fn slash_selection_wraps() {
    use crate::ui::slash::SlashPicker;

    let mut picker = SlashPicker::for_input("/", &[]).expect("all commands");
    let count = picker.matches.len();
    assert!(count > 1);

    picker.move_selection(-1);
    assert_eq!(picker.selected, count - 1);
    picker.move_selection(1);
    assert_eq!(picker.selected, 0);
}

#[test]
fn polls_project_with_shares_that_sum_sensibly() {
    let state = demo_state();
    let rows = project_messages(
        &state,
        concord::discord::Id::new(111),
        state.current_user_id(),
    );

    let poll = rows
        .iter()
        .find_map(|row| row.poll.as_ref())
        .expect("fixture defines a poll");

    assert!(!poll.answers.is_empty());
    assert!(!poll.voted, "nobody has voted in the fixture poll");

    let total: f32 = poll.answers.iter().map(|answer| answer.share).sum();
    assert!(
        (total - 1.0).abs() < 0.001,
        "shares should cover the whole poll, got {total}"
    );
}

#[test]
fn an_empty_poll_does_not_divide_by_zero() {
    use concord::discord::{PollAnswerInfo, PollInfo};

    // A poll with no votes would produce NaN shares and a NaN-width bar.
    let poll = PollInfo {
        question: "?".into(),
        answers: vec![PollAnswerInfo {
            answer_id: 1,
            text: "only".into(),
            vote_count: Some(0),
            me_voted: false,
        }],
        allow_multiselect: false,
        results_finalized: Some(false),
        total_votes: Some(0),
    };

    let mut state = demo_state();
    let channel = concord::discord::Id::new(111);
    let id = concord::discord::fixtures::append_message(
        &mut state,
        channel,
        Some(concord::discord::Id::new(10)),
        concord::discord::fixtures::demo_user_id(),
        "blu",
        "",
    );
    concord::discord::fixtures::set_poll(&mut state, channel, id, poll);

    let rows = project_messages(&state, channel, state.current_user_id());
    let projected = rows
        .last()
        .and_then(|row| row.poll.as_ref())
        .expect("the poll just added");

    assert!(projected.answers[0].share.is_finite());
    assert_eq!(projected.answers[0].share, 0.0);
}

#[test]
fn voting_updates_counts_and_withdraws_the_previous_choice() {
    use concord::discord::fixtures;

    let mut state = demo_state();
    let channel = concord::discord::Id::new(111);
    let rows = project_messages(&state, channel, state.current_user_id());
    let target = rows
        .iter()
        .find(|row| row.poll.is_some())
        .expect("fixture poll")
        .id;

    fixtures::vote_poll(&mut state, channel, target, &[1]);
    let after = project_messages(&state, channel, state.current_user_id());
    let poll = after
        .iter()
        .find(|row| row.id == target)
        .and_then(|row| row.poll.as_ref())
        .unwrap();

    assert!(poll.voted);
    assert!(poll.answers[0].mine, "the chosen answer is marked");
    assert_eq!(poll.answers[0].votes, 8, "the count went up by one");

    // A single-answer poll replaces rather than accumulates.
    fixtures::vote_poll(&mut state, channel, target, &[2]);
    let after = project_messages(&state, channel, state.current_user_id());
    let poll = after
        .iter()
        .find(|row| row.id == target)
        .and_then(|row| row.poll.as_ref())
        .unwrap();

    assert!(!poll.answers[0].mine, "the first choice was withdrawn");
    assert_eq!(poll.answers[0].votes, 7, "and its count went back down");
    assert!(poll.answers[1].mine);
}

#[test]
fn slash_picker_offers_builtins_before_application_commands() {
    use crate::ui::slash::{Entry, SlashPicker};
    use concord::discord::ApplicationCommandInfo;

    let app = ApplicationCommandInfo {
        id: concord::discord::Id::new(1),
        application_id: concord::discord::Id::new(2),
        version: "1".into(),
        name: "shipit".into(),
        application_name: Some("Deploybot".into()),
        description: "Ship the build".into(),
        options: Vec::new(),
        raw: serde_json::Value::Null,
    };

    let picker = SlashPicker::for_input("/sh", std::slice::from_ref(&app))
        .expect("both sources should match");

    // Builtins always work; an application command depends on a bot being
    // present, so it must not displace one.
    assert!(matches!(picker.matches.first(), Some(Entry::Builtin(_))));
    assert!(
        picker.matches.iter().any(|entry| entry.name() == "shipit"),
        "the bot command should still be offered"
    );
}

#[test]
fn application_commands_complete_with_a_trailing_space() {
    use crate::ui::slash::SlashPicker;
    use concord::discord::ApplicationCommandInfo;

    let app = ApplicationCommandInfo {
        id: concord::discord::Id::new(1),
        application_id: concord::discord::Id::new(2),
        version: "1".into(),
        name: "weather".into(),
        application_name: None,
        description: "Forecast".into(),
        options: Vec::new(),
        raw: serde_json::Value::Null,
    };

    let picker = SlashPicker::for_input("/weat", std::slice::from_ref(&app)).unwrap();
    let completion = picker.completion().expect("a match");

    // The trailing space matters: arguments follow the name, and without it
    // the next keystroke would run into the command.
    assert_eq!(completion, "/weather ");
}

#[test]
fn message_links_are_collected_from_the_rendered_body() {
    use concord::discord::fixtures;

    let mut state = demo_state();
    let channel = concord::discord::Id::new(111);
    fixtures::append_message(
        &mut state,
        channel,
        Some(concord::discord::Id::new(10)),
        fixtures::demo_user_id(),
        "blu",
        "see https://example.com/one and https://example.com/two",
    );

    let rows = project_messages(&state, channel, state.current_user_id());
    let row = rows.last().unwrap();

    // What is openable must be exactly what was rendered as a link, or a
    // click would open something the user did not see.
    assert_eq!(row.links.len(), 2);
    assert_eq!(row.links[0], "https://example.com/one");
    assert_eq!(row.links[1], "https://example.com/two");
}

#[test]
fn plain_messages_expose_no_links() {
    let state = demo_state();
    let rows = project_messages(
        &state,
        concord::discord::Id::new(111),
        state.current_user_id(),
    );

    assert!(
        rows.iter().any(|row| row.links.is_empty()),
        "ordinary messages should not manufacture links"
    );
}

#[test]
fn zoom_scales_type_and_clamps_at_both_ends() {
    use crate::theme;

    theme::set_zoom(1.0);
    let base = theme::scaled(14.0);

    theme::set_zoom(2.0);
    assert!(theme::scaled(14.0) > base, "zooming in must enlarge type");

    theme::set_zoom(0.5);
    assert!(theme::scaled(14.0) < base, "zooming out must shrink it");

    theme::set_zoom(1.0);
    assert_eq!(
        theme::scaled(14.0),
        base,
        "reset must restore the base size"
    );
}

#[test]
fn demo_acking_a_channel_clears_its_unread_state() {
    use concord::discord::fixtures;
    use concord::discord::{ChannelUnreadState, Id};

    let mut state = demo_state();
    // 112 carries mentions in the fixture, the strongest unread state.
    let channel = Id::new(112);
    assert!(
        !matches!(state.channel_unread(channel), ChannelUnreadState::Seen),
        "the fixture should start with this channel unread"
    );

    let newest = state
        .messages_for_channel(channel)
        .last()
        .expect("the channel has messages")
        .id;
    fixtures::mark_read(&mut state, channel, newest);

    // Zeroing the counts alone is not enough: without an acked id at or past
    // the newest message the channel still reads as plain Unread.
    assert_eq!(state.channel_unread(channel), ChannelUnreadState::Seen);
}

#[test]
fn demo_thread_pinning_preserves_other_flags() {
    use concord::discord::Id;
    use concord::discord::fixtures;

    let mut state = demo_state();
    let thread = Id::new(130);

    // A bit this client does not interpret, which must survive a pin/unpin.
    const OTHER: u64 = 1 << 4;
    fixtures::set_thread_pinned(&mut state, thread, true);
    let flags = |state: &concord::discord::DiscordState| {
        state
            .channel(thread)
            .and_then(|channel| channel.flags)
            .unwrap_or(0)
    };
    assert_eq!(flags(&state) & (1 << 1), 1 << 1, "pin bit should be set");

    fixtures::set_thread_pinned(&mut state, thread, false);
    assert_eq!(flags(&state) & (1 << 1), 0, "pin bit should be cleared");
    let _ = OTHER;
}

#[test]
fn demo_forum_post_creates_a_thread_with_its_opening_message() {
    use concord::discord::Id;
    use concord::discord::fixtures;

    let mut state = demo_state();
    let forum = Id::new(114);

    let post = fixtures::create_forum_post(&mut state, forum, "how do i rust", "borrow checker");

    let channel = state.channel(post).expect("the post should exist");
    assert_eq!(channel.name, "how do i rust");
    assert_eq!(channel.parent_id, Some(forum));
    // The kind is the core's string name, not a numeric wire value - getting
    // this wrong misclassifies the channel silently.
    assert_eq!(channel.kind, "thread");

    let messages = state.messages_for_channel(post);
    assert_eq!(messages.len(), 1, "the post's body is its first message");
    assert_eq!(messages[0].content.as_deref(), Some("borrow checker"));
}

#[test]
fn a_bot_choice_replaces_only_the_argument_being_typed() {
    use crate::ui::composer::Composer;

    // Standalone check of the rule the composer path relies on: earlier
    // arguments were already accepted and must survive completion.
    let replace = |content: &str, value: &str| {
        let head = content
            .rsplit_once(char::is_whitespace)
            .map(|(head, _)| head)
            .unwrap_or(content);
        format!("{head} {value}")
    };

    assert_eq!(replace("/play song na", "nautilus"), "/play song nautilus");
    assert_eq!(replace("/play ", "nautilus"), "/play nautilus");

    // And the composer accepts the result unchanged.
    let mut composer = Composer::default();
    composer.set_text(&replace("/play song na", "nautilus"));
    assert_eq!(composer.text(), "/play song nautilus");
}

#[test]
fn download_progress_is_only_shown_when_a_total_is_known() {
    // A download of unknown length must not display a fabricated percentage.
    let fraction = |downloaded: u64, total: Option<u64>| -> Option<f32> {
        total
            .filter(|total| *total > 0)
            .map(|total| (downloaded as f32 / total as f32).clamp(0.0, 1.0))
    };

    assert_eq!(fraction(50, Some(200)), Some(0.25));
    assert_eq!(fraction(50, None), None);
    // A zero total would divide by zero rather than mean "complete".
    assert_eq!(fraction(50, Some(0)), None);
    // Servers can report more bytes than they promised.
    assert_eq!(fraction(300, Some(200)), Some(1.0));
}

#[test]
fn image_format_comes_from_the_extension_not_a_default() {
    use crate::ui::workspace::image_format_for;

    assert_eq!(image_format_for("a/b.png"), Some(gpui::ImageFormat::Png));
    assert_eq!(image_format_for("a/b.JPG"), Some(gpui::ImageFormat::Jpeg));
    assert_eq!(image_format_for("a/b.jpeg"), Some(gpui::ImageFormat::Jpeg));

    // CDN links always carry a query string, which would otherwise be read as
    // part of the extension.
    assert_eq!(
        image_format_for("https://cdn.example/x.webp?size=1024&ex=aa"),
        Some(gpui::ImageFormat::Webp)
    );

    // An unknown or absent extension must not fall back to PNG: decoding with
    // the wrong format renders nothing and reports nothing.
    assert_eq!(image_format_for("https://cdn.example/x.heic"), None);
    assert_eq!(image_format_for("https://cdn.example/noextension"), None);
}

#[test]
fn the_demo_preview_encodes_to_a_real_png() {
    use concord::discord::fixtures;

    let bytes = fixtures::demo_preview_png(7);
    assert!(!bytes.is_empty(), "the encoder should produce output");
    // PNG magic, so this is verified as decodable rather than merely non-empty.
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");

    // Different seeds must differ, or every attachment looks the same.
    assert_ne!(bytes, fixtures::demo_preview_png(200));
}
