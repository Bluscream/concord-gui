//! Administering a server: its invites, its emoji, and what has been done.
//!
//! One popup with three tabs rather than three popups, for the same reason the
//! GUI has one panel: they are all "administering this server", and three
//! entry points to find would be worse than one with tabs in it.

use crate::discord::ids::{Id, marker::GuildMarker};
use crate::discord::{AppCommand, AuditLogEntryInfo, GuildEmojiInfo, GuildInviteInfo};
use crate::tui::text_input::{TextEditAction, TextInputState};

use super::super::DashboardState;
use super::{ActiveModalPopupKind, ModalPopup, SelectablePopupState, SelectablePopupTarget};

/// Which list the popup is showing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerPanelTab {
    Invites,
    Emoji,
    AuditLog,
}

impl ServerPanelTab {
    pub const ALL: [Self; 3] = [Self::Invites, Self::Emoji, Self::AuditLog];

    pub(in crate::tui) fn label(self) -> &'static str {
        match self {
            Self::Invites => "Invites",
            Self::Emoji => "Emoji",
            Self::AuditLog => "Audit log",
        }
    }

    fn load(self, guild_id: Id<GuildMarker>) -> AppCommand {
        match self {
            Self::Invites => AppCommand::LoadGuildInvites { guild_id },
            Self::Emoji => AppCommand::LoadGuildEmojis { guild_id },
            Self::AuditLog => AppCommand::LoadGuildAuditLog { guild_id },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct ServerManagementState {
    pub(super) guild_id: Id<GuildMarker>,
    pub(super) tab: ServerPanelTab,
    pub(super) selection: SelectablePopupState,
    pub(super) invites: Vec<GuildInviteInfo>,
    pub(super) emojis: Vec<GuildEmojiInfo>,
    pub(super) audit_log: Vec<AuditLogEntryInfo>,
    /// Set while the open tab's fetch is outstanding, so the popup can say so
    /// rather than looking like an empty list.
    pub(super) loading: bool,
    pub(super) error: Option<String>,
    /// The emoji being renamed and the name as typed, while renaming.
    pub(super) renaming: Option<(usize, TextInputState)>,
}

impl ServerManagementState {
    pub(in crate::tui) fn tab(&self) -> ServerPanelTab {
        self.tab
    }

    pub(in crate::tui) fn invites(&self) -> &[GuildInviteInfo] {
        &self.invites
    }

    pub(in crate::tui) fn emojis(&self) -> &[GuildEmojiInfo] {
        &self.emojis
    }

    pub(in crate::tui) fn audit_log(&self) -> &[AuditLogEntryInfo] {
        &self.audit_log
    }

    pub(in crate::tui) fn is_loading(&self) -> bool {
        self.loading
    }

    pub(in crate::tui) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// How many rows the open tab has, which is what the selection moves over.
    /// The name being typed, while renaming an emoji.
    pub(in crate::tui) fn renaming(&self) -> Option<&TextInputState> {
        self.renaming.as_ref().map(|(_, input)| input)
    }

    pub(in crate::tui) fn row_count(&self) -> usize {
        match self.tab {
            ServerPanelTab::Invites => self.invites.len(),
            ServerPanelTab::Emoji => self.emojis.len(),
            ServerPanelTab::AuditLog => self.audit_log.len(),
        }
    }
}

impl DashboardState {
    pub fn open_server_management(
        &mut self,
        guild_id: Id<GuildMarker>,
        tab: ServerPanelTab,
    ) -> Option<AppCommand> {
        self.popups
            .set_modal(ModalPopup::ServerManagement(ServerManagementState {
                guild_id,
                tab,
                selection: SelectablePopupState::default(),
                invites: Vec::new(),
                emojis: Vec::new(),
                audit_log: Vec::new(),
                loading: true,
                error: None,
                renaming: None,
            }));
        Some(tab.load(guild_id))
    }

    pub fn close_server_management(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::ServerManagement) {
            self.popups.clear_modal();
        }
    }

    pub(in crate::tui) fn server_management_state(&self) -> Option<&ServerManagementState> {
        self.popups.server_management()
    }

    /// Move to the next tab, fetching its list if it has not arrived.
    pub fn next_server_tab(&mut self) -> Option<AppCommand> {
        let state = self.popups.server_management_mut()?;
        let index = ServerPanelTab::ALL
            .iter()
            .position(|tab| *tab == state.tab)
            .unwrap_or(0);
        let tab = ServerPanelTab::ALL[(index + 1) % ServerPanelTab::ALL.len()];

        state.tab = tab;
        state.error = None;
        state.selection = SelectablePopupState::default();

        // Only fetch what has not arrived. Refetching on every tab switch
        // would spend requests for no new information; that is what reload is
        // for.
        let already_loaded = match tab {
            ServerPanelTab::Invites => !state.invites.is_empty(),
            ServerPanelTab::Emoji => !state.emojis.is_empty(),
            ServerPanelTab::AuditLog => !state.audit_log.is_empty(),
        };
        state.loading = !already_loaded;
        let guild_id = state.guild_id;
        (!already_loaded).then(|| tab.load(guild_id))
    }

    /// Start renaming the highlighted emoji.
    ///
    /// Seeded with the current name: a rename is usually a correction, and
    /// retyping the whole thing to fix one letter is busywork.
    pub fn start_emoji_rename(&mut self) {
        let Some(index) = self.selected_server_row() else {
            return;
        };
        let Some(state) = self.popups.server_management_mut() else {
            return;
        };
        if state.tab != ServerPanelTab::Emoji {
            return;
        }
        let Some(emoji) = state.emojis.get(index) else {
            return;
        };

        let mut input = TextInputState::default();
        input.set_value(emoji.name.clone());
        state.renaming = Some((index, input));
    }

    pub fn cancel_emoji_rename(&mut self) {
        if let Some(state) = self.popups.server_management_mut() {
            state.renaming = None;
        }
    }

    pub fn insert_emoji_rename_char(&mut self, value: char) {
        if let Some(state) = self.popups.server_management_mut()
            && let Some((_, input)) = &mut state.renaming
        {
            input.insert_char(value);
        }
    }

    pub fn edit_emoji_rename(&mut self, action: TextEditAction) {
        if let Some(state) = self.popups.server_management_mut()
            && let Some((_, input)) = &mut state.renaming
        {
            input.apply_edit_action(action);
        }
    }

    /// Send the renamed emoji.
    pub fn submit_emoji_rename(&mut self) -> Option<AppCommand> {
        let state = self.popups.server_management_mut()?;
        let (index, input) = state.renaming.take()?;
        let name = input.value().trim().to_owned();
        let guild_id = state.guild_id;
        let emoji = state.emojis.get_mut(index)?;

        // An empty name is a cancel rather than a rename to nothing, which
        // Discord would reject anyway.
        if name.is_empty() || emoji.name == name {
            return None;
        }

        // Applied locally too: the list is a snapshot, and leaving the old
        // name showing makes a successful rename look like it failed.
        emoji.name = name.clone();
        Some(AppCommand::RenameEmoji {
            guild_id,
            emoji_id: emoji.id,
            name,
        })
    }

    pub fn reload_server_management(&mut self) -> Option<AppCommand> {
        let state = self.popups.server_management_mut()?;
        state.loading = true;
        state.error = None;
        Some(state.tab.load(state.guild_id))
    }

    pub fn move_server_selection_down(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::ServerManagement,
            crate::tui::keybindings::SelectionAction::Next,
        );
    }

    pub fn move_server_selection_up(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::ServerManagement,
            crate::tui::keybindings::SelectionAction::Previous,
        );
    }

    pub(in crate::tui) fn selected_server_row(&self) -> Option<usize> {
        self.popups
            .server_management()
            .map(|state| state.selection.selected_for_len(state.row_count()))
    }

    /// Act on the highlighted row: revoke an invite, or delete an emoji.
    ///
    /// The row goes straight away rather than waiting for a refetch: the list
    /// is a snapshot, and leaving a revoked invite on screen invites a second
    /// revoke for a code that no longer exists. The audit log has no action -
    /// history is a record, not something to be edited from here.
    pub fn activate_selected_server_row(&mut self) -> Option<AppCommand> {
        let index = self.selected_server_row()?;
        let state = self.popups.server_management_mut()?;
        let guild_id = state.guild_id;

        match state.tab {
            ServerPanelTab::Invites => {
                if index >= state.invites.len() {
                    return None;
                }
                let invite = state.invites.remove(index);
                Some(AppCommand::RevokeInvite { code: invite.code })
            }
            ServerPanelTab::Emoji => {
                if index >= state.emojis.len() {
                    return None;
                }
                let emoji = state.emojis.remove(index);
                Some(AppCommand::DeleteEmoji {
                    guild_id,
                    emoji_id: emoji.id,
                    label: emoji.name,
                })
            }
            ServerPanelTab::AuditLog => None,
        }
    }

    pub(in crate::tui) fn apply_guild_invites(
        &mut self,
        guild_id: Id<GuildMarker>,
        invites: Vec<GuildInviteInfo>,
    ) {
        let Some(state) = self.popups.server_management_mut() else {
            return;
        };
        // A reply for a different guild belongs to a popup that has since been
        // closed and reopened elsewhere.
        if state.guild_id != guild_id {
            return;
        }
        state.loading = false;
        state.error = None;
        state.invites = invites;
    }

    pub(in crate::tui) fn apply_guild_emojis(
        &mut self,
        guild_id: Id<GuildMarker>,
        emojis: Vec<GuildEmojiInfo>,
    ) {
        let Some(state) = self.popups.server_management_mut() else {
            return;
        };
        if state.guild_id != guild_id {
            return;
        }
        state.loading = false;
        state.error = None;
        state.emojis = emojis;
    }

    pub(in crate::tui) fn apply_guild_audit_log(
        &mut self,
        guild_id: Id<GuildMarker>,
        entries: Vec<AuditLogEntryInfo>,
    ) {
        let Some(state) = self.popups.server_management_mut() else {
            return;
        };
        if state.guild_id != guild_id {
            return;
        }
        state.loading = false;
        state.error = None;
        state.audit_log = entries;
    }

    pub(in crate::tui) fn apply_server_management_failure(
        &mut self,
        guild_id: Id<GuildMarker>,
        message: String,
    ) {
        let Some(state) = self.popups.server_management_mut() else {
            return;
        };
        if state.guild_id != guild_id {
            return;
        }
        state.loading = false;
        state.error = Some(message);
    }
}
