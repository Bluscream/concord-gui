//! The permission grid.
//!
//! One editor, two uses. A role grants a permission or does not - two states.
//! A channel overwrite allows, denies, or says nothing and lets the role
//! decide - three. Modelling the role case as a special kind of overwrite
//! would be neater and wrong: a role's bitfield has no "inherit", and
//! pretending it does would make the editor show a state that cannot be saved.

use concord::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, RoleMarker},
};
use concord::discord::{AppCommand, OverwriteTarget, RoleEdit, permissions_catalogue};

use super::super::DashboardState;
use super::{ActiveModalPopupKind, ModalPopup, SelectablePopupState, SelectablePopupTarget};

/// What the grid is editing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) enum PermissionScope {
    /// A role's own permissions, guild-wide.
    Role {
        guild_id: Id<GuildMarker>,
        role_id: Id<RoleMarker>,
        name: String,
    },
    /// One role's overwrite on one channel.
    ChannelOverwrite {
        channel_id: Id<ChannelMarker>,
        target: OverwriteTarget,
        name: String,
    },
}

/// How one permission stands in an overwrite.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionSetting {
    Allow,
    /// Neither allowed nor denied here, so whatever the roles say applies.
    Inherit,
    Deny,
}

impl PermissionSetting {
    /// Step through the states this scope has.
    ///
    /// A role has two: it grants or it does not. Cycling through an inherit
    /// state that cannot be saved would offer a setting that silently becomes
    /// something else.
    fn next(self, allows_inherit: bool) -> Self {
        if !allows_inherit {
            // A role grants or it does not.
            return match self {
                Self::Allow => Self::Inherit,
                Self::Inherit | Self::Deny => Self::Allow,
            };
        }
        // Inherit, allow, deny, round again. The first version skipped allow
        // entirely - inherit went straight to deny - which a test caught.
        match self {
            Self::Inherit => Self::Allow,
            Self::Allow => Self::Deny,
            Self::Deny => Self::Inherit,
        }
    }

    pub fn marker(self) -> &'static str {
        match self {
            Self::Allow => "[+]",
            Self::Inherit => "[ ]",
            Self::Deny => "[-]",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct PermissionGridState {
    pub(super) scope: PermissionScope,
    /// Allowed bits. For a role this is the whole answer.
    pub(super) allow: u64,
    /// Denied bits. Always zero for a role, which has no deny.
    pub(super) deny: u64,
    /// What it was when the grid opened, so saving can send only the change.
    pub(super) original_allow: u64,
    pub(super) original_deny: u64,
    pub(super) selection: SelectablePopupState,
}

impl PermissionGridState {
    pub(in crate::tui) fn scope_name(&self) -> &str {
        match &self.scope {
            PermissionScope::Role { name, .. } | PermissionScope::ChannelOverwrite { name, .. } => {
                name
            }
        }
    }

    pub(in crate::tui) fn allows_inherit(&self) -> bool {
        matches!(self.scope, PermissionScope::ChannelOverwrite { .. })
    }

    /// How the permission at this index currently stands.
    pub(in crate::tui) fn setting(&self, index: usize) -> PermissionSetting {
        let Some(permission) = permissions_catalogue::ALL.get(index) else {
            return PermissionSetting::Inherit;
        };
        if permission.is_set(self.allow) {
            PermissionSetting::Allow
        } else if permission.is_set(self.deny) {
            PermissionSetting::Deny
        } else {
            PermissionSetting::Inherit
        }
    }

    pub(in crate::tui) fn len(&self) -> usize {
        permissions_catalogue::ALL.len()
    }

    /// Whether anything has been changed.
    pub(in crate::tui) fn is_dirty(&self) -> bool {
        self.allow != self.original_allow || self.deny != self.original_deny
    }
}

impl DashboardState {
    /// Edit a channel's overwrite for a role.
    ///
    /// Seeded from the channel's existing overwrite when it has one, so the
    /// grid opens on what is actually in force rather than on blank.
    pub fn open_channel_overwrite(
        &mut self,
        channel_id: Id<ChannelMarker>,
        role_id: Id<RoleMarker>,
        name: String,
    ) -> bool {
        let Some(channel) = self.discord.channel(channel_id) else {
            return false;
        };
        let Some(guild_id) = channel.guild_id else {
            return false;
        };
        if !self.discord.cache.can_manage_roles(guild_id) {
            self.show_error_toast(
                "you do not have permission".to_owned(),
                std::time::Instant::now(),
            );
            return false;
        }

        let existing = channel
            .permission_overwrites
            .iter()
            .find(|overwrite| overwrite.id == role_id.get());
        let (allow, deny) = existing.map_or((0, 0), |overwrite| (overwrite.allow, overwrite.deny));

        self.popups
            .set_modal(ModalPopup::PermissionGrid(PermissionGridState {
                scope: PermissionScope::ChannelOverwrite {
                    channel_id,
                    target: OverwriteTarget::Role(role_id),
                    name,
                },
                allow,
                deny,
                original_allow: allow,
                original_deny: deny,
                selection: SelectablePopupState::default(),
            }));
        true
    }

    /// Edit what a role may do.
    pub fn open_role_permissions(
        &mut self,
        guild_id: Id<GuildMarker>,
        role_id: Id<RoleMarker>,
    ) -> bool {
        let Some(role) = self
            .discord
            .cache
            .roles_for_guild(guild_id)
            .into_iter()
            .find(|role| role.id == role_id)
            .cloned()
        else {
            return false;
        };

        // Discord refuses a change to a role at or above your own highest,
        // which is what stops anyone granting themselves more than they have.
        if !self.discord.cache.can_manage_roles(guild_id)
            || !self.discord.cache.can_assign_role(guild_id, role_id)
        {
            self.show_error_toast(
                format!("{} is at or above your highest role", role.name),
                std::time::Instant::now(),
            );
            return false;
        }

        self.popups
            .set_modal(ModalPopup::PermissionGrid(PermissionGridState {
                scope: PermissionScope::Role {
                    guild_id,
                    role_id,
                    name: role.name.clone(),
                },
                allow: role.permissions,
                deny: 0,
                original_allow: role.permissions,
                original_deny: 0,
                selection: SelectablePopupState::default(),
            }));
        true
    }

    pub fn close_permission_grid(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::PermissionGrid) {
            self.popups.clear_modal();
        }
    }

    pub(in crate::tui) fn permission_grid_state(&self) -> Option<&PermissionGridState> {
        self.popups.permission_grid()
    }

    pub fn move_permission_selection_down(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::Permissions,
            crate::tui::keybindings::SelectionAction::Next,
        );
    }

    pub fn move_permission_selection_up(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::Permissions,
            crate::tui::keybindings::SelectionAction::Previous,
        );
    }

    pub(in crate::tui) fn selected_permission_index(&self) -> Option<usize> {
        self.popups
            .permission_grid()
            .map(|state| state.selection.selected_for_len(state.len()))
    }

    /// Step the highlighted permission through its states.
    pub fn cycle_selected_permission(&mut self) {
        let Some(index) = self.selected_permission_index() else {
            return;
        };
        let Some(state) = self.popups.permission_grid_mut() else {
            return;
        };
        let Some(permission) = permissions_catalogue::ALL.get(index).copied() else {
            return;
        };

        let allows_inherit = state.allows_inherit();
        let current = if permission.is_set(state.allow) {
            PermissionSetting::Allow
        } else if permission.is_set(state.deny) {
            PermissionSetting::Deny
        } else {
            PermissionSetting::Inherit
        };

        match current.next(allows_inherit) {
            PermissionSetting::Allow => {
                state.allow = permissions_catalogue::with(state.allow, permission, true);
                state.deny = permissions_catalogue::with(state.deny, permission, false);
            }
            PermissionSetting::Deny => {
                state.allow = permissions_catalogue::with(state.allow, permission, false);
                state.deny = permissions_catalogue::with(state.deny, permission, true);
            }
            PermissionSetting::Inherit => {
                state.allow = permissions_catalogue::with(state.allow, permission, false);
                state.deny = permissions_catalogue::with(state.deny, permission, false);
            }
        }
    }

    /// Save the grid.
    ///
    /// Nothing is sent when nothing changed: it would spend a request and
    /// write an audit log entry saying so.
    pub fn submit_permission_grid(&mut self) -> Option<AppCommand> {
        let state = self.popups.permission_grid()?;
        if !state.is_dirty() {
            self.close_permission_grid();
            return None;
        }

        let (allow, deny, scope) = (state.allow, state.deny, state.scope.clone());
        self.close_permission_grid();

        match scope {
            PermissionScope::Role {
                guild_id,
                role_id,
                name,
            } => Some(AppCommand::ModifyRole {
                guild_id,
                role_id,
                edit: Box::new(RoleEdit {
                    permissions: Some(allow),
                    ..RoleEdit::default()
                }),
                label: name,
            }),
            PermissionScope::ChannelOverwrite {
                channel_id,
                target,
                name,
            } => {
                // An overwrite that neither allows nor denies anything is not
                // an overwrite; Discord keeps the row, so it is removed rather
                // than saved as two empty bitfields.
                if allow == 0 && deny == 0 {
                    Some(AppCommand::DeleteChannelOverwrite {
                        channel_id,
                        target,
                        label: name,
                    })
                } else {
                    Some(AppCommand::SetChannelOverwrite {
                        channel_id,
                        target,
                        allow,
                        deny,
                        label: name,
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_role_has_two_states_and_an_overwrite_has_three() {
        // A role's bitfield has no "inherit". Cycling through one would offer
        // a setting that cannot be saved and silently becomes something else.
        let mut role = PermissionSetting::Inherit;
        role = role.next(false);
        assert_eq!(role, PermissionSetting::Allow);
        role = role.next(false);
        assert_eq!(role, PermissionSetting::Inherit);

        // Inherit, allow, deny, round again.
        let mut overwrite = PermissionSetting::Inherit;
        overwrite = overwrite.next(true);
        assert_eq!(overwrite, PermissionSetting::Allow);
        overwrite = overwrite.next(true);
        assert_eq!(overwrite, PermissionSetting::Deny);
        overwrite = overwrite.next(true);
        assert_eq!(overwrite, PermissionSetting::Inherit);
    }

    #[test]
    fn every_state_is_reachable_by_cycling_an_overwrite() {
        // Otherwise a setting exists that the editor cannot produce.
        let mut seen = std::collections::BTreeSet::new();
        let mut setting = PermissionSetting::Inherit;
        for _ in 0..6 {
            seen.insert(format!("{setting:?}"));
            setting = setting.next(true);
        }

        assert_eq!(seen.len(), 3, "all three states should be reachable");
    }

    #[test]
    fn allow_and_deny_are_mutually_exclusive() {
        // Discord rejects a bit set in both, and it means nothing anyway.
        let permission = permissions_catalogue::by_name("SEND_MESSAGES").expect("should exist");
        let allow = permissions_catalogue::with(0, permission, true);
        let deny = permissions_catalogue::with(0, permission, true);

        // Setting one must clear the other, which is what the cycle does.
        let cleared = permissions_catalogue::with(deny, permission, false);
        assert!(permission.is_set(allow));
        assert!(!permission.is_set(cleared));
    }
}
