//! What both front ends need and neither should own.
//!
//! The terminal and GPUI clients agree about more than they look like they do:
//! how a search query scores a candidate, what a key press resolves to, how a
//! theme's colours are decided. All of that used to live in the terminal front
//! end, which meant the GPUI client depended on a terminal module and the core
//! carried a drawing library to hold it.
//!
//! The test for whether something belongs here is a renderer, not the core.
//! Depending on `concord` is fine - it sits below both front ends. Needing
//! ratatui or GPUI is what makes a thing one front end's.

pub mod fuzzy;
pub mod key;
pub mod keybindings;
pub mod model;
pub mod style;
pub mod text_cursor;
pub mod text_input;
pub mod theme;

pub use fuzzy::fuzzy_text_score;
pub use keybindings::KeyBindings;
pub use model::{ActionAvailability, FocusPane, MessageActionKind};
pub use text_input::{TextEditAction, TextInputState};
