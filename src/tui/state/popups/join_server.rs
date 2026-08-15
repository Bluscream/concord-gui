//! Joining a server from an invite.
//!
//! Until invites existed a client could leave a guild but never join one, so
//! the official client was still needed for it. The flow is deliberately two
//! steps: an invite code says nothing about where it leads, so it is resolved
//! and shown before anything is joined.

use crate::discord::{AppCommand, InvitePreview, invite_code_from};
use crate::risk::RiskKind;
use crate::tui::text_input::{TextEditAction, TextInputState};

use super::super::DashboardState;
use super::{ActiveModalPopupKind, ModalPopup};

/// The join-server prompt, in whichever of its two stages it has reached.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::tui) struct JoinServerState {
    /// What the user has typed, until an invite resolves.
    input: TextInputState,
    /// Set once the lookup is in flight, so the prompt can say so rather than
    /// looking like it ignored the keypress.
    resolving: bool,
    /// The resolved invite, which is what turns the prompt into a preview.
    preview: Option<InvitePreview>,
    /// Why the invite could not be used.
    error: Option<String>,
}

impl JoinServerState {
    pub(in crate::tui) fn input(&self) -> &TextInputState {
        &self.input
    }

    pub(in crate::tui) fn preview(&self) -> Option<&InvitePreview> {
        self.preview.as_ref()
    }

    pub(in crate::tui) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(in crate::tui) fn is_resolving(&self) -> bool {
        self.resolving
    }

    /// Whether the preview is one that can actually be joined.
    pub(in crate::tui) fn is_joinable(&self) -> bool {
        self.preview
            .as_ref()
            .is_some_and(|preview| !preview.already_joined)
    }
}

impl DashboardState {
    pub fn open_join_server(&mut self) {
        self.popups
            .set_modal(ModalPopup::JoinServer(JoinServerState::default()));
    }

    pub fn close_join_server(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::JoinServer) {
            self.popups.clear_modal();
        }
    }

    pub(in crate::tui) fn join_server_state(&self) -> Option<&JoinServerState> {
        self.popups.join_server()
    }

    /// Apply a text edit to the invite field.
    ///
    /// Editing clears a previous error: the message referred to the old code,
    /// and leaving it up makes a corrected code look like it failed too.
    pub fn edit_join_server_input(&mut self, action: TextEditAction) -> bool {
        let Some(state) = self.popups.join_server_mut() else {
            return false;
        };
        let changed = state.input.apply_edit_action(action);
        if changed {
            state.error = None;
        }
        changed
    }

    pub fn insert_join_server_char(&mut self, value: char) -> bool {
        let Some(state) = self.popups.join_server_mut() else {
            return false;
        };
        state.input.insert_char(value);
        state.error = None;
        true
    }

    pub fn insert_join_server_str(&mut self, value: &str) -> bool {
        let Some(state) = self.popups.join_server_mut() else {
            return false;
        };
        state.input.insert_str(value);
        state.error = None;
        true
    }

    /// Submit whichever stage the prompt is on.
    ///
    /// One key does both steps: resolve what was typed, then join what was
    /// resolved. Two separate keys would mean explaining which is which.
    pub fn submit_join_server(&mut self) -> Option<AppCommand> {
        let state = self.popups.join_server_mut()?;

        if let Some(preview) = state.preview.clone() {
            if preview.already_joined {
                return None;
            }
            self.close_join_server();
            // Joining is the action most likely to get a third-party client
            // flagged, so it is explained before it happens.
            return self.request_risky(
                RiskKind::JoinGuild,
                AppCommand::AcceptInvite { code: preview.code },
            );
        }

        // Parsed by the core so both clients accept the same forms.
        let Some(code) = invite_code_from(state.input.value()) else {
            state.error = Some("That does not look like an invite".to_owned());
            return None;
        };

        state.resolving = true;
        state.error = None;
        Some(AppCommand::ResolveInvite { code })
    }

    /// Show a resolved invite.
    pub(in crate::tui) fn apply_resolved_invite(&mut self, preview: InvitePreview) {
        let Some(state) = self.popups.join_server_mut() else {
            return;
        };
        state.resolving = false;
        state.error = None;
        state.preview = Some(preview);
    }

    /// Report an invite that could not be resolved or joined.
    pub(in crate::tui) fn apply_invite_failure(&mut self, message: String) {
        let Some(state) = self.popups.join_server_mut() else {
            return;
        };
        state.resolving = false;
        // The preview is cleared too: a failure after one resolved means the
        // preview is no longer something that can be acted on.
        state.preview = None;
        state.error = Some(message);
    }
}
