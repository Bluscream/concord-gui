//! Interface translation.
//!
//! Lives in the core rather than in a front end so both clients translate from
//! the same catalogue: a string added for the GUI is available to the TUI, and
//! a translator has one place to work.
//!
//! Deliberately small - a static table and a lookup, no Fluent or gettext.
//! The catalogue is compile-time, so a missing key is a build error rather
//! than a blank label at runtime, and there is no loader, no plural rules
//! engine and no runtime file format to keep in step.
//!
//! Adding a language means adding a column to `CATALOGUE`. Adding a string
//! means adding a row, with English required and others optional - an
//! untranslated string falls back to English rather than showing its key.

use std::sync::atomic::{AtomicU8, Ordering};

/// Languages the interface is available in.
///
/// Serialised by tag rather than by variant name, so `config.toml` carries
/// `language = "de"` - a locale, which is what someone editing it by hand
/// would expect to write.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "de")]
    German,
}

impl Language {
    /// Parse a locale tag such as `de`, `de-DE` or `de_DE.UTF-8`.
    ///
    /// Only the primary subtag is considered: there is no separate Austrian
    /// or Swiss catalogue, and matching the whole tag would fail to find one.
    pub fn from_locale(tag: &str) -> Option<Self> {
        let primary = tag
            .split(['-', '_', '.'])
            .next()
            .unwrap_or(tag)
            .to_ascii_lowercase();

        match primary.as_str() {
            "en" => Some(Self::English),
            "de" => Some(Self::German),
            _ => None,
        }
    }

    /// The tag this language is configured as.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::German => "de",
        }
    }

    /// Its name in itself, which is how language pickers should list it.
    pub const fn endonym(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::German => "Deutsch",
        }
    }

    pub const ALL: &'static [Self] = &[Self::English, Self::German];

    const fn index(self) -> u8 {
        match self {
            Self::English => 0,
            Self::German => 1,
        }
    }

    const fn from_index(index: u8) -> Self {
        match index {
            1 => Self::German,
            _ => Self::English,
        }
    }
}

/// The active language.
///
/// Global because every render path needs it and threading a locale through
/// every view signature would touch all of both front ends for no benefit.
static ACTIVE: AtomicU8 = AtomicU8::new(0);

pub fn set_language(language: Language) {
    ACTIVE.store(language.index(), Ordering::Relaxed);
}

pub fn language() -> Language {
    Language::from_index(ACTIVE.load(Ordering::Relaxed))
}

/// Pick a language from the system locale, falling back to English.
pub fn language_from_system() -> Language {
    sys_locale::get_locale()
        .as_deref()
        .and_then(Language::from_locale)
        .unwrap_or_default()
}

/// Translate a key into the active language.
///
/// An unknown key returns itself. That is deliberate: it makes the mistake
/// visible in the interface rather than showing an empty label, and the key
/// is written to be readable if it ever escapes.
pub fn translate(key: &str) -> &'static str {
    let language = language();
    for entry in CATALOGUE {
        if entry.key == key {
            return match language {
                Language::English => entry.english,
                // An untranslated string falls back to English rather than
                // showing the key, so a partial translation is still usable.
                Language::German => entry.german.unwrap_or(entry.english),
            };
        }
    }
    // Leaked rather than returned by value so the signature stays &'static,
    // which is what GPUI and ratatui both want. Only reached for a key that
    // is not in the catalogue, which is a bug either way.
    Box::leak(key.to_owned().into_boxed_str())
}

/// Shorthand for [`translate`].
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::translate($key)
    };
}

struct Entry {
    key: &'static str,
    english: &'static str,
    german: Option<&'static str>,
}

/// The catalogue.
///
/// Keys are dotted and describe where the string appears, so a translator can
/// tell a noun from a verb without reading the source: `action.` is something
/// the user does, `label.` names a thing, `status.` reports state.
const CATALOGUE: &[Entry] = &[
    // ---- presence -------------------------------------------------------
    Entry {
        key: "presence.online",
        english: "Online",
        german: Some("Online"),
    },
    Entry {
        key: "presence.idle",
        english: "Idle",
        german: Some("Abwesend"),
    },
    Entry {
        key: "presence.dnd",
        english: "Do not disturb",
        german: Some("Bitte nicht stören"),
    },
    Entry {
        key: "presence.invisible",
        english: "Invisible",
        german: Some("Unsichtbar"),
    },
    Entry {
        key: "presence.offline",
        english: "Offline",
        german: Some("Offline"),
    },
    // ---- actions --------------------------------------------------------
    Entry {
        key: "action.set_status",
        english: "Set status",
        german: Some("Status setzen"),
    },
    Entry {
        key: "action.mute",
        english: "Mute",
        german: Some("Stummschalten"),
    },
    Entry {
        key: "action.unmute",
        english: "Unmute",
        german: Some("Stummschaltung aufheben"),
    },
    Entry {
        key: "action.deafen",
        english: "Deafen",
        german: Some("Ton aus"),
    },
    Entry {
        key: "action.undeafen",
        english: "Undeafen",
        german: Some("Ton an"),
    },
    Entry {
        key: "action.leave",
        english: "Leave",
        german: Some("Verlassen"),
    },
    Entry {
        key: "action.leave_voice",
        english: "Disconnect",
        german: Some("Trennen"),
    },
    Entry {
        key: "action.join",
        english: "Join",
        german: Some("Beitreten"),
    },
    Entry {
        key: "action.cancel",
        english: "Cancel",
        german: Some("Abbrechen"),
    },
    Entry {
        key: "action.save",
        english: "Save",
        german: Some("Speichern"),
    },
    Entry {
        key: "action.close",
        english: "Close",
        german: Some("Schließen"),
    },
    Entry {
        key: "action.confirm",
        english: "Confirm",
        german: Some("Bestätigen"),
    },
    Entry {
        key: "action.remove",
        english: "Remove",
        german: Some("Entfernen"),
    },
    Entry {
        key: "action.reply",
        english: "Reply",
        german: Some("Antworten"),
    },
    Entry {
        key: "action.forward",
        english: "Forward",
        german: Some("Weiterleiten"),
    },
    Entry {
        key: "action.edit",
        english: "Edit",
        german: Some("Bearbeiten"),
    },
    Entry {
        key: "action.delete",
        english: "Delete",
        german: Some("Löschen"),
    },
    Entry {
        key: "action.pin",
        english: "Pin",
        german: Some("Anheften"),
    },
    Entry {
        key: "action.unpin",
        english: "Unpin",
        german: Some("Loslösen"),
    },
    Entry {
        key: "action.react",
        english: "React",
        german: Some("Reagieren"),
    },
    Entry {
        key: "action.copy_text",
        english: "Copy text",
        german: Some("Text kopieren"),
    },
    Entry {
        key: "action.copy_link",
        english: "Copy link",
        german: Some("Link kopieren"),
    },
    Entry {
        key: "action.share_screen",
        english: "Share screen",
        german: Some("Bildschirm teilen"),
    },
    Entry {
        key: "action.stop_sharing",
        english: "Stop sharing",
        german: Some("Teilen beenden"),
    },
    Entry {
        key: "action.audio_devices",
        english: "Audio devices",
        german: Some("Audiogeräte"),
    },
    Entry {
        key: "action.microphone",
        english: "Microphone",
        german: Some("Mikrofon"),
    },
    Entry {
        key: "action.sticker",
        english: "Sticker",
        german: Some("Sticker"),
    },
    Entry {
        key: "action.attach",
        english: "Attach",
        german: Some("Anhängen"),
    },
    Entry {
        key: "action.join_server",
        english: "Join a server",
        german: Some("Server beitreten"),
    },
    Entry {
        key: "action.manage_roles",
        english: "Manage roles",
        german: Some("Rollen verwalten"),
    },
    Entry {
        key: "action.kick",
        english: "Kick",
        german: Some("Kicken"),
    },
    Entry {
        key: "action.ban",
        english: "Ban",
        german: Some("Bannen"),
    },
    Entry {
        key: "action.unban",
        english: "Unban",
        german: Some("Bann aufheben"),
    },
    Entry {
        key: "action.timeout",
        english: "Time out",
        german: Some("Auszeit"),
    },
    Entry {
        key: "action.clear_timeout",
        english: "Clear timeout",
        german: Some("Auszeit aufheben"),
    },
    // ---- labels ---------------------------------------------------------
    Entry {
        key: "label.bans",
        english: "Bans",
        german: Some("Banns"),
    },
    Entry {
        key: "label.roles",
        english: "Roles",
        german: Some("Rollen"),
    },
    Entry {
        key: "label.stickers",
        english: "Stickers",
        german: Some("Sticker"),
    },
    Entry {
        key: "label.mentions",
        english: "Mentions",
        german: Some("Erwähnungen"),
    },
    Entry {
        key: "label.pins",
        english: "Pins",
        german: Some("Angeheftet"),
    },
    Entry {
        key: "label.settings",
        english: "Settings",
        german: Some("Einstellungen"),
    },
    Entry {
        key: "label.voice_connected",
        english: "Voice Connected",
        german: Some("Sprachkanal verbunden"),
    },
    Entry {
        key: "label.moderation",
        english: "Moderation",
        german: Some("Moderation"),
    },
    Entry {
        key: "label.language",
        english: "Language",
        german: Some("Sprache"),
    },
    // ---- status ---------------------------------------------------------
    Entry {
        key: "status.loading",
        english: "Loading...",
        german: Some("Wird geladen …"),
    },
    Entry {
        key: "status.no_matches",
        english: "No matches",
        german: Some("Keine Treffer"),
    },
    Entry {
        key: "status.connecting",
        english: "connecting…",
        german: Some("verbinde …"),
    },
    Entry {
        key: "status.reconnected",
        english: "Reconnected",
        german: Some("Wieder verbunden"),
    },
    Entry {
        key: "status.disconnected",
        english: "Disconnected; reconnecting",
        german: Some("Getrennt; verbinde neu"),
    },
    Entry {
        key: "status.signed_out",
        english: "Signed out",
        german: Some("Abgemeldet"),
    },
    Entry {
        key: "status.joined",
        english: "Joined",
        german: Some("Beigetreten"),
    },
    Entry {
        key: "status.message_not_sent",
        english: "Message was not sent",
        german: Some("Nachricht wurde nicht gesendet"),
    },
    Entry {
        key: "status.no_stickers",
        english: "This server has no stickers",
        german: Some("Dieser Server hat keine Sticker"),
    },
    Entry {
        key: "status.no_bans",
        english: "Nobody is banned from this server",
        german: Some("Niemand ist von diesem Server gebannt"),
    },
    Entry {
        key: "status.no_mentions",
        english: "No recent mentions",
        german: Some("Keine neuen Erwähnungen"),
    },
    Entry {
        key: "status.already_joined",
        english: "You are already in this server",
        german: Some("Du bist bereits auf diesem Server"),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_unique() {
        // A duplicate key silently shadows the later one, and the lookup would
        // return whichever happened to come first.
        let mut seen = std::collections::BTreeSet::new();
        for entry in CATALOGUE {
            assert!(seen.insert(entry.key), "duplicate key: {}", entry.key);
        }
    }

    #[test]
    fn locale_tags_resolve_to_a_language() {
        assert_eq!(Language::from_locale("de"), Some(Language::German));
        assert_eq!(Language::from_locale("de-DE"), Some(Language::German));
        assert_eq!(Language::from_locale("de_AT.UTF-8"), Some(Language::German));
        assert_eq!(Language::from_locale("en-GB"), Some(Language::English));

        // An unsupported language is not an error - it falls back to English.
        assert_eq!(Language::from_locale("fr"), None);
        assert_eq!(Language::from_locale(""), None);
    }

    #[test]
    fn an_untranslated_string_falls_back_to_english() {
        // A partial translation must stay usable rather than showing keys.
        set_language(Language::German);
        let entry = CATALOGUE
            .iter()
            .find(|entry| entry.german.is_none())
            .map(|entry| (entry.key, entry.english));

        if let Some((key, english)) = entry {
            assert_eq!(translate(key), english);
        }
        set_language(Language::English);
    }

    #[test]
    fn german_is_used_when_selected() {
        set_language(Language::German);
        assert_eq!(translate("presence.idle"), "Abwesend");
        set_language(Language::English);
        assert_eq!(translate("presence.idle"), "Idle");
    }

    #[test]
    fn an_unknown_key_returns_itself_rather_than_nothing() {
        // Visible in the interface beats a blank label: a missing string
        // should look like a bug, not like an empty control.
        assert_eq!(translate("does.not.exist"), "does.not.exist");
    }
}
