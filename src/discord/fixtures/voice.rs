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

/// Every voice room that already has somebody in it.
///
/// The fixture seats people in a room before anybody joins it, so without
/// this the room only starts moving once the person watching joins one -
/// which is the one moment they are not looking at it from the outside.
pub fn occupied_scopes(state: &DiscordState) -> Vec<VoiceScope> {
    state.voice.fixture_occupied_scopes()
}

/// Who is currently speaking in a room.
pub fn speaking(state: &DiscordState, scope: VoiceScope) -> Vec<Id<marker::UserMarker>> {
    state.voice.fixture_speaking(scope)
}

/// How often the fake moves a voice room on.
///
/// Slow enough to read - a room where everyone flickers is noise, not a
/// demonstration - and fast enough that a glance at the panel catches it.
pub const TICK: std::time::Duration = std::time::Duration::from_millis(1400);

/// Move a voice room on by one step.
///
/// Nobody is really talking, so who appears to be is derived from the clock:
/// a rotating index over the people in the room, which gives one speaker at a
/// time and a predictable order rather than something that has to be seeded
/// and carried between ticks.
///
/// The account's own state is left alone. It is the one participant whose
/// microphone and camera the person watching actually controls, and moving it
/// under them would fight their own clicks.
pub fn tick(state: &mut DiscordState, scope: VoiceScope, now: std::time::Instant) {
    let mut people = participants(state, scope);
    if people.is_empty() {
        return;
    }
    people.sort_by_key(|user| user.get());

    let own = super::demo_user_id();

    // Measured from a fixed point rather than from `now`: `now.elapsed()` is
    // the time since that instant, which is nearly zero, so the index never
    // moved and the same person talked for the whole session.
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let start = *START.get_or_init(std::time::Instant::now);
    let step = now.saturating_duration_since(start).as_millis() / TICK.as_millis();

    let talker = people[(step as usize) % people.len()];

    for user in people {
        if user == own {
            continue;
        }
        set_speaking(state, scope, user, user == talker);
    }
}
