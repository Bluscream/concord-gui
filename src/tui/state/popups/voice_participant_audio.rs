use super::*;

pub(super) const VOICE_PARTICIPANT_AUDIO_FIELD_COUNT: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum VoiceParticipantAudioField {
    Volume,
    Muted,
    /// Whether to stop showing their camera and screen share. Separate from
    /// mute: not wanting to see a face is not the same as not wanting to hear
    /// a voice, and one control for both would make either impossible alone.
    VideoHidden,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct VoiceParticipantAudioPopupView {
    pub display_name: String,
    pub selected: VoiceParticipantAudioField,
    pub settings: VoiceParticipantPlaybackSettings,
}

#[derive(Debug)]
pub(in crate::tui::state) struct VoiceParticipantAudioPopupState {
    user_id: Id<UserMarker>,
    display_name: String,
    pub(super) selection: SelectablePopupState,
}

impl VoiceParticipantAudioPopupState {
    fn selected_field(&self) -> VoiceParticipantAudioField {
        match self
            .selection
            .selected_for_len(VOICE_PARTICIPANT_AUDIO_FIELD_COUNT)
        {
            0 => VoiceParticipantAudioField::Volume,
            1 => VoiceParticipantAudioField::Muted,
            _ => VoiceParticipantAudioField::VideoHidden,
        }
    }
}

impl DashboardState {
    pub(in crate::tui::state) fn open_voice_participant_audio_popup(
        &mut self,
        user_id: Id<UserMarker>,
        display_name: String,
    ) {
        self.popups.set_modal(ModalPopup::VoiceParticipantAudio(
            VoiceParticipantAudioPopupState {
                user_id,
                display_name,
                selection: SelectablePopupState::default(),
            },
        ));
    }

    pub(in crate::tui) fn close_voice_participant_audio_popup(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::VoiceParticipantAudio) {
            self.popups.clear_modal();
        }
    }

    pub(in crate::tui) fn voice_participant_audio_popup_view(
        &self,
    ) -> Option<VoiceParticipantAudioPopupView> {
        let popup = self.popups.voice_participant_audio()?;
        Some(VoiceParticipantAudioPopupView {
            display_name: popup.display_name.clone(),
            selected: popup.selected_field(),
            settings: self.voice_participant_playback_settings(popup.user_id),
        })
    }

    pub(in crate::tui) fn move_voice_participant_audio_selection(
        &mut self,
        action: SelectionAction,
    ) {
        self.move_selectable_popup(SelectablePopupTarget::VoiceParticipantAudio, action);
    }

    pub(in crate::tui) fn adjust_voice_participant_audio_volume(
        &mut self,
        delta: i8,
    ) -> Option<AppCommand> {
        let popup = self.popups.voice_participant_audio()?;
        if popup.selected_field() != VoiceParticipantAudioField::Volume {
            return None;
        }
        let user_id = popup.user_id;
        let mut settings = self.voice_participant_playback_settings(user_id);
        let adjusted_volume = settings.volume.adjust(i16::from(delta));
        if adjusted_volume == settings.volume {
            return None;
        }
        settings.volume = adjusted_volume;
        Some(self.update_voice_participant_playback_settings(user_id, settings))
    }

    pub(in crate::tui) fn activate_voice_participant_audio_field(&mut self) -> Option<AppCommand> {
        let popup = self.popups.voice_participant_audio()?;
        let field = popup.selected_field();
        let user_id = popup.user_id;
        let mut settings = self.voice_participant_playback_settings(user_id);
        match field {
            VoiceParticipantAudioField::Muted => settings.muted = !settings.muted,
            VoiceParticipantAudioField::VideoHidden => {
                settings.video_hidden = !settings.video_hidden;
            }
            // Volume is adjusted rather than toggled, and has its own keys.
            VoiceParticipantAudioField::Volume => return None,
        }
        Some(self.update_voice_participant_playback_settings(user_id, settings))
    }
}
