mod actions;
mod keyboard;
mod pointer;

use crossterm::event::MouseEvent;
use ratatui::layout::Rect;

use super::state::DashboardState;

pub use self::keyboard::{
    handle_key, handle_paste, handle_pasted_file_attachments, handle_pasted_user_profile_avatar,
};
pub type MouseInputState = self::pointer::MouseInputState;
pub type MouseEventResult = self::pointer::MouseEventResult;

pub fn handle_mouse_event(
    state: &mut DashboardState,
    mouse: MouseEvent,
    area: Rect,
    input: &mut MouseInputState,
) -> MouseEventResult {
    self::pointer::handle_mouse_event(state, mouse, area, input)
}

#[cfg(test)]
pub fn handle_mouse(state: &mut DashboardState, mouse: MouseEvent, area: Rect) -> bool {
    self::pointer::handle_mouse(state, mouse, area)
}

#[cfg(test)]
mod tests;
