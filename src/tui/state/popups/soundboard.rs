//! The soundboard picker.
//!
//! Holds both lists at once - the guild's sounds and the defaults every account
//! has - because a guild that has added nothing should still get a usable
//! picker rather than an empty one.

use crate::discord::ids::{Id, marker::GuildMarker};
use crate::discord::{AppCommand, SoundboardSound};

use super::super::DashboardState;
use super::{ActiveModalPopupKind, ModalPopup, SelectablePopupState, SelectablePopupTarget};

#[derive(Clone, Debug, PartialEq)]
pub(in crate::tui) struct SoundboardState {
    pub(super) selection: SelectablePopupState,
    pub(super) guild_sounds: Vec<SoundboardSound>,
    pub(super) default_sounds: Vec<SoundboardSound>,
    /// Set while either fetch is outstanding, so the popup can say so rather
    /// than looking like a guild with no sounds.
    pub(super) loading: bool,
    pub(super) error: Option<String>,
}

impl SoundboardState {
    /// Every sound the picker shows, the guild's first.
    ///
    /// The guild's own come first because they are what somebody opened the
    /// picker to reach; the defaults are the fallback.
    pub(in crate::tui) fn sounds(&self) -> impl Iterator<Item = &SoundboardSound> {
        self.guild_sounds.iter().chain(self.default_sounds.iter())
    }

    pub(in crate::tui) fn len(&self) -> usize {
        self.guild_sounds.len() + self.default_sounds.len()
    }

    pub(in crate::tui) fn is_loading(&self) -> bool {
        self.loading
    }

    pub(in crate::tui) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

impl DashboardState {
    /// Open the picker, fetching both lists.
    pub fn open_soundboard(&mut self) -> Vec<AppCommand> {
        let guild_id = match self.navigation.guilds.active {
            crate::tui::state::ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            crate::tui::state::ActiveGuildScope::DirectMessages
            | crate::tui::state::ActiveGuildScope::Unset => None,
        };

        self.popups
            .set_modal(ModalPopup::Soundboard(SoundboardState {
                selection: SelectablePopupState::default(),
                guild_sounds: Vec::new(),
                default_sounds: Vec::new(),
                loading: true,
                error: None,
            }));

        let mut commands = Vec::new();
        if let Some(guild_id) = guild_id {
            commands.push(AppCommand::LoadSoundboardSounds {
                guild_id: Some(guild_id),
            });
        }
        commands.push(AppCommand::LoadSoundboardSounds { guild_id: None });
        commands
    }

    pub fn close_soundboard(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::Soundboard) {
            self.popups.clear_modal();
        }
    }

    pub(in crate::tui) fn soundboard_state(&self) -> Option<&SoundboardState> {
        self.popups.soundboard()
    }

    pub fn move_soundboard_selection_down(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::Soundboard,
            crate::tui::keybindings::SelectionAction::Next,
        );
    }

    pub fn move_soundboard_selection_up(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::Soundboard,
            crate::tui::keybindings::SelectionAction::Previous,
        );
    }

    pub(in crate::tui) fn selected_sound_index(&self) -> Option<usize> {
        self.popups
            .soundboard()
            .map(|state| state.selection.selected_for_len(state.len()))
    }

    /// Play the highlighted sound into the voice channel we are in.
    pub fn play_selected_sound(&mut self) -> Option<AppCommand> {
        let index = self.selected_sound_index()?;
        // Only while in a voice channel: a sound has nowhere to go otherwise,
        // and Discord refuses it.
        let channel_id = self
            .runtime
            .voice_connection
            .and_then(|voice| voice.channel_id)?;
        let state = self.popups.soundboard()?;
        let sound = state.sounds().nth(index)?;

        // Refused rather than sent: Discord rejects an unavailable sound, and
        // the popup already shows why it is greyed.
        if !sound.available {
            return None;
        }

        Some(AppCommand::PlaySoundboardSound {
            channel_id,
            sound_id: sound.sound_id,
            source_guild_id: sound.guild_id,
            label: sound.name.clone(),
        })
    }

    pub(in crate::tui) fn apply_soundboard_sounds(
        &mut self,
        guild_id: Option<Id<GuildMarker>>,
        sounds: Vec<SoundboardSound>,
    ) {
        let Some(state) = self.popups.soundboard_mut() else {
            return;
        };
        state.loading = false;
        state.error = None;
        // The two lists arrive as separate replies, told apart by whether they
        // name a guild.
        match guild_id {
            Some(_) => state.guild_sounds = sounds,
            None => state.default_sounds = sounds,
        }
    }

    pub(in crate::tui) fn apply_soundboard_failure(&mut self, message: String) {
        if let Some(state) = self.popups.soundboard_mut() {
            state.loading = false;
            state.error = Some(message);
        }
    }
}
