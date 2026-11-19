#[cfg(all(target_arch = "wasm32", feature = "browser"))]
pub mod browser;
pub mod cell_commands;
pub mod cellselection;
pub mod columnresizing;
pub mod commands;
pub mod copypaste;
pub mod fixtables;
pub mod input;
pub mod schema;
pub mod tablemap;
pub mod tableview;
pub mod utils;

pub use schema::{table_node_types, table_nodes, TableNodesOptions, TableRole};
pub use tablemap::{Axis, Problem, Rect, TableMap, TableMapError};
pub use fixtables::{fix_tables, FIX_TABLES_KEY};
pub use commands::{
    add_column, add_column_after, add_column_before, add_row, add_row_after, add_row_before,
    delete_column, delete_row, remove_column, remove_row, row_is_header, selected_rect, TableEdit,
    TableRect,
};
pub use cell_commands::{
    delete_cell_selection, delete_table, go_to_next_cell, merge_cells, move_table_column, move_table_row,
    set_cell_attr, split_cell, split_cell_with_type, toggle_header, toggle_header_cell, toggle_header_column,
    toggle_header_row, GetCellTypeOptions, MoveTableOptions, ToggleHeaderOptions, ToggleHeaderType,
};
pub use columnresizing::{column_resizing, ColumnResizingOptions, ResizeState, COLUMN_RESIZING_KEY};
pub use input::{handle_paste, handle_triple_click};
pub use tableview::{update_columns_on_resize, ColumnLayout, TableView};
pub use cellselection::{
    cell_around, draw_cell_selection, normalize_selection, points_at_cell, table_editing,
    CellBookmark, CellSelection, TableEditingOptions, TABLE_EDITING_KEY,
};
