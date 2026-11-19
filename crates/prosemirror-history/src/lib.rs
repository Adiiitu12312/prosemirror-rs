pub mod history;

pub use history::{
    close_history, history, history_plugin, redo, redo_depth, redo_no_scroll, undo, undo_depth,
    undo_no_scroll, Branch, HistoryField, HistoryItem, HistoryOptions, HistoryState, HISTORY_KEY,
};
