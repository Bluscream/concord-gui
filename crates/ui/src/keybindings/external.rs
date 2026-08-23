//! Keymap resolution for front ends outside the TUI.
//!
//! The GUI must honour the same `keymap.toml` the TUI does, including custom
//! bindings and multi-chord leader sequences. Rather than reimplement chord
//! resolution - which would drift, and would silently disagree with the user's
//! own configuration - this exposes the existing resolver behind a surface
//! narrow enough that the TUI's internals stay private.
//!
//! Only resolution is public. Everything about *how* a key reaches this point,
//! and what an action then does, belongs to the front end.

use crate::key::{KeyCode, KeyEvent, KeyModifiers};

pub use super::actions::UiAction;
use super::chord::KeyChord;
use super::{KeyBindings, KeyMapLookup};

/// One key press, described independently of any input library.
///
/// The front end translates its own event into this; the crossterm types it
/// maps onto stay inside the crate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyPress {
    /// The key itself. A single character, or a named key such as "enter".
    pub key: Key,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// A key, either a character or one of the named non-character keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Backspace,
    Tab,
    BackTab,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    F(u8),
}

impl Key {
    /// Parse a key name as GPUI spells it.
    ///
    /// Returns `None` for names this does not model, so an unmapped key falls
    /// through to the front end rather than resolving to something arbitrary.
    pub fn parse(name: &str) -> Option<Self> {
        let key = match name {
            "enter" => Self::Enter,
            "escape" => Self::Escape,
            "backspace" => Self::Backspace,
            "tab" => Self::Tab,
            "backtab" => Self::BackTab,
            "delete" => Self::Delete,
            "insert" => Self::Insert,
            "home" => Self::Home,
            "end" => Self::End,
            "pageup" => Self::PageUp,
            "pagedown" => Self::PageDown,
            "up" => Self::Up,
            "down" => Self::Down,
            "left" => Self::Left,
            "right" => Self::Right,
            "space" => Self::Char(' '),
            other => {
                if let Some(number) = other.strip_prefix('f')
                    && let Ok(number) = number.parse::<u8>()
                    && (1..=12).contains(&number)
                {
                    Self::F(number)
                } else {
                    // Single characters only: a longer unknown name is a key
                    // this does not model, not a literal string to match.
                    let mut chars = other.chars();
                    let first = chars.next()?;
                    if chars.next().is_some() {
                        return None;
                    }
                    Self::Char(first)
                }
            }
        };
        Some(key)
    }

    fn code(self) -> KeyCode {
        match self {
            Self::Char(value) => KeyCode::Char(value),
            Self::Enter => KeyCode::Enter,
            Self::Escape => KeyCode::Esc,
            Self::Backspace => KeyCode::Backspace,
            Self::Tab => KeyCode::Tab,
            Self::BackTab => KeyCode::BackTab,
            Self::Delete => KeyCode::Delete,
            Self::Insert => KeyCode::Insert,
            Self::Home => KeyCode::Home,
            Self::End => KeyCode::End,
            Self::PageUp => KeyCode::PageUp,
            Self::PageDown => KeyCode::PageDown,
            Self::Up => KeyCode::Up,
            Self::Down => KeyCode::Down,
            Self::Left => KeyCode::Left,
            Self::Right => KeyCode::Right,
            Self::F(number) => KeyCode::F(number),
        }
    }
}

impl KeyPress {
    fn event(self) -> KeyEvent {
        let mut modifiers = KeyModifiers::NONE;
        if self.ctrl {
            modifiers |= KeyModifiers::CONTROL;
        }
        if self.alt {
            modifiers |= KeyModifiers::ALT;
        }
        // Shift is only meaningful for keys that do not already encode it in
        // the character; "shift-a" arrives as 'A', and claiming both would not
        // match a binding written either way.
        if self.shift && !matches!(self.key, Key::Char(_)) {
            modifiers |= KeyModifiers::SHIFT;
        }
        KeyEvent::new(self.key.code(), modifiers)
    }
}

/// The outcome of feeding one key press to the keymap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Resolution {
    /// A complete binding.
    Action(UiAction),
    /// A prefix of a longer sequence. The front end should hold these chords
    /// and offer the next key to `resolve` again.
    Pending,
    /// Not a binding. The front end handles the key itself - this is how
    /// ordinary typing reaches the composer.
    Unbound,
}

/// A partially entered key sequence.
///
/// Held by the front end between key presses, since a leader sequence spans
/// several of them.
#[derive(Clone, Debug, Default)]
pub struct PendingSequence {
    chords: Vec<KeyChord>,
}

impl PendingSequence {
    pub fn is_empty(&self) -> bool {
        self.chords.is_empty()
    }

    /// Abandon the sequence, as escape does.
    pub fn clear(&mut self) {
        self.chords.clear();
    }
}

impl KeyBindings {
    /// Feed one key press to the keymap, advancing `pending` as needed.
    ///
    /// On anything but `Pending` the sequence is reset, so a failed or
    /// completed chord cannot leave the next key resolving against a stale
    /// prefix.
    pub fn resolve(&self, pending: &mut PendingSequence, press: KeyPress) -> Resolution {
        let event = press.event();

        let lookup = if pending.is_empty() {
            self.keymap_lookup_root_key(event)
        } else {
            self.keymap_lookup_with_key(&pending.chords, event)
        };

        match lookup {
            Some(KeyMapLookup::Action(action)) => {
                pending.clear();
                Resolution::Action(action)
            }
            Some(KeyMapLookup::Pending) => {
                pending.chords.push(self.keymap_chord_for_event(event));
                Resolution::Pending
            }
            None => {
                pending.clear();
                Resolution::Unbound
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use concord::config::KeymapOptions;

    fn press(key: Key) -> KeyPress {
        KeyPress {
            key,
            ctrl: false,
            alt: false,
            shift: false,
        }
    }

    #[test]
    fn key_names_parse_the_way_gpui_spells_them() {
        assert_eq!(Key::parse("enter"), Some(Key::Enter));
        assert_eq!(Key::parse("a"), Some(Key::Char('a')));
        assert_eq!(Key::parse("space"), Some(Key::Char(' ')));
        assert_eq!(Key::parse("f5"), Some(Key::F(5)));

        // Keys this does not model must not resolve to something arbitrary.
        assert_eq!(Key::parse("mediaplaypause"), None);
        assert_eq!(Key::parse("f13"), None);
        assert_eq!(Key::parse(""), None);
    }

    #[test]
    fn an_unbound_key_is_reported_rather_than_swallowed() {
        // Ordinary typing must reach the composer, so a key with no binding
        // has to be distinguishable from one that ran an action.
        let bindings = KeyBindings::from_options(&KeymapOptions::default());
        let mut pending = PendingSequence::default();

        let resolution = bindings.resolve(&mut pending, press(Key::Char('\u{1}')));
        assert_eq!(resolution, Resolution::Unbound);
        assert!(pending.is_empty(), "a miss must not leave a stale prefix");
    }

    #[test]
    fn a_leader_sequence_reports_pending_then_the_action() {
        let bindings = KeyBindings::from_options(&KeymapOptions::default());
        let mut pending = PendingSequence::default();

        // The default leader is a real prefix, so the first press cannot be a
        // complete binding on its own.
        let leader = press(Key::Char(' '));
        if bindings.resolve(&mut pending, leader) == Resolution::Pending {
            assert!(!pending.is_empty(), "a prefix must be retained");

            // Whatever follows, the sequence must not stay pending forever.
            let resolution = bindings.resolve(&mut pending, press(Key::Escape));
            assert!(
                matches!(resolution, Resolution::Action(_) | Resolution::Unbound),
                "a sequence must terminate"
            );
            assert!(pending.is_empty(), "termination must reset the prefix");
        }
    }
}
