//! Creating, renaming and deleting channels.
//!
//! One popup with a text field and, when creating, a kind to pick. Deleting
//! gets a confirmation of its own because it takes the channel's whole history
//! with it and Discord offers no undo.

use crate::discord::ids::{Id, marker::ChannelMarker};
use crate::discord::{AppCommand, ChannelEdit, NewChannelKind};
use crate::tui::text_input::{TextEditAction, TextInputState};

use super::super::DashboardState;
use super::{ActiveModalPopupKind, ModalPopup};

/// What the channel-edit popup is for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChannelEditPurpose {
    /// A new channel, with the kind being picked alongside the name.
    Create {
        kind: NewChannelKind,
    },
    Rename {
        channel_id: Id<ChannelMarker>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct ChannelEditState {
    pub(super) purpose: ChannelEditPurpose,
    pub(super) name: TextInputState,
}

impl ChannelEditState {
    pub(in crate::tui) fn purpose(&self) -> &ChannelEditPurpose {
        &self.purpose
    }

    pub(in crate::tui) fn name(&self) -> &TextInputState {
        &self.name
    }
}

/// A channel about to be deleted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct ChannelDeleteState {
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) name: String,
}

impl DashboardState {
    pub(in crate::tui) fn open_channel_create(&mut self) {
        self.popups.confirmation_button = Default::default();
        self.popups
            .set_modal(ModalPopup::ChannelEdit(ChannelEditState {
                purpose: ChannelEditPurpose::Create {
                    kind: NewChannelKind::Text,
                },
                name: TextInputState::default(),
            }));
    }

    pub(in crate::tui) fn open_channel_rename(&mut self, channel_id: Id<ChannelMarker>) {
        let mut name = TextInputState::default();
        // Seeded with the current name: a rename is usually a correction, and
        // retyping the whole thing to fix one letter is busywork.
        if let Some(channel) = self.discord.channel(channel_id) {
            name.set_value(channel.name.clone());
        }

        self.popups
            .set_modal(ModalPopup::ChannelEdit(ChannelEditState {
                purpose: ChannelEditPurpose::Rename { channel_id },
                name,
            }));
    }

    pub(in crate::tui) fn open_channel_delete_confirmation(
        &mut self,
        channel_id: Id<ChannelMarker>,
        name: String,
    ) {
        self.popups.confirmation_button = Default::default();
        self.popups
            .set_modal(ModalPopup::ChannelDelete(ChannelDeleteState {
                channel_id,
                name,
            }));
    }

    pub fn close_channel_edit(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::ChannelEdit) {
            self.popups.clear_modal();
        }
    }

    pub fn close_channel_delete(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::ChannelDelete) {
            self.popups.clear_modal();
        }
    }

    pub(in crate::tui) fn channel_edit_state(&self) -> Option<&ChannelEditState> {
        self.popups.channel_edit()
    }

    pub(in crate::tui) fn channel_delete_name(&self) -> Option<String> {
        self.popups.channel_delete().map(|state| state.name.clone())
    }

    pub fn edit_channel_name(&mut self, action: TextEditAction) {
        if let Some(state) = self.popups.channel_edit_mut() {
            state.name.apply_edit_action(action);
        }
    }

    pub fn insert_channel_name_char(&mut self, value: char) {
        if let Some(state) = self.popups.channel_edit_mut() {
            state.name.insert_char(value);
        }
    }

    /// Step through the kinds a new channel can be.
    pub fn cycle_new_channel_kind(&mut self) {
        let Some(state) = self.popups.channel_edit_mut() else {
            return;
        };
        let ChannelEditPurpose::Create { kind } = &mut state.purpose else {
            return;
        };
        let index = NewChannelKind::ALL
            .iter()
            .position(|candidate| candidate == kind)
            .unwrap_or(0);
        *kind = NewChannelKind::ALL[(index + 1) % NewChannelKind::ALL.len()];
    }

    /// Create or rename, whichever the popup was opened for.
    pub fn submit_channel_edit(&mut self) -> Option<AppCommand> {
        let state = self.popups.channel_edit()?;
        let name = state.name.value().trim().to_owned();
        // An empty name is a cancel rather than a channel called nothing,
        // which Discord would reject anyway.
        if name.is_empty() {
            return None;
        }

        let purpose = state.purpose.clone();
        self.close_channel_edit();

        match purpose {
            ChannelEditPurpose::Create { kind } => {
                let guild_id = match self.navigation.guilds.active {
                    crate::tui::state::ActiveGuildScope::Guild(guild_id) => guild_id,
                    _ => return None,
                };
                Some(AppCommand::CreateGuildChannel {
                    guild_id,
                    name,
                    kind,
                    // Created at the top level. Putting it in the category the
                    // cursor happens to be near would be a guess.
                    parent_id: None,
                })
            }
            ChannelEditPurpose::Rename { channel_id } => {
                let label = self
                    .discord
                    .channel(channel_id)
                    .map(|channel| channel.name.clone())
                    .unwrap_or_else(|| name.clone());

                Some(AppCommand::ModifyChannel {
                    channel_id,
                    edit: Box::new(ChannelEdit {
                        name: Some(name),
                        ..ChannelEdit::default()
                    }),
                    label,
                })
            }
        }
    }

    pub fn confirm_channel_delete(&mut self) -> Option<AppCommand> {
        let state = self.popups.take_channel_delete()?;
        Some(AppCommand::DeleteChannel {
            channel_id: state.channel_id,
            label: state.name,
        })
    }
}
