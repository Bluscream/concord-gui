//! Desktop notifications.
//!
//! Eligibility is decided by the core, not here:
//! `message_event_triggers_notification` already applies guild and channel
//! mutes, notification levels, mention rules and suppression flags. Duplicating
//! any of that in the front-end would drift from the TUI's behaviour and,
//! worse, could notify for a channel the user muted.
//!
//! Delivery is best-effort. A missing or refusing notification daemon must
//! never interrupt the client, so every failure is logged and swallowed.

use concord::discord::{AppEvent, DiscordState};

/// Notification content, already filtered for eligibility.
pub struct Notification {
    pub title: String,
    pub body: String,
}

/// Decide whether an event warrants a desktop notification.
///
/// `focused_channel` suppresses notifications for the channel the user is
/// currently reading - the message is already on screen.
pub fn notification_for(
    state: &DiscordState,
    event: &AppEvent,
    focused_channel: Option<concord::discord::Id<concord::discord::marker::ChannelMarker>>,
    window_focused: bool,
) -> Option<Notification> {
    if !state.message_event_triggers_notification(event) {
        return None;
    }

    let AppEvent::MessageCreate { message } = event else {
        return None;
    };

    if window_focused && focused_channel == Some(message.channel_id) {
        return None;
    }

    // Channel names give the notification somewhere to point; without one the
    // author alone is still useful, so a missing name is not disqualifying.
    let channel = state
        .channel(message.channel_id)
        .map(|channel| {
            if channel.is_dm_or_group_dm() {
                String::new()
            } else {
                format!(" in #{}", channel.name)
            }
        })
        .unwrap_or_default();

    let body = message
        .content
        .clone()
        .filter(|content| !content.is_empty())
        .unwrap_or_else(|| "sent an attachment".to_string());

    Some(Notification {
        title: format!("{}{channel}", message.author),
        // Long messages are truncated: a notification is a pointer, not a
        // reader, and some daemons reject oversized bodies outright.
        body: body.chars().take(180).collect(),
    })
}

/// Deliver a notification, swallowing any backend failure.
/// Play the notification sound alongside the desktop notification.
///
/// Deliberately driven by the same eligibility decision as [`notification_for`]
/// rather than a rule of its own: a sound for a channel the user muted is
/// worse than no sound, and the core already knows which those are.
///
/// Best-effort like delivery - a machine with no sound device must not stall
/// the client, so playback happens off the UI thread and failures are dropped.
pub fn play_sound(custom_path: Option<std::path::PathBuf>) {
    std::thread::spawn(move || {
        #[cfg(feature = "media")]
        {
            let _ = concord::sound::play_notification_sound(custom_path.as_deref());
        }
        #[cfg(not(feature = "media"))]
        {
            // Without the playback feature there is no sound to make, and a
            // terminal bell would go to a terminal the user is not looking at.
            let _ = custom_path;
        }
    });
}

pub fn deliver(notification: &Notification) {
    if let Err(error) = notify_rust::Notification::new()
        .summary(&notification.title)
        .body(&notification.body)
        .appname("concord")
        .show()
    {
        tracing::debug!("desktop notification failed: {error}");
    }
}
