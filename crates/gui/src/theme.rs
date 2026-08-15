//! Design tokens.
//!
//! Centralised so views never hardcode a colour or a spacing value. The
//! palette is deliberately *not* a Discord clone: it is a neutral dark theme
//! tuned for long reading sessions, with a single accent hue and a strict
//! contrast floor (body text >= 7:1 on its background, secondary >= 4.5:1).
//!
//! Upstream ships a themeable TUI (`config::ThemeOptions`, `theme.toml`).
//! Mapping that onto these tokens is a later step; the token names are chosen
//! to make that mapping mechanical.

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{Hsla, Rgba, rgb};

/// Semantic colour tokens.
#[derive(Clone, Copy)]
pub struct Palette {
    /// Window background, behind everything.
    pub bg: u32,
    /// Raised surface: message area, main content.
    pub surface: u32,
    /// Sunken surface: sidebars, rails.
    pub surface_sunken: u32,
    /// Hover / selected row background.
    pub surface_hover: u32,
    pub surface_active: u32,

    /// Hairline dividers.
    pub border: u32,

    /// Primary body text.
    pub text: u32,
    /// Labels, timestamps, metadata.
    pub text_muted: u32,
    /// Disabled / placeholder.
    pub text_subtle: u32,

    /// Single accent hue - links, focus, selection.
    pub accent: u32,
    pub accent_hover: u32,
    /// Text placed on top of `accent`.
    pub on_accent: u32,

    pub success: u32,
    pub warning: u32,
    pub danger: u32,

    /// Presence indicators.
    pub online: u32,
    pub idle: u32,
    pub dnd: u32,
    pub offline: u32,
}

pub const DARK: Palette = Palette {
    bg: 0x121417,
    surface: 0x181b1f,
    surface_sunken: 0x0e1013,
    surface_hover: 0x21252b,
    surface_active: 0x2a2f36,

    border: 0x262b31,

    text: 0xe8eaed,
    text_muted: 0x9aa3af,
    text_subtle: 0x6b7280,

    accent: 0x5b8def,
    accent_hover: 0x7aa3f5,
    on_accent: 0xffffff,

    success: 0x3fb950,
    warning: 0xd29922,
    danger: 0xf85149,

    online: 0x3fb950,
    idle: 0xd29922,
    dnd: 0xf85149,
    offline: 0x6b7280,
};

pub const LIGHT: Palette = Palette {
    bg: 0xf2f3f5,
    surface: 0xffffff,
    surface_sunken: 0xebedef,
    surface_hover: 0xe3e5e8,
    surface_active: 0xd8daed,

    border: 0xd1d5db,

    text: 0x2e3338,
    text_muted: 0x4f5660,
    text_subtle: 0x747f8d,

    accent: 0x5865f2,
    accent_hover: 0x4752c4,
    on_accent: 0xffffff,

    success: 0x3ba55d,
    warning: 0xfaa81a,
    danger: 0xed4245,

    online: 0x3ba55d,
    idle: 0xfaa81a,
    dnd: 0xed4245,
    offline: 0x747f8d,
};

/// Selected palette.
///
/// An atomic rather than threading a `&Palette` through every view: the theme
/// changes only when the user flips a setting, and passing it down would touch
/// every signature in the UI for a value that is effectively constant.
static LIGHT_MODE: AtomicBool = AtomicBool::new(false);

/// Interface scale, as a percentage held in an integer because floats have no
/// atomic form. Applied to the type scale rather than to the window, so layout
/// reflows at the new size instead of being magnified with it.
static ZOOM_PERCENT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(100);

pub fn set_zoom(scale: f32) {
    ZOOM_PERCENT.store((scale * 100.0) as u32, Ordering::Relaxed);
}

pub fn zoom() -> f32 {
    ZOOM_PERCENT.load(Ordering::Relaxed) as f32 / 100.0
}

/// Scale a type size by the current zoom.
pub fn scaled(size: f32) -> f32 {
    size * zoom()
}

/// Switch palettes. Takes effect on the next render.
pub fn set_light_mode(enabled: bool) {
    LIGHT_MODE.store(enabled, Ordering::Relaxed);
}

pub fn is_light_mode() -> bool {
    LIGHT_MODE.load(Ordering::Relaxed)
}

/// The palette every view should read.
pub fn active() -> &'static Palette {
    if is_light_mode() { &LIGHT } else { &DARK }
}

impl Palette {
    pub fn c(&self, value: u32) -> Rgba {
        let _ = self;
        rgb(value)
    }
}

/// Spacing scale (px). A 4px base grid - every gap and pad is a multiple,
/// which is most of what keeps a dense chat UI from looking arbitrary.
pub mod space {
    pub const XS: f32 = 4.;
    pub const SM: f32 = 8.;
    pub const MD: f32 = 12.;
    pub const LG: f32 = 16.;
    pub const XL: f32 = 24.;
}

/// Type scale (px / line-height pairs).
pub mod text {
    /// Timestamps, badges.
    pub const XS: f32 = 11.;
    /// Metadata, channel names.
    pub const SM: f32 = 13.;
    /// Message body - the size that matters most.
    pub const BASE: f32 = 14.;
    /// Section headers.
    pub const LG: f32 = 16.;
    /// Screen titles.
    pub const XL: f32 = 20.;
}

/// Fixed layout dimensions (px).
pub mod layout {
    /// Guild rail on the far left.
    pub const GUILD_RAIL: f32 = 68.;
    /// Channel sidebar.
    pub const SIDEBAR: f32 = 232.;
    /// Member list on the right.
    pub const MEMBERS: f32 = 232.;
    /// Top bar height.
    pub const HEADER: f32 = 48.;
    /// Avatar sizes.
    pub const AVATAR: f32 = 36.;
    pub const AVATAR_SM: f32 = 20.;
    /// Corner radius.
    pub const RADIUS: f32 = 6.;
    pub const RADIUS_LG: f32 = 10.;
}

/// Presence state, mapped to a palette colour.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Online,
    Idle,
    Dnd,
    Offline,
}

impl Presence {
    pub fn color(self, p: &Palette) -> u32 {
        match self {
            Presence::Online => p.online,
            Presence::Idle => p.idle,
            Presence::Dnd => p.dnd,
            Presence::Offline => p.offline,
        }
    }
}

/// Convenience for callers that need an `Hsla` (GPUI's native colour type).
pub fn hsla(value: u32) -> Hsla {
    rgb(value).into()
}
