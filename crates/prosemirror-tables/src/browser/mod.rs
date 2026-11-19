//! Browser adapters for the table editing and column resizing state plugins.

mod columnresizing;
mod drag;
mod input;
mod tableview;

pub use columnresizing::{column_resizing_props, column_resizing_props_with_view};
pub use input::table_editing_props;
pub use tableview::{update_dom_columns_on_resize, BrowserTableView};

/// Upstream table selection and resizing styles.
pub const CSS: &str = include_str!("../../style/tables.css");
