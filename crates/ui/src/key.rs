//! A key press, spelled without reference to any particular front end.
//!
//! This used to be crossterm's `KeyEvent` throughout, which meant the GPUI
//! client pulled a terminal library in order to ask what a key was bound to.
//! The shapes here mirror crossterm's closely - it models terminal input well,
//! and a different vocabulary would only make the conversion lossy - but they
//! are ours, so nothing outside a terminal has to know crossterm exists.
//!
//! The crossterm conversion lives behind the `terminal` feature, which the
//! terminal front end turns on and the GPUI one does not.

/// Which modifiers were held.
///
/// A small bitflag set rather than a dependency on `bitflags`: the operations
/// wanted here are union and containment, and both are two lines.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KeyModifiers(u8);

impl KeyModifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1 << 0);
    pub const CONTROL: Self = Self(1 << 1);
    pub const ALT: Self = Self(1 << 2);
    pub const SUPER: Self = Self(1 << 3);
    pub const HYPER: Self = Self(1 << 4);
    pub const META: Self = Self(1 << 5);

    /// Every modifier in `other` is held.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// At least one modifier in `other` is held.
    ///
    /// Distinct from `contains`, and the difference matters: asking whether
    /// any of ctrl/alt/super is down is not the same as asking whether all
    /// three are.
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Add a modifier to the set.
    pub const fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Nothing held. Spelled as a constructor as well as a constant because
    /// both readings appear naturally at call sites.
    pub const fn empty() -> Self {
        Self::NONE
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for KeyModifiers {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitAnd for KeyModifiers {
    type Output = Self;

    fn bitand(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

impl std::ops::Not for KeyModifiers {
    type Output = Self;

    /// Bounded to the modifiers actually modelled, so a complement does not
    /// set bits nothing can hold.
    fn not(self) -> Self {
        const ALL: u8 = 0b0011_1111;
        Self(!self.0 & ALL)
    }
}

impl std::ops::BitOrAssign for KeyModifiers {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// A media key, on keyboards that have them.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MediaKeyCode {
    Play,
    Pause,
    PlayPause,
    Reverse,
    Stop,
    FastForward,
    Rewind,
    TrackNext,
    TrackPrevious,
    Record,
    LowerVolume,
    RaiseVolume,
    MuteVolume,
}

/// Which key.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum KeyCode {
    Backspace,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    Delete,
    Insert,
    F(u8),
    Char(char),
    Null,
    Esc,
    CapsLock,
    ScrollLock,
    NumLock,
    PrintScreen,
    Pause,
    Menu,
    KeypadBegin,
    Media(MediaKeyCode),
    /// A modifier pressed on its own. The payload is dropped rather than
    /// modelled: nothing here distinguishes left shift from right, and a
    /// binding to a bare modifier is refused rather than resolved either way.
    Modifier(()),
}

/// One key press.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyEvent {
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }
}

#[cfg(feature = "terminal")]
mod terminal {
    use super::{KeyCode, KeyEvent, KeyModifiers, MediaKeyCode};

    impl From<crossterm::event::KeyModifiers> for KeyModifiers {
        fn from(modifiers: crossterm::event::KeyModifiers) -> Self {
            use crossterm::event::KeyModifiers as Ct;
            let mut result = Self::NONE;
            for (theirs, ours) in [
                (Ct::SHIFT, Self::SHIFT),
                (Ct::CONTROL, Self::CONTROL),
                (Ct::ALT, Self::ALT),
                (Ct::SUPER, Self::SUPER),
                (Ct::HYPER, Self::HYPER),
                (Ct::META, Self::META),
            ] {
                if modifiers.contains(theirs) {
                    result |= ours;
                }
            }
            result
        }
    }

    impl From<crossterm::event::MediaKeyCode> for MediaKeyCode {
        fn from(media: crossterm::event::MediaKeyCode) -> Self {
            use crossterm::event::MediaKeyCode as Ct;
            match media {
                Ct::Play => Self::Play,
                Ct::Pause => Self::Pause,
                Ct::PlayPause => Self::PlayPause,
                Ct::Reverse => Self::Reverse,
                Ct::Stop => Self::Stop,
                Ct::FastForward => Self::FastForward,
                Ct::Rewind => Self::Rewind,
                Ct::TrackNext => Self::TrackNext,
                Ct::TrackPrevious => Self::TrackPrevious,
                Ct::Record => Self::Record,
                Ct::LowerVolume => Self::LowerVolume,
                Ct::RaiseVolume => Self::RaiseVolume,
                Ct::MuteVolume => Self::MuteVolume,
            }
        }
    }

    impl From<crossterm::event::KeyCode> for KeyCode {
        fn from(code: crossterm::event::KeyCode) -> Self {
            use crossterm::event::KeyCode as Ct;
            match code {
                Ct::Backspace => Self::Backspace,
                Ct::Enter => Self::Enter,
                Ct::Left => Self::Left,
                Ct::Right => Self::Right,
                Ct::Up => Self::Up,
                Ct::Down => Self::Down,
                Ct::Home => Self::Home,
                Ct::End => Self::End,
                Ct::PageUp => Self::PageUp,
                Ct::PageDown => Self::PageDown,
                Ct::Tab => Self::Tab,
                Ct::BackTab => Self::BackTab,
                Ct::Delete => Self::Delete,
                Ct::Insert => Self::Insert,
                Ct::F(number) => Self::F(number),
                Ct::Char(character) => Self::Char(character),
                Ct::Null => Self::Null,
                Ct::Esc => Self::Esc,
                Ct::CapsLock => Self::CapsLock,
                Ct::ScrollLock => Self::ScrollLock,
                Ct::NumLock => Self::NumLock,
                Ct::PrintScreen => Self::PrintScreen,
                Ct::Pause => Self::Pause,
                Ct::Menu => Self::Menu,
                Ct::KeypadBegin => Self::KeypadBegin,
                Ct::Media(media) => Self::Media(media.into()),
                Ct::Modifier(_) => Self::Modifier(()),
            }
        }
    }

    impl From<crossterm::event::KeyEvent> for KeyEvent {
        fn from(event: crossterm::event::KeyEvent) -> Self {
            Self::new(event.code.into(), event.modifiers.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_combine_and_are_tested_for_individually() {
        let held = KeyModifiers::CONTROL | KeyModifiers::SHIFT;

        assert!(held.contains(KeyModifiers::CONTROL));
        assert!(held.contains(KeyModifiers::SHIFT));
        assert!(!held.contains(KeyModifiers::ALT));
    }

    #[test]
    fn containment_of_a_pair_needs_both() {
        // A binding written "ctrl-shift-k" must not match a press of ctrl-k.
        let held = KeyModifiers::CONTROL;
        assert!(!held.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT));
    }

    #[test]
    fn nothing_held_contains_nothing() {
        assert!(KeyModifiers::NONE.is_empty());
        assert!(KeyModifiers::NONE.contains(KeyModifiers::NONE));
        assert!(!KeyModifiers::NONE.contains(KeyModifiers::SHIFT));
    }
}
