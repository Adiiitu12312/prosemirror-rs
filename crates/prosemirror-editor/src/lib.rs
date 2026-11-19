//! Headless editor for native renderers and platform services.

mod clipboard;
mod editor;
mod event;
mod host;
mod navigation;
mod plugins;

pub use clipboard::{paste_slice, serialize_clipboard};
pub use editor::{Editor, EditorError};
pub use event::*;
pub use host::*;
pub use navigation::{move_selection, Direction, Movement};
pub use plugins::*;
pub use prosemirror_view::{Decoration, DecorationAttrs, DecorationGroup, DecorationSet, DecorationType};
