use std::sync::Arc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use prosemirror_state::{EditorState, Plugin, PluginKey, Selection, StateField, Transaction};
use prosemirror_transform::Step;

pub const HISTORY_KEY: &str = "history$";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryItem {
    pub step: Step,
    pub inverted: Step,
    pub selection: Selection,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HistoryState {
    pub done: Vec<HistoryItem>,
    pub undone: Vec<HistoryItem>,
}

pub struct HistoryField {
    pub depth: usize,
}

impl Default for HistoryField {
    fn default() -> Self {
        Self { depth: 100 }
    }
}

impl StateField for HistoryField {
    fn init(&self, _config: &Value) -> Value {
        serde_json::to_value(HistoryState::default()).unwrap()
    }

    fn apply(&self, tr: &Transaction, value: &Value) -> Value {
        if let Some(meta_val) = tr.get_meta(HISTORY_KEY) {
            if let Ok(new_hist) = serde_json::from_value::<HistoryState>(meta_val.clone()) {
                return serde_json::to_value(new_hist).unwrap();
            }
        }

        let mut history: HistoryState = serde_json::from_value(value.clone()).unwrap_or_default();

        if !tr.doc_changed() {
            return value.clone();
        }

        // Normal document change: clear redo stack, push to done stack
        history.undone.clear();

        for (i, step) in tr.steps().iter().enumerate() {
            let doc_before = tr.transform.docs.get(i).unwrap_or(tr.doc());
            let inverted = step.invert(doc_before);
            history.done.push(HistoryItem {
                step: step.clone(),
                inverted,
                selection: tr.selection.clone(),
            });
            if history.done.len() > self.depth {
                history.done.remove(0);
            }
        }

        serde_json::to_value(history).unwrap()
    }
}

pub fn history_plugin(depth: usize) -> Plugin {
    Plugin::new(
        PluginKey::new("history"),
        Some(Arc::new(HistoryField { depth })),
    )
}

pub fn undo(state: &EditorState, mut dispatch: impl FnMut(Transaction)) -> bool {
    let hist_val = match state.get_plugin_state("history") {
        Some(v) => v,
        None => return false,
    };
    let mut history: HistoryState = match serde_json::from_value(hist_val.clone()) {
        Ok(h) => h,
        Err(_) => return false,
    };

    if history.done.is_empty() {
        return false;
    }

    let item = history.done.pop().unwrap();
    let mut tr = state.tr();
    let _ = tr.step(item.inverted.clone());
    tr.set_selection(item.selection.clone());

    let redo_item = HistoryItem {
        step: item.inverted,
        inverted: item.step,
        selection: state.selection.clone(),
    };
    history.undone.push(redo_item);
    tr.set_meta(HISTORY_KEY, serde_json::to_value(&history).unwrap());

    dispatch(tr);
    true
}

pub fn redo(state: &EditorState, mut dispatch: impl FnMut(Transaction)) -> bool {
    let hist_val = match state.get_plugin_state("history") {
        Some(v) => v,
        None => return false,
    };
    let mut history: HistoryState = match serde_json::from_value(hist_val.clone()) {
        Ok(h) => h,
        Err(_) => return false,
    };

    if history.undone.is_empty() {
        return false;
    }

    let item = history.undone.pop().unwrap();
    let mut tr = state.tr();
    let _ = tr.step(item.inverted.clone());
    tr.set_selection(item.selection.clone());

    let undo_item = HistoryItem {
        step: item.inverted,
        inverted: item.step,
        selection: state.selection.clone(),
    };
    history.done.push(undo_item);
    tr.set_meta(HISTORY_KEY, serde_json::to_value(&history).unwrap());

    dispatch(tr);
    true
}

pub fn undo_depth(state: &EditorState) -> usize {
    state
        .get_plugin_state("history")
        .and_then(|v| serde_json::from_value::<HistoryState>(v.clone()).ok())
        .map(|h| h.done.len())
        .unwrap_or(0)
}

pub fn redo_depth(state: &EditorState) -> usize {
    state
        .get_plugin_state("history")
        .and_then(|v| serde_json::from_value::<HistoryState>(v.clone()).ok())
        .map(|h| h.undone.len())
        .unwrap_or(0)
}
