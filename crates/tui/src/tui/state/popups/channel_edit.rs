//! Creating channels, and editing their settings.
//!
//! One popup for both. Creating shows a name and a kind; editing an existing
//! channel shows the settings that kind actually has, which is why the field
//! list is built per channel rather than being fixed. Deleting gets a
//! confirmation of its own because it takes the channel's whole history with
//! it and Discord offers no undo.

use crate::tui::text_input::{TextEditAction, TextInputState};
use concord::discord::ids::{Id, marker::ChannelMarker};
use concord::discord::{AppCommand, ChannelEdit, NewChannelKind};

use super::super::DashboardState;
use super::{ActiveModalPopupKind, ModalPopup};

/// What the channel-edit popup is for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChannelEditPurpose {
    /// A new channel, with the kind being picked alongside the name.
    Create { kind: NewChannelKind },
    /// An existing channel's settings.
    Edit { channel_id: Id<ChannelMarker> },
}

/// One editable setting.
///
/// Which of these a channel shows depends on its kind: a category has no
/// topic, a text channel has no bitrate, and offering a field that does
/// nothing is worse than leaving it out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelField {
    Name,
    Topic,
    Slowmode,
    Nsfw,
    UserLimit,
    /// The short line beside a voice channel's name - what is happening in it
    /// right now. Not the channel topic: this one is transient, and Discord
    /// clears it when the channel empties.
    VoiceStatus,
    /// A stage's topic: what the live session is about.
    ///
    /// Not the channel's topic. A stage channel has both - the channel's
    /// describes the room, this describes the session running in it - and
    /// conflating them would overwrite one with the other.
    StageTopic,
}

impl ChannelField {
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Topic => "Topic",
            Self::Slowmode => "Slowmode (seconds)",
            Self::Nsfw => "Age-restricted",
            Self::UserLimit => "User limit (0 = none)",
            // Named for what it is, so it is not mistaken for the channel
            // topic two rows above it.
            Self::StageTopic => "Stage topic (empty ends the stage)",
            Self::VoiceStatus => "Voice status (empty clears it)",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct ChannelEditState {
    pub(super) purpose: ChannelEditPurpose,
    pub(super) name: TextInputState,
    pub(super) topic: TextInputState,
    /// The live stage's topic, for a stage channel. Separate from the channel
    /// topic above, which describes the room rather than the session.
    pub(super) stage_topic: TextInputState,
    pub(super) voice_status: TextInputState,
    /// Whether a stage is already running here.
    ///
    /// Decides between starting and changing: Discord's start endpoint fails
    /// on a running stage, and its patch fails on one that is not.
    pub(super) stage_running: bool,
    pub(super) slowmode: TextInputState,
    pub(super) user_limit: TextInputState,
    pub(super) nsfw: bool,
    /// Which fields this channel actually has, in the order shown.
    pub(super) fields: Vec<ChannelField>,
    pub(super) focused: usize,
}

impl ChannelEditState {
    pub(in crate::tui) fn purpose(&self) -> &ChannelEditPurpose {
        &self.purpose
    }

    pub(in crate::tui) fn fields(&self) -> &[ChannelField] {
        &self.fields
    }

    pub(in crate::tui) fn focused(&self) -> usize {
        self.focused
    }

    /// What is currently in a field, for display.
    pub(in crate::tui) fn value(&self, field: ChannelField) -> String {
        match field {
            ChannelField::Name => self.name.value().to_owned(),
            ChannelField::Topic => self.topic.value().to_owned(),
            ChannelField::Slowmode => self.slowmode.value().to_owned(),
            ChannelField::UserLimit => self.user_limit.value().to_owned(),
            ChannelField::StageTopic => self.stage_topic.value().to_owned(),
            ChannelField::VoiceStatus => self.voice_status.value().to_owned(),
            ChannelField::Nsfw => if self.nsfw { "yes" } else { "no" }.to_owned(),
        }
    }

    fn input_mut(&mut self, field: ChannelField) -> Option<&mut TextInputState> {
        match field {
            ChannelField::Name => Some(&mut self.name),
            ChannelField::Topic => Some(&mut self.topic),
            ChannelField::Slowmode => Some(&mut self.slowmode),
            ChannelField::UserLimit => Some(&mut self.user_limit),
            ChannelField::StageTopic => Some(&mut self.stage_topic),
            ChannelField::VoiceStatus => Some(&mut self.voice_status),
            ChannelField::Nsfw => None,
        }
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
                topic: TextInputState::default(),
                stage_topic: TextInputState::default(),
                voice_status: TextInputState::default(),
                stage_running: false,
                slowmode: TextInputState::default(),
                user_limit: TextInputState::default(),
                nsfw: false,
                // Creating asks only for a name; the rest is easier to set
                // once the channel exists and its kind is settled.
                fields: vec![ChannelField::Name],
                focused: 0,
            }));
    }

    /// Open a channel's settings, seeded with what it currently is.
    pub(in crate::tui) fn open_channel_settings(&mut self, channel_id: Id<ChannelMarker>) {
        let Some(channel) = self.discord.channel(channel_id) else {
            return;
        };

        let mut name = TextInputState::default();
        name.set_value(channel.name.clone());
        let mut topic = TextInputState::default();
        topic.set_value(channel.topic.clone().unwrap_or_default());
        let mut slowmode = TextInputState::default();
        slowmode.set_value(channel.rate_limit_per_user.unwrap_or(0).to_string());
        let mut user_limit = TextInputState::default();
        user_limit.set_value(channel.user_limit.unwrap_or(0).to_string());

        // Only the fields this kind actually has. Offering a bitrate on a text
        // channel, or a topic on a category, would be a control that does
        // nothing.
        let mut fields = vec![ChannelField::Name];
        if channel.is_voice() {
            fields.push(ChannelField::UserLimit);
            // Only a voice channel has one, so it appears here rather than
            // being an always-present control that does nothing elsewhere.
            fields.push(ChannelField::VoiceStatus);
            if channel.is_stage() {
                fields.push(ChannelField::StageTopic);
            }
        } else if !channel.is_category() {
            fields.push(ChannelField::Topic);
            fields.push(ChannelField::Slowmode);
            fields.push(ChannelField::Nsfw);
        }

        let nsfw = channel.nsfw.unwrap_or(false);
        self.popups
            .set_modal(ModalPopup::ChannelEdit(ChannelEditState {
                purpose: ChannelEditPurpose::Edit { channel_id },
                name,
                topic,
                stage_topic: TextInputState::default(),
                voice_status: TextInputState::default(),
                stage_running: false,
                slowmode,
                user_limit,
                nsfw,
                fields,
                focused: 0,
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

    /// Take the stage that is running here, if any.
    ///
    /// Seeds the topic field so a change is a correction rather than a
    /// retype, and decides which endpoint the form will use.
    pub(in crate::tui) fn set_stage_instance(
        &mut self,
        instance: Option<concord::discord::StageInstance>,
    ) {
        if let Some(state) = self.popups.channel_edit_mut() {
            match instance {
                Some(instance) => {
                    state.stage_topic.set_value(instance.topic);
                    state.stage_running = true;
                }
                None => state.stage_running = false,
            }
        }
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
        let Some(state) = self.popups.channel_edit_mut() else {
            return;
        };
        let Some(field) = state.fields.get(state.focused).copied() else {
            return;
        };
        if let Some(input) = state.input_mut(field) {
            input.apply_edit_action(action);
        }
    }

    pub fn insert_channel_name_char(&mut self, value: char) {
        let Some(state) = self.popups.channel_edit_mut() else {
            return;
        };
        let Some(field) = state.fields.get(state.focused).copied() else {
            return;
        };
        match state.input_mut(field) {
            Some(input) => input.insert_char(value),
            // A toggle has nothing to type into; space flips it, which is the
            // convention every other checkbox in this client uses.
            None if value == ' ' => state.nsfw = !state.nsfw,
            None => {}
        }
    }

    /// Move between the settings this channel has.
    pub fn cycle_channel_field(&mut self, forward: bool) {
        let Some(state) = self.popups.channel_edit_mut() else {
            return;
        };
        let count = state.fields.len();
        if count < 2 {
            return;
        }
        state.focused = if forward {
            (state.focused + 1) % count
        } else {
            (state.focused + count - 1) % count
        };
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
        let fields = state.fields.clone();
        let values: Vec<String> = fields.iter().map(|field| state.value(*field)).collect();
        let nsfw = state.nsfw;
        // An empty name is a cancel rather than a channel called nothing,
        // which Discord would reject anyway.
        if name.is_empty() {
            return None;
        }

        let purpose = state.purpose.clone();
        // Taken before the popup closes. A stage topic is its own endpoint, so
        // it is queued rather than folded into the channel edit.
        let stage_topic = fields
            .iter()
            .position(|field| *field == ChannelField::StageTopic)
            .and_then(|index| values.get(index).cloned());
        // Read before the popup closes: which endpoint to use depends on it.
        let running = state.stage_running;
        let voice_status = fields
            .iter()
            .position(|field| *field == ChannelField::VoiceStatus)
            .and_then(|index| values.get(index).cloned());
        self.close_channel_edit();

        if let (Some(status), ChannelEditPurpose::Edit { channel_id }) = (voice_status, &purpose) {
            let status = status.trim().to_owned();
            self.enqueue_pending_command(AppCommand::SetVoiceChannelStatus {
                channel_id: *channel_id,
                // Empty clears it, which is a real thing to want and distinct
                // from leaving the status alone.
                status: (!status.is_empty()).then_some(status),
            });
        }

        if let (Some(topic), ChannelEditPurpose::Edit { channel_id }) = (stage_topic, &purpose) {
            let topic = topic.trim().to_owned();
            // The rule is in the core, so both clients decide it alike.
            let command = match concord::discord::stage_action_for(&topic, running) {
                concord::discord::StageAction::End => AppCommand::EndStageInstance {
                    channel_id: *channel_id,
                    label: name.clone(),
                },
                concord::discord::StageAction::ChangeTopic => AppCommand::ModifyStageTopic {
                    channel_id: *channel_id,
                    topic,
                },
                concord::discord::StageAction::Start => AppCommand::StartStageInstance {
                    channel_id: *channel_id,
                    topic,
                },
            };
            self.enqueue_pending_command(command);
        }

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
            ChannelEditPurpose::Edit { channel_id } => {
                let label = self
                    .discord
                    .channel(channel_id)
                    .map(|channel| channel.name.clone())
                    .unwrap_or_else(|| name.clone());
                let edit = self.build_channel_edit(channel_id, name, &fields, &values, nsfw);

                // Nothing changed, so nothing is sent: it would spend a
                // request and write an audit entry saying so.
                if edit.is_empty() {
                    return None;
                }
                Some(AppCommand::ModifyChannel {
                    channel_id,
                    edit: Box::new(edit),
                    label,
                })
            }
        }
    }

    /// Build the edit from the form, keeping only what actually changed.
    ///
    /// Comparing against the cached channel rather than sending everything is
    /// what stops an edit overwriting a setting somebody else changed while
    /// the form was open.
    fn build_channel_edit(
        &self,
        channel_id: Id<ChannelMarker>,
        name: String,
        fields: &[ChannelField],
        values: &[String],
        nsfw: bool,
    ) -> ChannelEdit {
        let Some(channel) = self.discord.channel(channel_id) else {
            return ChannelEdit::default();
        };

        let mut edit = ChannelEdit::default();
        if name != channel.name {
            edit.name = Some(name);
        }

        for (field, value) in fields.iter().zip(values) {
            match field {
                ChannelField::Name => {}
                ChannelField::Topic => {
                    let current = channel.topic.clone().unwrap_or_default();
                    if *value != current {
                        // An emptied topic clears it rather than leaving it,
                        // which is what the form appears to promise.
                        edit.topic = Some((!value.is_empty()).then(|| value.clone()));
                    }
                }
                ChannelField::Slowmode => {
                    if let Ok(seconds) = value.trim().parse::<u32>()
                        && u64::from(seconds) != channel.rate_limit_per_user.unwrap_or(0)
                    {
                        edit.slowmode_seconds = Some(seconds);
                    }
                }
                ChannelField::UserLimit => {
                    if let Ok(limit) = value.trim().parse::<u32>()
                        && u64::from(limit) != channel.user_limit.unwrap_or(0)
                    {
                        edit.user_limit = Some(limit);
                    }
                }
                // Neither is part of the channel edit: both are separate
                // endpoints, and are sent alongside rather than folded in.
                ChannelField::StageTopic | ChannelField::VoiceStatus => {}
                ChannelField::Nsfw => {
                    if nsfw != channel.nsfw.unwrap_or(false) {
                        edit.nsfw = Some(nsfw);
                    }
                }
            }
        }
        edit
    }

    pub fn confirm_channel_delete(&mut self) -> Option<AppCommand> {
        let state = self.popups.take_channel_delete()?;
        Some(AppCommand::DeleteChannel {
            channel_id: state.channel_id,
            label: state.name,
        })
    }
}
