//! Stages, and the parts of a voice room a fake can move.
//!
//! There is no audio here and there never will be: a demo that opened a
//! capture device would be doing the one thing a demo must not do. What it
//! can do is move every piece of state a voice room shows - who is in it, who
//! is speaking, who is muted, who has a camera or a screen share on - because
//! that is what the panel draws, and none of it needs a sound card.
//!
//! A stage is the same picture with one rule laid over it: speakers are
//! unmuted and the audience is not. Promoting somebody to speaker is
//! therefore unmuting them, which is exactly what it looks like on Discord.

use std::sync::Arc;

use crate::discord::{DiscordState, Id, StageInstance, VoiceScope, marker};

/// Begin a stage in a channel.
pub fn start_stage(id: u64, channel: Id<marker::ChannelMarker>, topic: &str) -> StageInstance {
    StageInstance {
        id,
        channel_id: channel,
        topic: topic.to_string(),
    }
}

/// Move somebody between the audience and the speakers.
///
/// Modelled as mute, because that is what the distinction is on screen: a
/// speaker is unmuted and everybody else is not. There is no separate
/// "suppressed" flag in the state to set, and inventing one would be a field
/// no view reads.
pub fn set_speaker(
    state: &mut DiscordState,
    scope: VoiceScope,
    user: Id<marker::UserMarker>,
    speaking: bool,
) {
    let voice = Arc::make_mut(&mut state.voice);
    voice.set_fixture_self_mute(scope, user, !speaking);
}

/// Set whether somebody's microphone is picking anything up.
///
/// The one piece of a voice room that changes on its own while nobody
/// touches anything, so a demo that never moves it looks frozen.
pub fn set_speaking(
    state: &mut DiscordState,
    scope: VoiceScope,
    user: Id<marker::UserMarker>,
    speaking: bool,
) {
    let voice = Arc::make_mut(&mut state.voice);
    voice.set_fixture_speaking(scope, user, speaking);
}

/// Turn somebody's camera on or off.
pub fn set_camera(
    state: &mut DiscordState,
    scope: VoiceScope,
    user: Id<marker::UserMarker>,
    on: bool,
) {
    let voice = Arc::make_mut(&mut state.voice);
    voice.set_fixture_video(scope, user, on);
}

/// Turn somebody's screen share on or off.
pub fn set_stream(
    state: &mut DiscordState,
    scope: VoiceScope,
    user: Id<marker::UserMarker>,
    on: bool,
) {
    let voice = Arc::make_mut(&mut state.voice);
    voice.set_fixture_stream(scope, user, on);
}

/// Everybody currently in a voice scope.
pub fn participants(state: &DiscordState, scope: VoiceScope) -> Vec<Id<marker::UserMarker>> {
    state.voice.fixture_participants(scope)
}
