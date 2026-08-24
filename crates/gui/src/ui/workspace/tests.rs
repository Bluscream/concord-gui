use super::super::*;
use crate::ui::workspace::*;
use concord::discord::*;
use concord_ui::model::AttachmentViewerZoom;

#[cfg(test)]
mod context_menu_tests {
    use super::*;

    #[test]
    fn every_message_action_has_a_distinct_slot() {
        // Slots build element ids. Two actions sharing one would make GPUI
        // treat two different controls as the same element.
        let actions = [
            MessageAction::Reply,
            MessageAction::React,
            MessageAction::Edit,
            MessageAction::Delete,
            MessageAction::ToggleReaction(0),
            MessageAction::RevealSpoiler,
            MessageAction::OpenProfile,
            MessageAction::LoadOlder,
            MessageAction::JumpToReplied,
            MessageAction::CopyText,
            MessageAction::CopyLink,
            MessageAction::ShowReactionUsers(0),
            MessageAction::TogglePin,
            MessageAction::VotePoll(0),
            MessageAction::DownloadAttachment(0),
            MessageAction::PlayAttachment(0),
            MessageAction::RemoveEmbeds,
            MessageAction::OpenLink(0),
            MessageAction::OpenThread,
            MessageAction::LoadNewer,
            MessageAction::Forward,
            MessageAction::ViewImage(0),
            MessageAction::ContextMenu(gpui::point(gpui::px(0.), gpui::px(0.))),
        ];

        let mut slots: Vec<usize> = actions.iter().map(|action| action.slot()).collect();
        slots.sort_unstable();
        slots.dedup();

        assert_eq!(
            slots.len(),
            actions.len(),
            "two message actions share an element-id slot"
        );
    }
}

#[cfg(test)]
mod permission_grid_tests {
    use super::*;

    fn grid(scope: PermissionScope) -> PermissionGridView {
        PermissionGridView {
            scope,
            name: "test".to_owned(),
            allow: 0,
            deny: 0,
            original_allow: 0,
            original_deny: 0,
        }
    }

    fn role_scope() -> PermissionScope {
        PermissionScope::Role {
            guild_id: concord::discord::Id::new(1),
            role_id: concord::discord::Id::new(2),
        }
    }

    fn overwrite_scope() -> PermissionScope {
        PermissionScope::ChannelOverwrite {
            channel_id: concord::discord::Id::new(3),
            target: concord::discord::OverwriteTarget::Role(concord::discord::Id::new(1)),
        }
    }

    #[test]
    fn a_role_has_no_inherit_to_offer() {
        // A role's bitfield has two states. Cycling through a third would show
        // a setting that cannot be saved.
        assert!(!grid(role_scope()).allows_inherit());
        assert!(grid(overwrite_scope()).allows_inherit());
    }

    #[test]
    fn an_unchanged_grid_is_not_dirty() {
        // Saving one would spend a request and write an audit entry saying
        // nothing happened.
        let mut view = grid(role_scope());
        assert!(!view.is_dirty());

        view.allow = 1;
        assert!(view.is_dirty());
    }

    #[test]
    fn deny_only_counts_as_a_change_too() {
        // The first version compared allow alone, which would have made a
        // deny-only overwrite look unchanged and silently discard it.
        let mut view = grid(overwrite_scope());
        view.deny = 1;

        assert!(view.is_dirty());
    }
}

#[cfg(test)]
mod access_tests {
    use super::*;

    fn view(sessions: usize, apps: usize) -> AccessView {
        AccessView {
            sessions: (0..sessions)
                .map(|index| concord::discord::AuthSession {
                    id_hash: format!("s{index}"),
                    os: "Linux".to_owned(),
                    platform: "Desktop".to_owned(),
                    location: None,
                    last_used: None,
                    current: index == 0,
                })
                .collect(),
            apps: (0..apps)
                .map(|index| concord::discord::AuthorisedApp {
                    id: format!("a{index}"),
                    name: format!("App {index}"),
                    scopes: Vec::new(),
                })
                .collect(),
            loading: false,
            error: None,
            logout_targets: std::collections::BTreeSet::new(),
            password: String::new(),
        }
    }

    #[test]
    fn the_password_is_never_drawn() {
        let mut access = view(1, 0);
        access.password = "hunter2".to_owned();

        let masked = access.masked_password();
        assert!(!masked.contains("hunter2"));
        assert_eq!(masked.chars().count(), 7);
    }

    #[test]
    fn bullets_count_characters_not_bytes() {
        // A multi-byte password would otherwise show more bullets than it has
        // characters, which reads as typing that did not land where it did.
        let mut access = view(1, 0);
        access.password = "héllo".to_owned();

        assert_eq!(access.masked_password().chars().count(), 5);
    }
}

#[cfg(test)]
mod account_view_tests {
    use super::*;
    use concord::discord::AccountField;

    fn view() -> AccountView {
        AccountView {
            form: concord::discord::AccountForm::new("someone", ""),
            focused: 0,
            totp_secret: None,
            totp_code: String::new(),
            backup_codes: Vec::new(),
        }
    }

    #[test]
    fn no_password_field_is_ever_drawn_or_printed() {
        let mut account = view();
        for field in [
            AccountField::NewPassword,
            AccountField::ConfirmPassword,
            AccountField::CurrentPassword,
        ] {
            account.form.set(field, "hunter2".to_owned());
            assert!(
                !account.form.display_value(field).contains("hunter2"),
                "{field:?} was drawn"
            );
        }
        // `{:?}` on the whole view is what a debug log does.
        assert!(!format!("{account:?}").contains("hunter2"));
    }

    #[test]
    fn an_ordinary_field_is_drawn_as_typed() {
        // Masking everything would hide the username the form exists to edit.
        let account = view();
        assert_eq!(
            account.form.display_value(AccountField::Username),
            "someone"
        );
    }
}

#[cfg(test)]
mod membership_tab_tests {
    use super::*;

    #[test]
    fn every_tab_that_needs_a_fetch_asks_for_one() {
        // A tab added without a fetch renders empty forever, which reads as a
        // server that has none of whatever the tab shows.
        let guild_id = Id::new(1);
        for tab in ServerTab::ALL {
            let fetches = tab.load(guild_id);
            // Settings, roles and members are read from the snapshot.
            let snapshot_tab = matches!(
                tab,
                ServerTab::Settings | ServerTab::Roles | ServerTab::Members
            );
            assert_eq!(
                fetches.is_empty(),
                snapshot_tab,
                "{tab:?} fetches {} commands",
                fetches.len()
            );
        }
    }

    #[test]
    fn membership_asks_for_all_three_things_it_shows() {
        // Returning one and queueing the rest at the call site is what left
        // the TUI's tab-switch path fetching nothing at all.
        let fetches = ServerTab::Membership.load(Id::new(1));

        assert!(
            fetches
                .iter()
                .any(|c| matches!(c, AppCommand::LoadWelcomeScreen { .. }))
        );
        assert!(
            fetches
                .iter()
                .any(|c| matches!(c, AppCommand::LoadGuildWidget { .. }))
        );
        assert!(
            fetches
                .iter()
                .any(|c| matches!(c, AppCommand::LoadPruneCount { .. }))
        );
    }

    #[test]
    fn the_prune_window_it_opens_on_is_one_discord_accepts() {
        // Discord rejects anything outside its own list, so a default outside
        // it would make the first count request fail every time.
        assert!(concord::discord::PRUNE_DAYS.contains(&DEFAULT_PRUNE_DAYS));
    }
}

#[cfg(test)]
mod activity_tests {
    use super::*;

    fn draft(kind: ActivityKind, values: [&str; ActivityDraft::FIELDS]) -> ActivityDraft {
        let mut draft = ActivityDraft::from_current(None);
        draft.kind = kind;
        for (field, value) in draft.fields.iter_mut().zip(values) {
            field.set_text(value);
        }
        draft
    }

    #[test]
    fn a_named_activity_carries_its_three_lines() {
        let activities = draft(
            ActivityKind::Listening,
            ["a record", "side two", "on vinyl"],
        )
        .to_activities();

        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].kind, ActivityKind::Listening);
        assert_eq!(activities[0].name, "a record");
        assert_eq!(activities[0].details.as_deref(), Some("side two"));
        assert_eq!(activities[0].state.as_deref(), Some("on vinyl"));
    }

    #[test]
    fn empty_optional_lines_are_omitted_rather_than_sent_blank() {
        let activities = draft(ActivityKind::Playing, ["chess", "  ", ""]).to_activities();

        assert_eq!(activities[0].details, None);
        assert_eq!(activities[0].state, None);
    }

    #[test]
    fn a_nameless_activity_broadcasts_nothing() {
        // Otherwise everyone who looks at the profile sees an empty line under
        // the name, which reads as a bug rather than as no activity.
        assert!(
            draft(ActivityKind::Playing, ["   ", "details", ""])
                .to_activities()
                .is_empty()
        );
    }

    #[test]
    fn reopening_the_editor_shows_what_is_being_broadcast() {
        let current = draft(ActivityKind::Watching, ["a film", "reel one", ""])
            .to_activities()
            .remove(0);
        let reopened = ActivityDraft::from_current(Some(&current));

        assert_eq!(reopened.kind, ActivityKind::Watching);
        assert_eq!(reopened.fields[0].text(), "a film");
        assert_eq!(reopened.fields[1].text(), "reel one");
        assert_eq!(reopened.fields[2].text(), "");
    }

    #[test]
    fn a_kind_the_editor_does_not_offer_falls_back_rather_than_vanishing() {
        // A custom status or an RPC stream reopens as Playing: the editor has
        // no control for those kinds, and silently keeping one would send it
        // back unchanged with whatever the user typed attached to it.
        let mut custom = draft(ActivityKind::Playing, ["something", "", ""])
            .to_activities()
            .remove(0);
        custom.kind = ActivityKind::Streaming;

        assert_eq!(
            ActivityDraft::from_current(Some(&custom)).kind,
            ActivityKind::Playing
        );
    }
}

#[cfg(test)]
mod server_management_tests {
    use super::*;

    fn invite(max_uses: Option<u32>, max_age_seconds: Option<u32>) -> GuildInviteInfo {
        GuildInviteInfo {
            code: "aBc-123".to_owned(),
            channel_id: None,
            channel_name: Some("general".to_owned()),
            inviter: Some("ferris".to_owned()),
            uses: 3,
            max_uses,
            max_age_seconds,
            temporary: false,
        }
    }

    #[test]
    fn an_unlimited_invite_does_not_read_as_spent() {
        // Discord writes "no limit" as 0 in both fields. Showing that straight
        // through would render as "3/0", which reads as already used up.
        let summary = invite_summary(&invite(None, None));

        assert!(summary.contains(&concord::t!("status-invite-unlimited")));
        assert!(summary.contains(&concord::t!("status-invite-never-expires")));
        assert!(!summary.contains("3/0"));
    }

    #[test]
    fn a_limited_invite_shows_what_is_left() {
        let summary = invite_summary(&invite(Some(10), Some(3600)));

        assert!(summary.contains("3/10"));
        assert!(summary.contains("60m"));
        assert!(summary.contains("#general"));
        assert!(summary.contains("ferris"));
    }

    fn emoji(animated: bool, role_restricted: bool) -> GuildEmojiInfo {
        GuildEmojiInfo {
            id: concord::discord::Id::new(1),
            name: "ferris".to_owned(),
            animated,
            role_restricted,
        }
    }

    #[test]
    fn an_ordinary_emoji_needs_no_explanation() {
        assert_eq!(emoji_summary(&emoji(false, false)), None);
    }

    #[test]
    fn an_unusual_emoji_says_why() {
        // Role restriction especially: a member who cannot use an emoji they
        // can see listed would otherwise have no way to find out why.
        let summary = emoji_summary(&emoji(true, true)).expect("should explain itself");

        assert!(summary.contains(&concord::t!("label-emoji-animated")));
        assert!(summary.contains(&concord::t!("label-emoji-restricted")));
    }

    fn entry(
        action: AuditLogAction,
        actor: Option<&str>,
        target: Option<&str>,
    ) -> AuditLogEntryInfo {
        AuditLogEntryInfo {
            id: concord::discord::Id::new(1),
            actor: actor.map(str::to_owned),
            action,
            target: target.map(str::to_owned),
            reason: None,
        }
    }

    #[test]
    fn an_audit_entry_reads_as_a_sentence() {
        assert_eq!(
            audit_line(&entry(
                AuditLogAction::MemberBanAdd,
                Some("ferris"),
                Some("spammer")
            )),
            "ferris banned spammer"
        );

        // An action with no target still reads, rather than trailing off.
        assert_eq!(
            audit_line(&entry(AuditLogAction::ChannelCreate, Some("ferris"), None)),
            "ferris created a channel"
        );
    }

    #[test]
    fn an_unnamed_action_is_still_shown() {
        // Discord keeps adding action types. Dropping the ones this client
        // does not know would quietly hide moderation from the log people
        // read specifically to find out what was done.
        let line = audit_line(&entry(AuditLogAction::Other(999), None, None));

        assert!(line.contains("999"));
        assert!(line.starts_with(&concord::t!("label-unknown")));
    }
}

#[cfg(test)]
mod image_viewer_tests {
    use super::*;

    fn view(count: usize) -> ImageViewerView {
        ImageViewerView {
            urls: (0..count).map(|index| format!("image-{index}")).collect(),
            index: 0,
            zoom: AttachmentViewerZoom::default(),
        }
    }

    #[test]
    fn stepping_wraps_at_both_ends() {
        // Forward off the end comes back to the start, and backward off the
        // start goes to the end. Stopping at an edge would look like the
        // control had broken.
        let mut images = view(3);

        images.step(true);
        assert_eq!(images.url(), Some("image-1"));
        images.step(true);
        images.step(true);
        assert_eq!(images.url(), Some("image-0"));

        images.step(false);
        assert_eq!(images.url(), Some("image-2"));
    }

    #[test]
    fn stepping_an_empty_viewer_does_nothing() {
        // Not reachable through view_image, which refuses to open with no
        // images - but a modulo by zero would panic, so it is guarded.
        let mut empty = view(0);
        empty.step(true);
        assert_eq!(empty.url(), None);
    }

    #[test]
    fn zoom_stops_at_each_end_rather_than_wrapping() {
        // Zoom is not a cycle: zooming in past the largest must not drop back
        // to the smallest, which would be the opposite of what was asked.
        let mut zoom = AttachmentViewerZoom::default();
        for _ in 0..5 {
            zoom = zoom.zoom_in();
        }
        assert_eq!(zoom, AttachmentViewerZoom::Fullscreen);

        for _ in 0..5 {
            zoom = zoom.zoom_out();
        }
        assert_eq!(zoom, AttachmentViewerZoom::Default);
    }

    #[test]
    fn a_single_image_still_has_a_url() {
        let single = view(1);
        assert_eq!(single.url(), Some("image-0"));
    }

    #[test]
    fn an_index_past_the_end_yields_nothing_rather_than_panicking() {
        // Reachable if a message is edited while its images are open.
        let mut stale = view(1);
        stale.index = 5;
        assert_eq!(stale.url(), None);
    }
}
