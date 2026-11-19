//! Native host-facing port of prosemirror-menu 1.3.0.
//!
//! The menu tracks command availability and control focus. The host owns
//! widget rendering, native event propagation, and application of layout.

#[cfg(target_arch = "wasm32")]
pub mod browser;

mod icons;
mod menu;
mod menubar;
mod stock;

pub use icons::{icons, IconSpec};
pub use menu::{
    find_focusable_index, keyboard_move_focus, Dropdown, DropdownOptions, DropdownSubmenu,
    FocusDirection, MenuAction, MenuDisplay, MenuElement, MenuEvent, MenuEventSource, MenuFocus,
    MenuHost, MenuItem, MenuItemSpec, MenuItemState, MenuKey, MenuPath, MenuPredicate,
    MenuPrompt, MenuPromptField, MenuRender, MenuTitle, MenuTitleText, Orientation,
};
pub use menubar::{
    menu_bar, popup_origin, render_grouped, FloatingGeometry, FloatingMenu, MenuBar, MenuBarLayout,
    MenuBarOptions, MenuControl, MenuControlKind, MenuGroup, MenuPosition, MenuSnapshot, Rect,
};
pub use stock::{block_type_item, join_up_item, lift_item, redo_item, select_parent_node_item, undo_item, wrap_item};

#[derive(Debug, thiserror::Error)]
pub enum MenuError {
    #[error("MenuItem without icon or label property")]
    MissingDisplay,
}
