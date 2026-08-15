//! Theme resolution for front ends outside the TUI.
//!
//! `theme.toml` is shared with the TUI. Resolving it here rather than in the
//! GUI means link inheritance, defaults and warnings behave identically in
//! both clients, and a user editing one colour sees it in both.
//!
//! Only resolved colours cross the boundary. Everything the terminal cares
//! about - border shapes, dim, the distinction between "unset" and "reset" -
//! stays inside, because it has no counterpart in a GPU-rendered UI.

use ratatui::style::Color;

use super::Theme;
use crate::config::{HighlightGroup, ThemeOptions};

/// A resolved highlight, in terms any front end can use.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResolvedStyle {
    /// Foreground as `0xRRGGBB`. `None` means the theme left it to the
    /// terminal's default, which a GUI must answer with its own palette
    /// rather than an invented colour.
    pub foreground: Option<u32>,
    pub background: Option<u32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

/// Every highlight group the theme defines, resolved.
///
/// Keyed by the group's configuration name, which is the name users write in
/// `theme.toml`.
pub fn resolved_highlights(
    options: &ThemeOptions,
    warnings: &mut Vec<String>,
) -> Vec<(&'static str, ResolvedStyle)> {
    let theme = Theme::from_options(options, warnings);

    HighlightGroup::ALL
        .iter()
        .copied()
        .map(|group| {
            let style = theme.style(group);
            let modifiers = style.add_modifier;

            (
                group.name(),
                ResolvedStyle {
                    foreground: style.fg.and_then(rgb_of),
                    background: style.bg.and_then(rgb_of),
                    bold: modifiers.contains(ratatui::style::Modifier::BOLD),
                    italic: modifiers.contains(ratatui::style::Modifier::ITALIC),
                    underline: modifiers.contains(ratatui::style::Modifier::UNDERLINED),
                    strikethrough: modifiers.contains(ratatui::style::Modifier::CROSSED_OUT),
                },
            )
        })
        .collect()
}

/// Convert a terminal colour to RGB.
///
/// The 16 ANSI colours have no fixed RGB value - they are whatever the user's
/// terminal palette says - so they resolve to `None` rather than to one
/// vendor's guess at "blue". A GUI keeps its own colour in that case, which is
/// truer to the intent than picking an arbitrary one.
fn rgb_of(color: Color) -> Option<u32> {
    match color {
        Color::Rgb(r, g, b) => Some(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)),
        // 6x6x6 cube and greyscale ramp are defined in absolute terms, unlike
        // the low 16, so these can be converted exactly.
        Color::Indexed(index) if index >= 16 => Some(xterm_rgb(index)),
        _ => None,
    }
}

/// RGB for an xterm-256 index at or above 16.
fn xterm_rgb(index: u8) -> u32 {
    if index >= 232 {
        // Greyscale ramp: 24 steps from 8 to 238.
        let level = 8 + 10 * u32::from(index - 232);
        return level << 16 | level << 8 | level;
    }

    // 6x6x6 colour cube. The steps are not linear: 0 then 95, thereafter +40.
    let index = u32::from(index - 16);
    let component = |value: u32| -> u32 { if value == 0 { 0 } else { 55 + value * 40 } };

    let r = component(index / 36);
    let g = component((index / 6) % 6);
    let b = component(index % 6);

    r << 16 | g << 8 | b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_group_resolves() {
        let mut warnings = Vec::new();
        let resolved = resolved_highlights(&ThemeOptions::default(), &mut warnings);

        assert_eq!(resolved.len(), HighlightGroup::COUNT);
        assert!(
            warnings.is_empty(),
            "the default theme must not warn: {warnings:?}"
        );
    }

    #[test]
    fn ansi_colours_resolve_to_none_rather_than_a_guess() {
        // These are whatever the user's terminal palette says, so there is no
        // correct RGB for them and inventing one would be worse than leaving
        // the front end's own colour in place.
        assert_eq!(rgb_of(Color::Blue), None);
        assert_eq!(rgb_of(Color::Reset), None);
        assert_eq!(rgb_of(Color::Indexed(9)), None);
    }

    #[test]
    fn indexed_colours_convert_on_the_xterm_scale() {
        assert_eq!(rgb_of(Color::Rgb(0x12, 0x34, 0x56)), Some(0x0012_3456));

        // Cube corners: 16 is black, 231 is white.
        assert_eq!(rgb_of(Color::Indexed(16)), Some(0x0000_0000));
        assert_eq!(rgb_of(Color::Indexed(231)), Some(0x00FF_FFFF));

        // The ramp starts at 95, not 51: the first step is the irregular one,
        // and a linear formula would be wrong for every colour in the cube.
        assert_eq!(rgb_of(Color::Indexed(16 + 36)), Some(0x005F_0000));

        // Greyscale ramp ends at 238, not 255.
        assert_eq!(rgb_of(Color::Indexed(232)), Some(0x0008_0808));
        assert_eq!(rgb_of(Color::Indexed(255)), Some(0x00EE_EEEE));
    }
}
