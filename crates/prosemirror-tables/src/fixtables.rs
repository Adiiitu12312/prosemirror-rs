//! Table normalization from prosemirror-tables@1.8.5/src/fixtables.ts.

use prosemirror_model::{Fragment, Node};
use prosemirror_state::{EditorState, Transaction};
use serde_json::{json, Map, Value};

use crate::{table_node_types, Problem, TableMap, TableMapError, TableRole};

pub const FIX_TABLES_KEY: &str = "fix-tables";

pub fn fix_tables(
    state: &EditorState, old_state: Option<&EditorState>,
) -> Result<Option<Transaction>, TableMapError> {
    let mut result = Ok(None);
    let mut check = |node: &Node, pos| {
        if node.node_type().is_some_and(|kind| kind.spec.table_role.as_deref() == Some("table")) {
            if let Ok(tr) = &mut result {
                result = fix_table(state, node, pos, tr.take());
            }
        }
        result.is_ok()
    };
    if let Some(old) = old_state {
        if old.doc != state.doc {
            changed_descendants(&old.doc, &state.doc, 0, &mut check);
        }
    } else {
        state.doc.descendants(|node, pos, _, _| check(node, pos));
    }
    result
}

fn changed_descendants(
    old: &Node, current: &Node, mut offset: usize, visit: &mut impl FnMut(&Node, usize) -> bool,
) {
    let mut previous = 0;
    for (index, child) in current.content().children.iter().enumerate() {
        let limit = old.child_count().min(index + 3);
        if let Some(found) = (previous..limit).find(|&scan| old.child(scan) == child) {
            previous = found + 1;
            offset += child.node_size();
            continue;
        }
        if visit(child, offset) {
            if let Some(before) = old.content().children.get(previous).filter(|before| before.same_markup(child)) {
                changed_descendants(before, child, offset + 1, visit);
            } else {
                child.descendants(|node, pos, _, _| visit(node, offset + 1 + pos));
            }
        }
        offset += child.node_size();
    }
}

fn fix_table(
    state: &EditorState, table: &Node, table_pos: usize, tr: Option<Transaction>,
) -> Result<Option<Transaction>, TableMapError> {
    let map = TableMap::get(table)?;
    if map.problems.is_empty() {
        return Ok(tr);
    }
    let mut tr = tr.unwrap_or_else(|| state.tr());
    let mut must_add = vec![0; map.height];
    for problem in map.problems {
        match problem {
            Problem::Collision { pos, row, n } => {
                let Some(cell) = table.node_at(pos) else { continue };
                let mut attrs = cell_attributes(cell);
                let rowspan = read_span(&attrs, "rowspan");
                let colspan = read_span(&attrs, "colspan");
                let end = row.saturating_add(rowspan).min(must_add.len());
                for add in &mut must_add[row..end] {
                    *add += n;
                }
                let remaining = colspan.saturating_sub(n);
                attrs.insert("colspan".into(), json!(remaining));
                if let Some(widths) = attrs.get_mut("colwidth").and_then(Value::as_array_mut) {
                    widths.truncate(remaining);
                    if !widths.iter().any(|width| width.as_f64().is_some_and(|width| width > 0.0)) {
                        attrs.insert("colwidth".into(), Value::Null);
                    }
                }
                update_cell(&mut tr, table_pos, pos, attrs);
            }
            Problem::Missing { row, n } => must_add[row] += n,
            Problem::OverlongRowspan { pos, n } => {
                let Some(cell) = table.node_at(pos) else { continue };
                let mut attrs = cell_attributes(cell);
                let rowspan = read_span(&attrs, "rowspan");
                attrs.insert("rowspan".into(), json!(rowspan.saturating_sub(n)));
                update_cell(&mut tr, table_pos, pos, attrs);
            }
            Problem::ColwidthMismatch { pos, colwidth } => {
                let Some(cell) = table.node_at(pos) else { continue };
                let mut attrs = cell_attributes(cell);
                attrs.insert("colwidth".into(), json!(colwidth));
                update_cell(&mut tr, table_pos, pos, attrs);
            }
            Problem::ZeroSized => {
                let pos = tr.mapping().map(table_pos, 1);
                tr.delete(pos, pos + table.node_size());
            }
        }
    }
    let first = must_add.iter().position(|&count| count != 0);
    let last = must_add.iter().rposition(|&count| count != 0);
    let cell_types = table_node_types(&state.schema);
    let mut pos = table_pos + 1;
    for (index, (row, count)) in table.content().children.iter().zip(must_add).enumerate() {
        let end = pos + row.node_size();
        if count > 0 {
            let role = row.content().children.first().and_then(Node::node_type)
                .and_then(|kind| kind.spec.table_role.as_deref());
            let role = if role == Some("header_cell") { TableRole::HeaderCell } else { TableRole::Cell };
            if let Some(cell) = cell_types.get(&role).and_then(|kind| kind.create_and_fill(None, None, None).ok().flatten()) {
                let side = if (index == 0 || first == index.checked_sub(1)) && last == Some(index) { pos + 1 } else { end - 1 };
                let pos = tr.mapping().map(side, 1);
                tr.insert(pos, Fragment::from_nodes(vec![cell; count]));
            }
        }
        pos = end;
    }
    tr.set_meta(FIX_TABLES_KEY, json!({"fixTables":true}));
    Ok(Some(tr))
}

fn update_cell(tr: &mut Transaction, table_pos: usize, cell_pos: usize, attrs: Map<String, Value>) {
    let pos = tr.mapping().map(table_pos + 1 + cell_pos, 1);
    tr.apply_transform(|transform| { transform.set_node_markup(pos, None, Some(Value::Object(attrs)), None); });
}

fn cell_attributes(cell: &Node) -> Map<String, Value> {
    cell.attrs().and_then(Value::as_object).cloned().unwrap_or_default()
}

fn read_span(attrs: &Map<String, Value>, name: &str) -> usize {
    attrs.get(name).and_then(Value::as_u64).and_then(|value| value.try_into().ok()).unwrap_or(1)
}
