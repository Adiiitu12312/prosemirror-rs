//! Cell selection presentation and normalization from prosemirror-tables 1.8.5.

use std::sync::Arc;

use prosemirror_model::{Node, ResolvedPos};
use prosemirror_state::{EditorState, Plugin, PluginKey, Selection, SelectionRange, StateField, Transaction};
use prosemirror_view::{Decoration, DecorationSet};
use serde_json::Value;

use crate::{fix_tables, TableMap};

pub use prosemirror_state::{cell_around, points_at_cell, CellBookmark, CellSelection};

pub const TABLE_EDITING_KEY: &str = "selectingCells$";

pub fn draw_cell_selection(state: &EditorState) -> Option<DecorationSet> {
    let Selection::Cell(selection) = &state.selection else { return None };
    let mut cells = Vec::new();
    selection.for_each_cell(&state.doc, |node, pos| {
        cells.push(Decoration::node(
            pos,
            pos + node.node_size(),
            [("class".into(), "selectedCell".into())].into(),
            Value::Null,
        ));
    }).ok()?;
    Some(DecorationSet::create(&state.doc, cells))
}

pub fn normalize_selection(
    state: &EditorState,
    mut tr: Option<Transaction>,
    allow_table_node_selection: bool,
) -> Option<Transaction> {
    let (doc, selection) = if let Some(tr) = &tr {
        (tr.doc(), tr.selection())
    } else {
        (&state.doc, state.selection.clone())
    };
    if let Some(normalized) = normalization(doc, &selection, allow_table_node_selection) {
        tr.get_or_insert_with(|| state.tr()).set_selection(normalized);
    }
    tr
}

fn normalization(doc: &Node, selection: &Selection, allow_table: bool) -> Option<Selection> {
    match selection {
        Selection::Node { anchor } => {
            let selected = selection.selected_node(doc)?;
            match role(&selected)? {
                "cell" | "header_cell" => Selection::cell(doc, *anchor, *anchor).ok(),
                "row" => CellSelection::row_selection(doc, anchor + 1, anchor + 1).ok().map(Selection::Cell),
                "table" if !allow_table => {
                    let map = TableMap::get(&selected).ok()?;
                    let start = anchor + 1;
                    Selection::cell(doc, start + 1, start + map.map.last()?).ok()
                }
                _ => None,
            }
        }
        Selection::Text { .. } => {
            let range = selection.range(doc).ok()?;
            if is_cell_boundary(&range) {
                Some(Selection::cursor(range.from.pos))
            } else if range.to.parent_offset == 0 && cell_boundary(&range.from) != cell_boundary(&range.to) {
                Some(Selection::text(range.from.start(range.from.depth), range.from.end(range.from.depth)))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn is_cell_boundary(range: &SelectionRange) -> bool {
    let SelectionRange { from, to } = range;
    if from.pos == to.pos || to.pos.saturating_sub(from.pos) > 6 {
        return false;
    }
    let mut after_from = from.pos;
    let mut boundary = None;
    for depth in (0..=from.depth).rev() {
        if from.after(depth + 1) < from.end(depth) {
            boundary = role(from.node(depth));
            break;
        }
        after_from += 1;
    }
    let mut before_to = to.pos;
    for depth in (0..=to.depth).rev() {
        if to.before(depth + 1) > to.start(depth) {
            break;
        }
        before_to = before_to.saturating_sub(1);
    }
    after_from == before_to && matches!(boundary, Some("row" | "table"))
}

fn cell_boundary(pos: &ResolvedPos) -> Option<usize> {
    (1..=pos.depth).rev()
        .find(|&depth| matches!(role(pos.node(depth)), Some("cell" | "header_cell")))
        .map(|depth| pos.before(depth))
}

fn role(node: &Node) -> Option<&str> {
    node.node_type()?.spec.table_role.as_deref()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TableEditingOptions {
    pub allow_table_node_selection: bool,
}

struct TableEditingField;

impl StateField for TableEditingField {
    fn init(&self, _: &Value) -> Value {
        Value::Null
    }

    fn apply(&self, tr: &Transaction, current: &Value) -> Value {
        if let Some(set) = tr.get_meta(TABLE_EDITING_KEY).filter(|value| !value.is_null()) {
            return if set.as_i64() == Some(-1) { Value::Null } else { set.clone() };
        }
        if let Some(pos) = current.as_u64().and_then(|pos| usize::try_from(pos).ok()) {
            if tr.doc_changed() {
                let mapped = tr.mapping().map_result(pos, 1);
                return if mapped.deleted() { Value::Null } else { Value::from(mapped.pos) };
            }
        }
        current.clone()
    }
}

pub fn table_editing(options: TableEditingOptions) -> Plugin {
    Plugin::new(
        PluginKey { name: TABLE_EDITING_KEY.into() },
        Some(Arc::new(TableEditingField)),
    ).with_append_transaction(move |transactions, old, state| {
        let changes_tables = match transactions {
            [tr] if !tr.doc_changed() => false,
            [tr] => tr.single_block_replacement().is_none_or(|(index, _)| {
                let block = state.doc.child(index);
                let is_table = |node: &Node| node.node_type()
                    .is_some_and(|kind| kind.spec.table_role.as_deref() == Some("table"));
                let mut contains_table = is_table(block);
                block.descendants(|node, _, _, _| {
                    contains_table |= is_table(node);
                    !contains_table
                });
                contains_table
            }),
            _ => true,
        };
        let fix = changes_tables.then(|| fix_tables(state, Some(old)).ok().flatten()).flatten();
        normalize_selection(state, fix, options.allow_table_node_selection)
    })
}
