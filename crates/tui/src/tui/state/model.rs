//! The terminal front end's view vocabulary.
//!
//! Almost all of it now lives in `concord-ui`, shared with the GPUI front end
//! so the key bindings that resolve to these types can be shared too. This
//! module re-exports it under the path the terminal code already used, and
//! keeps the one type that could not travel.

pub use concord_ui::model::*;

use ratatui_image::protocol::Protocol;

/// Stays here rather than moving with the rest: it borrows a ratatui image
/// protocol, which is a renderer's type, and a renderer is exactly what the
/// shared crate refuses to depend on.
pub enum LocalUploadPreviewView<'a> {
    Loading { filename: String },
    Ready { protocol: &'a Protocol },
    Failed { filename: String, message: String },
}
