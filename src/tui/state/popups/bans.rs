//! Reviewing and lifting a guild's bans.
//!
//! Without this a ban is a one-way door: `UnbanMember` needs a user id, and
//! nothing else in the client knows who is banned.

use crate::discord::ids::{Id, marker::GuildMarker};
use crate::discord::{AppCommand, GuildBanInfo};

use super::super::DashboardState;
use super::{ActiveModalPopupKind, ModalPopup, SelectablePopupState, SelectablePopupTarget};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct BanListState {
    pub(super) guild_id: Id<GuildMarker>,
    pub(super) selection: SelectablePopupState,
    pub(super) bans: Vec<GuildBanInfo>,
    /// Set while the fetch is outstanding, so the popup can say so rather
    /// than looking like an empty ban list.
    pub(super) loading: bool,
    pub(super) error: Option<String>,
}

impl BanListState {
    pub(in crate::tui) fn bans(&self) -> &[GuildBanInfo] {
        &self.bans
    }

    pub(in crate::tui) fn is_loading(&self) -> bool {
        self.loading
    }

    pub(in crate::tui) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

impl DashboardState {
    pub fn open_ban_list(&mut self, guild_id: Id<GuildMarker>) -> Option<AppCommand> {
        self.popups.set_modal(ModalPopup::BanList(BanListState {
            guild_id,
            selection: SelectablePopupState::default(),
            bans: Vec::new(),
            loading: true,
            error: None,
        }));
        Some(AppCommand::LoadGuildBans { guild_id })
    }

    pub fn close_ban_list(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::BanList) {
            self.popups.clear_modal();
        }
    }

    pub(in crate::tui) fn ban_list_state(&self) -> Option<&BanListState> {
        self.popups.ban_list()
    }

    /// Take a loaded ban list, if it is the one this popup asked for.
    pub(in crate::tui) fn apply_guild_bans(
        &mut self,
        guild_id: Id<GuildMarker>,
        bans: Vec<GuildBanInfo>,
    ) {
        let Some(state) = self.popups.ban_list_mut() else {
            return;
        };
        // A reply for a different guild belongs to a popup that has since been
        // closed and reopened elsewhere.
        if state.guild_id != guild_id {
            return;
        }
        state.loading = false;
        state.error = None;
        state.bans = bans;
    }

    pub(in crate::tui) fn apply_guild_bans_failure(
        &mut self,
        guild_id: Id<GuildMarker>,
        message: String,
    ) {
        let Some(state) = self.popups.ban_list_mut() else {
            return;
        };
        if state.guild_id != guild_id {
            return;
        }
        state.loading = false;
        state.error = Some(message);
    }

    pub(in crate::tui) fn selected_ban_index(&self) -> Option<usize> {
        self.popups
            .ban_list()
            .map(|state| state.selection.selected_for_len(state.bans.len()))
    }

    pub fn move_ban_selection_down(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::Bans,
            crate::tui::keybindings::SelectionAction::Next,
        );
    }

    pub fn move_ban_selection_up(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::Bans,
            crate::tui::keybindings::SelectionAction::Previous,
        );
    }

    /// Lift the highlighted ban.
    ///
    /// The row is removed straight away rather than waiting for a refetch: the
    /// list is a snapshot, and leaving a lifted ban on screen invites a second
    /// unban for someone already unbanned.
    pub fn unban_selected(&mut self) -> Option<AppCommand> {
        let index = self.selected_ban_index()?;
        let state = self.popups.ban_list_mut()?;
        if index >= state.bans.len() {
            return None;
        }

        let guild_id = state.guild_id;
        let ban = state.bans.remove(index);

        Some(AppCommand::UnbanMember {
            guild_id,
            user_id: ban.user_id,
            label: ban.username,
        })
    }
}
