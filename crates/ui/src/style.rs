//! Colours and text attributes, spelled without reference to a renderer.
//!
//! `theme.toml` is shared between the two front ends, so it has to be parsed
//! and resolved in one place or a colour edited once would not match in the
//! other. That parsing used to produce ratatui's `Color`, which meant the GPUI
//! client pulled a terminal library in order to learn what colour something is.
//!
//! These mirror ratatui's shapes because `theme.toml` describes terminal
//! colours - named ANSI, 256-colour indices, and true colour - and inventing a
//! different vocabulary would make the conversion lossy for no gain. The
//! ratatui conversion lives behind the `terminal` feature.

/// A colour as `theme.toml` can name one.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Color {
    /// Whatever the surface's default is. Not black: a terminal's default may
    /// be light, and a GUI answers this from its own palette rather than
    /// inventing a colour.
    #[default]
    Reset,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    DarkGray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    White,
    /// A 256-colour palette index.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// Text attributes.
///
/// A small bitflag set for the five attributes `theme.toml` supports, rather
/// than a dependency for the two operations wanted.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Modifier(u8);

impl Modifier {
    pub const NONE: Self = Self(0);
    pub const BOLD: Self = Self(1 << 0);
    pub const DIM: Self = Self(1 << 1);
    pub const ITALIC: Self = Self(1 << 2);
    pub const UNDERLINED: Self = Self(1 << 3);
    pub const CROSSED_OUT: Self = Self(1 << 4);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub const fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}

impl std::ops::BitOr for Modifier {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for Modifier {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

impl std::ops::BitAnd for Modifier {
    type Output = Self;

    fn bitand(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

impl std::ops::Not for Modifier {
    type Output = Self;

    /// Bounded to the attributes modelled, so a complement does not set bits
    /// nothing can express.
    fn not(self) -> Self {
        const ALL: u8 = 0b0001_1111;
        Self(!self.0 & ALL)
    }
}

/// A foreground, a background and some attributes.
///
/// `None` for a colour means the theme did not set one, which is different
/// from setting it to `Reset`: unset inherits, reset goes back to the default.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub add_modifier: Modifier,
    pub sub_modifier: Modifier,
}

impl Style {
    pub const fn new() -> Self {
        Self {
            fg: None,
            bg: None,
            add_modifier: Modifier::NONE,
            sub_modifier: Modifier::NONE,
        }
    }

    #[must_use]
    pub const fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    #[must_use]
    pub const fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    #[must_use]
    pub const fn add_modifier(mut self, modifier: Modifier) -> Self {
        self.add_modifier.insert(modifier);
        self.sub_modifier.remove(modifier);
        self
    }

    #[must_use]
    pub const fn remove_modifier(mut self, modifier: Modifier) -> Self {
        self.add_modifier.remove(modifier);
        self.sub_modifier.insert(modifier);
        self
    }

    /// Lay another style over this one. Anything the other leaves unset shows
    /// through, which is what makes a highlight group able to inherit.
    #[must_use]
    pub fn patch(mut self, other: Self) -> Self {
        self.fg = other.fg.or(self.fg);
        self.bg = other.bg.or(self.bg);
        self.add_modifier.remove(other.sub_modifier);
        self.add_modifier.insert(other.add_modifier);
        self.sub_modifier.remove(other.add_modifier);
        self.sub_modifier.insert(other.sub_modifier);
        self
    }
}

/// The shape of a box's border.
///
/// Terminal-shaped - these are the line-drawing sets a terminal can render -
/// but held here rather than as ratatui's own type so the whole theme can be
/// parsed without a renderer. Turning one into glyphs needs the `terminal`
/// feature, which is the part a GUI has no use for.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BorderType {
    #[default]
    Plain,
    Rounded,
    Double,
    Thick,
    QuadrantInside,
    QuadrantOutside,
    LightDoubleDashed,
    HeavyDoubleDashed,
    LightTripleDashed,
    HeavyTripleDashed,
    LightQuadrupleDashed,
    HeavyQuadrupleDashed,
}

#[cfg(feature = "terminal")]
mod terminal {
    use super::{BorderType, Color, Modifier, Style};

    impl From<BorderType> for ratatui::widgets::BorderType {
        fn from(border: BorderType) -> Self {
            match border {
                BorderType::Plain => Self::Plain,
                BorderType::Rounded => Self::Rounded,
                BorderType::Double => Self::Double,
                BorderType::Thick => Self::Thick,
                BorderType::QuadrantInside => Self::QuadrantInside,
                BorderType::QuadrantOutside => Self::QuadrantOutside,
                BorderType::LightDoubleDashed => Self::LightDoubleDashed,
                BorderType::HeavyDoubleDashed => Self::HeavyDoubleDashed,
                BorderType::LightTripleDashed => Self::LightTripleDashed,
                BorderType::HeavyTripleDashed => Self::HeavyTripleDashed,
                BorderType::LightQuadrupleDashed => Self::LightQuadrupleDashed,
                BorderType::HeavyQuadrupleDashed => Self::HeavyQuadrupleDashed,
            }
        }
    }

    impl BorderType {
        /// The line-drawing characters for this shape.
        pub fn to_border_set(self) -> ratatui::symbols::border::Set<'static> {
            ratatui::widgets::BorderType::from(self).to_border_set()
        }
    }

    impl From<Color> for ratatui::style::Color {
        fn from(color: Color) -> Self {
            match color {
                Color::Reset => Self::Reset,
                Color::Black => Self::Black,
                Color::Red => Self::Red,
                Color::Green => Self::Green,
                Color::Yellow => Self::Yellow,
                Color::Blue => Self::Blue,
                Color::Magenta => Self::Magenta,
                Color::Cyan => Self::Cyan,
                Color::Gray => Self::Gray,
                Color::DarkGray => Self::DarkGray,
                Color::LightRed => Self::LightRed,
                Color::LightGreen => Self::LightGreen,
                Color::LightYellow => Self::LightYellow,
                Color::LightBlue => Self::LightBlue,
                Color::LightMagenta => Self::LightMagenta,
                Color::LightCyan => Self::LightCyan,
                Color::White => Self::White,
                Color::Indexed(index) => Self::Indexed(index),
                Color::Rgb(red, green, blue) => Self::Rgb(red, green, blue),
            }
        }
    }

    impl From<Modifier> for ratatui::style::Modifier {
        fn from(modifier: Modifier) -> Self {
            let mut result = Self::empty();
            for (ours, theirs) in [
                (Modifier::BOLD, Self::BOLD),
                (Modifier::DIM, Self::DIM),
                (Modifier::ITALIC, Self::ITALIC),
                (Modifier::UNDERLINED, Self::UNDERLINED),
                (Modifier::CROSSED_OUT, Self::CROSSED_OUT),
            ] {
                if modifier.contains(ours) {
                    result |= theirs;
                }
            }
            result
        }
    }

    impl From<ratatui::widgets::BorderType> for BorderType {
        fn from(border: ratatui::widgets::BorderType) -> Self {
            use ratatui::widgets::BorderType as Rt;
            match border {
                Rt::Rounded => Self::Rounded,
                Rt::Double => Self::Double,
                Rt::Thick => Self::Thick,
                Rt::QuadrantInside => Self::QuadrantInside,
                Rt::QuadrantOutside => Self::QuadrantOutside,
                Rt::LightDoubleDashed => Self::LightDoubleDashed,
                Rt::HeavyDoubleDashed => Self::HeavyDoubleDashed,
                Rt::LightTripleDashed => Self::LightTripleDashed,
                Rt::HeavyTripleDashed => Self::HeavyTripleDashed,
                Rt::LightQuadrupleDashed => Self::LightQuadrupleDashed,
                Rt::HeavyQuadrupleDashed => Self::HeavyQuadrupleDashed,
                // `Plain`, and anything ratatui adds that this does not model.
                _ => Self::Plain,
            }
        }
    }

    impl From<ratatui::style::Color> for Color {
        fn from(color: ratatui::style::Color) -> Self {
            use ratatui::style::Color as Rt;
            match color {
                Rt::Reset => Self::Reset,
                Rt::Black => Self::Black,
                Rt::Red => Self::Red,
                Rt::Green => Self::Green,
                Rt::Yellow => Self::Yellow,
                Rt::Blue => Self::Blue,
                Rt::Magenta => Self::Magenta,
                Rt::Cyan => Self::Cyan,
                Rt::Gray => Self::Gray,
                Rt::DarkGray => Self::DarkGray,
                Rt::LightRed => Self::LightRed,
                Rt::LightGreen => Self::LightGreen,
                Rt::LightYellow => Self::LightYellow,
                Rt::LightBlue => Self::LightBlue,
                Rt::LightMagenta => Self::LightMagenta,
                Rt::LightCyan => Self::LightCyan,
                Rt::White => Self::White,
                Rt::Indexed(index) => Self::Indexed(index),
                Rt::Rgb(red, green, blue) => Self::Rgb(red, green, blue),
            }
        }
    }

    impl From<ratatui::style::Modifier> for Modifier {
        fn from(modifier: ratatui::style::Modifier) -> Self {
            let mut result = Self::NONE;
            for (theirs, ours) in [
                (ratatui::style::Modifier::BOLD, Self::BOLD),
                (ratatui::style::Modifier::DIM, Self::DIM),
                (ratatui::style::Modifier::ITALIC, Self::ITALIC),
                (ratatui::style::Modifier::UNDERLINED, Self::UNDERLINED),
                (ratatui::style::Modifier::CROSSED_OUT, Self::CROSSED_OUT),
            ] {
                if modifier.contains(theirs) {
                    result |= ours;
                }
            }
            result
        }
    }

    impl From<ratatui::style::Style> for Style {
        fn from(style: ratatui::style::Style) -> Self {
            Self {
                fg: style.fg.map(Into::into),
                bg: style.bg.map(Into::into),
                add_modifier: style.add_modifier.into(),
                sub_modifier: style.sub_modifier.into(),
            }
        }
    }

    impl From<Style> for ratatui::style::Style {
        fn from(style: Style) -> Self {
            let mut result = Self::default();
            if let Some(fg) = style.fg {
                result = result.fg(fg.into());
            }
            if let Some(bg) = style.bg {
                result = result.bg(bg.into());
            }
            result
                .add_modifier(style.add_modifier.into())
                .remove_modifier(style.sub_modifier.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_is_not_the_same_as_reset() {
        // Unset inherits from whatever is underneath; reset goes back to the
        // surface default. A theme that conflated them would make every group
        // that named no colour override the one it was layered over.
        let base = Style::new().fg(Color::Red);
        assert_eq!(base.patch(Style::new()).fg, Some(Color::Red));
        assert_eq!(
            base.patch(Style::new().fg(Color::Reset)).fg,
            Some(Color::Reset)
        );
    }

    #[test]
    fn a_patch_that_removes_an_attribute_wins_over_one_that_added_it() {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let patched = bold.patch(Style::new().remove_modifier(Modifier::BOLD));

        assert!(!patched.add_modifier.contains(Modifier::BOLD));
        assert!(patched.sub_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn attributes_accumulate_across_patches() {
        let style = Style::new()
            .add_modifier(Modifier::BOLD)
            .patch(Style::new().add_modifier(Modifier::ITALIC));

        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn the_default_colour_is_reset_rather_than_black() {
        // A terminal's default may be light, and a GUI answers this from its
        // own palette. Defaulting to black would be a guess that looks right
        // on exactly one setup.
        assert_eq!(Color::default(), Color::Reset);
    }
}
