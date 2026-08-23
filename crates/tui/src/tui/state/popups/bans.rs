//! Reviewing and lifting a guild's bans.
//!
//! Without this a ban is a one-way door: `UnbanMember` needs a user id, and
//! nothing else in the client knows who is banned.

use concord::discord::ids::{Id, marker::GuildMarker};
use concord::discord::{AppCommand, GuildBanInfo};

use super::super::DashboardState;
use super::{ActiveModalPopupKind, ModalPopup, SelectablePopupState, SelectablePopupTarget};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct BanListState {
    pub(super) guild_id: Id<GuildMarker>,
    pub(super) selection: SelectablePopupState,
    pub(super) bans: Vec<GuildBanInfo>,
    /// Set while the fetch is outstanding, so the popup can say so rather
    /// than looking like an empty ban list.
    /// Ids being typed for a bulk ban, while the field is open.
    pub(super) bulk_ban_input: Option<crate::tui::text_input::TextInputState>,
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
            bulk_ban_input: None,
            loading: true,
            error: None,
        }));
        Some(AppCommand::LoadGuildBans { guild_id })
    }

    /// Start typing a list of ids to ban.
    ///
    /// A typed list rather than a member picker: this is what raid cleanup
    /// looks like in practice, the ids are pasted from somewhere else, and the
    /// panel already has room for one field.
    pub fn start_bulk_ban(&mut self) {
        if let Some(state) = self.popups.ban_list_mut() {
            state.bulk_ban_input = Some(crate::tui::text_input::TextInputState::default());
        }
    }

    pub fn is_bulk_ban_open(&self) -> bool {
        self.popups
            .ban_list()
            .is_some_and(|state| state.bulk_ban_input.is_some())
    }

    pub fn bulk_ban_text(&self) -> Option<&str> {
        self.popups
            .ban_list()
            .and_then(|state| state.bulk_ban_input.as_ref())
            .map(crate::tui::text_input::TextInputState::value)
    }

    /// How many ids are currently readable, for the line under the field.
    pub fn bulk_ban_count(&self) -> usize {
        self.bulk_ban_text()
            .map_or(0, |text| concord::discord::parse_user_id_list(text).len())
    }

    pub fn edit_bulk_ban(&mut self, action: crate::tui::text_input::TextEditAction) {
        if let Some(state) = self.popups.ban_list_mut()
            && let Some(input) = state.bulk_ban_input.as_mut()
        {
            input.apply_edit_action(action);
        }
    }

    pub fn insert_bulk_ban_char(&mut self, value: char) {
        if let Some(state) = self.popups.ban_list_mut()
            && let Some(input) = state.bulk_ban_input.as_mut()
        {
            input.insert_char(value);
        }
    }

    pub fn cancel_bulk_ban(&mut self) {
        if let Some(state) = self.popups.ban_list_mut() {
            state.bulk_ban_input = None;
        }
    }

    /// The ban this field describes, for the risk prompt.
    pub fn pending_bulk_ban(&self) -> Option<AppCommand> {
        let state = self.popups.ban_list()?;
        let text = state.bulk_ban_input.as_ref()?.value();
        let user_ids = concord::discord::parse_user_id_list(text);
        // Nothing readable is not a ban of nobody: it is a typo, and warning
        // about it would teach the wrong lesson about the warning.
        if user_ids.is_empty() {
            return None;
        }
        Some(AppCommand::BulkBanMembers {
            guild_id: state.guild_id,
            user_ids,
            // No message deletion by default: it is a separate decision from
            // who to ban, and the destructive default would be the wrong one.
            delete_message_seconds: 0,
        })
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
