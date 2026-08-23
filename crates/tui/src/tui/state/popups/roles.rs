//! Assigning roles to a member.
//!
//! Toggling stages a change; saving sends the whole role set. Discord has
//! per-role endpoints but neither the official client nor Abaddon uses them,
//! and sending the set avoids a race where two concurrent edits each drop the
//! other's change.

use concord::discord::AppCommand;
use concord::discord::ids::{
    Id,
    marker::{GuildMarker, RoleMarker, UserMarker},
};

use super::super::DashboardState;
use super::{ActiveModalPopupKind, ModalPopup, SelectablePopupState, SelectablePopupTarget};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct RolePickerState {
    pub(super) guild_id: Id<GuildMarker>,
    pub(super) user_id: Id<UserMarker>,
    pub(super) selection: SelectablePopupState,
    /// The member's roles as edited, which is what gets sent on save.
    pub(super) role_ids: Vec<Id<RoleMarker>>,
}

/// One row in the picker.
pub(in crate::tui) struct RolePickerItem {
    pub name: String,
    pub assigned: bool,
    /// Why this role cannot be changed, when it cannot.
    pub disabled_reason: Option<&'static str>,
}

impl DashboardState {
    pub fn open_role_picker(&mut self, guild_id: Id<GuildMarker>, user_id: Id<UserMarker>) {
        let role_ids = self
            .discord
            .cache
            .member_for_guild(guild_id, user_id)
            .map(|member| member.role_ids.clone())
            .unwrap_or_default();

        self.popups
            .set_modal(ModalPopup::RolePicker(RolePickerState {
                guild_id,
                user_id,
                selection: SelectablePopupState::default(),
                role_ids,
            }));
    }

    pub fn close_role_picker(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::RolePicker) {
            self.popups.clear_modal();
        }
    }

    /// Every role in the guild, with whether it is assigned and whether it can
    /// be changed.
    ///
    /// Roles at or above the current user's highest are listed but refused, so
    /// the reason is visible rather than the row simply missing.
    pub(in crate::tui) fn role_picker_items(&self) -> Vec<RolePickerItem> {
        let Some(picker) = self.popups.role_picker() else {
            return Vec::new();
        };

        let mut roles = self.discord.cache.roles_for_guild(picker.guild_id);
        // Highest first, which is how Discord orders them everywhere else.
        roles.sort_by(|a, b| b.position.cmp(&a.position).then(a.name.cmp(&b.name)));

        roles
            .into_iter()
            .filter(|role| role.id.get() != picker.guild_id.get())
            .map(|role| RolePickerItem {
                name: role.name.clone(),
                assigned: picker.role_ids.contains(&role.id),
                disabled_reason: (!self.discord.cache.can_assign_role(picker.guild_id, role.id))
                    .then_some("above your highest role"),
            })
            .collect()
    }

    pub(in crate::tui) fn selected_role_index(&self) -> Option<usize> {
        let count = self.role_picker_items().len();
        self.popups
            .role_picker()
            .map(|picker| picker.selection.selected_for_len(count))
    }

    pub fn move_role_selection_down(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::Roles,
            crate::tui::keybindings::SelectionAction::Next,
        );
    }

    pub fn move_role_selection_up(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::Roles,
            crate::tui::keybindings::SelectionAction::Previous,
        );
    }

    /// Add or remove the highlighted role, without sending yet.
    pub fn toggle_selected_role(&mut self) {
        let Some(index) = self.selected_role_index() else {
            return;
        };
        let Some(picker) = self.popups.role_picker() else {
            return;
        };
        let guild_id = picker.guild_id;

        let mut roles = self.discord.cache.roles_for_guild(guild_id);
        roles.sort_by(|a, b| b.position.cmp(&a.position).then(a.name.cmp(&b.name)));
        let Some(role_id) = roles
            .into_iter()
            .filter(|role| role.id.get() != guild_id.get())
            .nth(index)
            .map(|role| role.id)
        else {
            return;
        };

        if !self.discord.cache.can_assign_role(guild_id, role_id) {
            return;
        }

        if let Some(picker) = self.popups.role_picker_mut() {
            if let Some(position) = picker.role_ids.iter().position(|id| *id == role_id) {
                picker.role_ids.remove(position);
            } else {
                picker.role_ids.push(role_id);
            }
        }
    }

    /// Send the edited role set.
    pub fn save_role_picker(&mut self) -> Option<AppCommand> {
        let picker = self.popups.role_picker()?;
        let guild_id = picker.guild_id;
        let user_id = picker.user_id;
        let role_ids = picker.role_ids.clone();

        let label = self
            .discord
            .cache
            .member_display_name(guild_id, user_id)
            .map(str::to_owned)
            .unwrap_or_else(|| user_id.get().to_string());

        self.close_role_picker();
        Some(AppCommand::SetMemberRoles {
            guild_id,
            user_id,
            role_ids,
            label,
        })
    }
}
