#[cfg(test)]
mod context_menu_tests {
    use super::super::*;
    use crate::ui::workspace::*;
    use concord::discord::*;

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
    use super::super::*;
    use crate::ui::workspace::*;
    use concord::discord::*;

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
    use super::super::*;
    use crate::ui::workspace::*;
    use concord::discord::*;

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
    use super::super::*;
    use crate::ui::workspace::*;
    use concord::discord::*;
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
    use super::super::*;
    use crate::ui::workspace::*;
    use concord::discord::*;

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
