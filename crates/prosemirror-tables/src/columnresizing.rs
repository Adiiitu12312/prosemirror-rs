//! Column resizing behavior from prosemirror-tables@1.8.5/src/columnresizing.ts.

use prosemirror_state::{points_at_cell, EditorState, EditorStateConfig, Plugin, PluginKey, Transaction, TypedStateField};
use prosemirror_view::{Decoration, DecorationSet};
use serde_json::{json, Value};

use crate::commands::{cell_attrs, get_span, set_attrs, TableRect};
use crate::tableview::{update_columns_on_resize, ColumnLayout};

pub const COLUMN_RESIZING_KEY: &str = "tableColumnResizing$";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnResizingOptions {
    pub handle_width: f64,
    pub cell_min_width: f64,
    pub default_cell_min_width: f64,
    pub last_column_resizable: bool,
}

impl Default for ColumnResizingOptions {
    fn default() -> Self {
        Self { handle_width: 5.0, cell_min_width: 25.0, default_cell_min_width: 100.0, last_column_resizable: true }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dragging {
    pub start_x: f64,
    pub start_width: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ResizeState {
    pub active_handle: Option<usize>,
    pub dragging: Option<Dragging>,
}

#[derive(Clone, Copy)]
enum Action {
    Handle(Option<usize>),
    Dragging(Option<Dragging>),
}

impl ResizeState {
    pub fn get(state: &EditorState) -> Option<&Self> {
        state.get_typed_plugin_state(COLUMN_RESIZING_KEY)
    }

    pub fn apply(self, tr: &Transaction) -> Self {
        if let Some(action) = tr.get_typed_meta::<Action>(COLUMN_RESIZING_KEY) {
            return match action {
                Action::Handle(active_handle) => Self { active_handle: *active_handle, dragging: None },
                Action::Dragging(dragging) => Self { dragging: *dragging, ..self },
            };
        }
        if tr.doc_changed() {
            if let Some(handle) = self.active_handle {
                let handle = tr.mapping().map(handle, -1);
                let active_handle = tr.doc().resolve(handle).ok().filter(points_at_cell).map(|pos| pos.pos);
                return Self { active_handle, ..self };
            }
        }
        self
    }
}

struct ResizeField;

impl TypedStateField for ResizeField {
    type Value = ResizeState;

    fn init(&self, _: &EditorStateConfig, _: &EditorState) -> Self::Value {
        ResizeState::default()
    }

    fn apply(&self, tr: &Transaction, state: &ResizeState, _: &EditorState, _: &EditorState) -> Self::Value {
        state.apply(tr)
    }
}

pub fn column_resizing() -> Plugin {
    Plugin::new(PluginKey { name: COLUMN_RESIZING_KEY.to_owned() }, None).with_typed_state_field(ResizeField)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge { Left, Right }

impl ColumnResizingOptions {
    pub fn edge_at(self, pointer_x: f64, left: f64, right: f64) -> Option<Edge> {
        if pointer_x - left <= self.handle_width { Some(Edge::Left) }
        else if right - pointer_x <= self.handle_width { Some(Edge::Right) }
        else { None }
    }

    pub fn hover(self, state: &EditorState, hit: Option<(usize, Edge)>) -> Option<Transaction> {
        let resize = ResizeState::get(state)?;
        if resize.dragging.is_some() {
            return None;
        }
        let handle = hit.and_then(|(cell, edge)| {
            let rect = TableRect::from_cells(&state.doc, cell, cell)?;
            match edge {
                Edge::Right => Some(cell),
                Edge::Left if rect.rect.left > 0 => {
                    let index = rect.rect.top * rect.map.width + rect.rect.left - 1;
                    Some(rect.table_start + rect.map.map[index])
                }
                Edge::Left => None,
            }
        });
        if let Some(handle) = handle.filter(|_| !self.last_column_resizable) {
            let rect = TableRect::from_cells(&state.doc, handle, handle)?;
            if rect.rect.right == rect.map.width {
                return None;
            }
        }
        (handle != resize.active_handle).then(|| metadata(state, Action::Handle(handle)))
    }

    pub fn start_drag(self, state: &EditorState, x: f64, measured_width: f64) -> Option<Transaction> {
        let resize = ResizeState::get(state)?;
        if resize.dragging.is_some() {
            return None;
        }
        let cell = state.doc.node_at(resize.active_handle?)?;
        let span = get_span(cell, "colspan");
        let widths = cell.attrs().and_then(|attrs| attrs["colwidth"].as_array());
        let explicit = widths.and_then(|widths| widths.last()).and_then(Value::as_f64).filter(|width| *width != 0.0);
        let start_width = explicit.unwrap_or_else(|| {
            let defined: Vec<_> = widths.into_iter().flatten().filter_map(Value::as_f64).filter(|width| *width != 0.0).collect();
            let defined_width: f64 = defined.iter().sum();
            (measured_width - defined_width) / span.saturating_sub(defined.len()).max(1) as f64
        });
        Some(metadata(state, Action::Dragging(Some(Dragging { start_x: x, start_width }))))
    }

    pub fn preview(self, state: &EditorState, x: f64) -> Option<ColumnLayout> {
        let resize = ResizeState::get(state)?;
        let handle = resize.active_handle?;
        let rect = TableRect::from_cells(&state.doc, handle, handle)?;
        let width = self.dragged_width(resize.dragging?, x);
        Some(update_columns_on_resize(&rect.table, self.default_cell_min_width, Some((rect.rect.right - 1, width))))
    }

    pub fn commit_width(self, state: &EditorState, x: f64) -> Option<Transaction> {
        let resize = ResizeState::get(state)?;
        update_column_width(state, resize.active_handle?, self.dragged_width(resize.dragging?, x))
    }

    pub fn end_drag(self, state: &EditorState) -> Transaction {
        metadata(state, Action::Dragging(None))
    }

    fn dragged_width(self, dragging: Dragging, x: f64) -> f64 {
        self.cell_min_width.max(dragging.start_width + x - dragging.start_x)
    }
}

fn metadata(state: &EditorState, action: Action) -> Transaction {
    let mut tr = state.tr();
    tr.set_typed_meta(COLUMN_RESIZING_KEY, action);
    tr
}

pub fn update_column_width(state: &EditorState, cell: usize, width: f64) -> Option<Transaction> {
    let rect = TableRect::from_cells(&state.doc, cell, cell)?;
    let col = rect.rect.right.checked_sub(1)?;
    let mut tr = state.tr();
    for row in 0..rect.map.height {
        let index = row * rect.map.width + col;
        let pos = rect.map.map[index];
        if row > 0 && rect.map.map[index - rect.map.width] == pos {
            continue;
        }
        let cell = rect.table.node_at(pos)?;
        let span = get_span(cell, "colspan");
        let index = col - rect.map.col_count(pos).ok()?;
        let mut attrs = cell_attrs(cell);
        let mut widths = attrs.get("colwidth").and_then(Value::as_array).cloned().unwrap_or_default();
        widths.resize(span, json!(0));
        if widths[index].as_f64() != Some(width) {
            widths[index] = json!(width);
            attrs.insert("colwidth".into(), Value::Array(widths));
            set_attrs(&mut tr, rect.table_start + pos, attrs);
        }
    }
    tr.doc_changed().then_some(tr)
}

pub fn handle_decorations(state: &EditorState, cell: Option<usize>) -> DecorationSet {
    let Some(rect) = cell.and_then(|cell| TableRect::from_cells(&state.doc, cell, cell)) else {
        return DecorationSet::default();
    };
    let col = rect.rect.right - 1;
    let dragging = ResizeState::get(state).and_then(|state| state.dragging).is_some();
    let mut decorations = Vec::new();
    for row in 0..rect.map.height {
        let index = row * rect.map.width + col;
        let pos = rect.map.map[index];
        if (col + 1 < rect.map.width && rect.map.map[index + 1] == pos)
            || (row > 0 && rect.map.map[index - rect.map.width] == pos)
        {
            continue;
        }
        let Some(cell) = rect.table.node_at(pos) else { continue };
        let from = rect.table_start + pos;
        let end = from + cell.node_size();
        if dragging {
            decorations.push(Decoration::node(from, end, [("class".into(), "column-resize-dragging".into())].into(), Value::Null));
        }
        decorations.push(Decoration::widget(end - 1, from as u64, json!({"class":"column-resize-handle"})));
    }
    DecorationSet::create(&state.doc, decorations)
}
