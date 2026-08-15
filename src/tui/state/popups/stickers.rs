//! Choosing a sticker to send.
//!
//! Only the open guild's own stickers are offered. Sending another guild's
//! sticker requires Nitro, so listing them would mean showing choices that
//! fail on send.

use crate::discord::StickerInfo;
use crate::discord::ids::{Id, marker::StickerMarker};

use super::super::DashboardState;
use super::{ActiveModalPopupKind, ModalPopup, SelectablePopupState, SelectablePopupTarget};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::tui) struct StickerPickerState {
    pub(super) selection: SelectablePopupState,
}

impl DashboardState {
    pub fn open_sticker_picker(&mut self) {
        self.popups
            .set_modal(ModalPopup::StickerPicker(StickerPickerState::default()));
    }

    pub fn close_sticker_picker(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::StickerPicker) {
            self.popups.clear_modal();
        }
    }

    /// Stickers the open guild offers.
    pub(in crate::tui) fn sticker_picker_items(&self) -> Vec<StickerInfo> {
        if self.popups.sticker_picker().is_none() {
            return Vec::new();
        }
        let Some(guild_id) = self.selected_channel_guild_id() else {
            return Vec::new();
        };
        self.discord.cache.stickers_for_guild(guild_id).to_vec()
    }

    pub(in crate::tui) fn selected_sticker_index(&self) -> Option<usize> {
        let items = self.sticker_picker_items().len();
        self.popups
            .sticker_picker()
            .map(|picker| picker.selection.selected_for_len(items))
    }

    pub fn move_sticker_selection_down(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::Stickers,
            crate::tui::keybindings::SelectionAction::Next,
        );
    }

    pub fn move_sticker_selection_up(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::Stickers,
            crate::tui::keybindings::SelectionAction::Previous,
        );
    }

    /// Stage the highlighted sticker for the next send.
    ///
    /// Staged rather than sent immediately: a sticker can accompany text, and
    /// sending on selection would make that impossible.
    pub fn stage_selected_sticker(&mut self) {
        let items = self.sticker_picker_items();
        let Some(index) = self.selected_sticker_index() else {
            return;
        };
        let Some(sticker) = items.get(index) else {
            return;
        };
        let sticker_id: Id<StickerMarker> = sticker.id;
        self.close_sticker_picker();

        // Discord accepts at most three and refuses the whole message if more
        // are sent, so the cap is enforced here rather than by the server.
        if self.composer.pending_stickers.len() >= crate::discord::MAX_MESSAGE_STICKERS {
            return;
        }
        if !self.composer.pending_stickers.contains(&sticker_id) {
            self.composer.pending_stickers.push(sticker_id);
        }
    }

    /// Drop the most recently staged sticker.
    pub fn pop_pending_sticker(&mut self) -> bool {
        self.composer.pending_stickers.pop().is_some()
    }

    pub(in crate::tui) fn pending_sticker_count(&self) -> usize {
        self.composer.pending_stickers.len()
    }
}
