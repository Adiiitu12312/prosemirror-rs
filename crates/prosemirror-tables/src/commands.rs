//! Row and column operations from prosemirror-tables@1.8.5/src/commands.ts.

use std::collections::HashSet;

use prosemirror_model::{Fragment, Node, ResolvedPos};
use prosemirror_state::{EditorState, Selection, Transaction};
use serde_json::{json, Map, Value};

use crate::{table_node_types, Rect, TableMap, TableRole};

pub type Dispatch<'a> = Option<&'a mut dyn FnMut(Transaction)>;

#[derive(Clone, Debug)]
pub struct TableRect {
    pub rect: Rect,
    pub table_start: usize,
    pub map: TableMap,
    pub table: Node,
}

impl TableRect {
    pub fn from_cells(doc: &Node, anchor: usize, head: usize) -> Option<Self> {
        let anchor = doc.resolve(anchor).ok()?;
        let head = doc.resolve(head).ok()?;
        let table_depth = anchor.depth.checked_sub(1)?;
        if role(anchor.parent()) != Some("row") || role(head.parent()) != Some("row")
            || anchor.depth != head.depth || anchor.start(table_depth) != head.start(table_depth)
        {
            return None;
        }
        let table = anchor.node(table_depth).clone();
        let table_start = anchor.start(table_depth);
        let map = TableMap::get(&table).ok()?;
        let rect = map.rect_between(anchor.pos - table_start, head.pos - table_start).ok()?;
        Some(Self { rect, table_start, map, table })
    }

    fn refresh(&mut self, tr: &Transaction) -> Option<()> {
        self.table = if self.table_start == 0 { tr.doc() } else { tr.doc().node_at(self.table_start - 1)? }.clone();
        self.map = TableMap::get(&self.table).ok()?;
        Some(())
    }
}

pub fn cell_around(pos: &ResolvedPos) -> Option<ResolvedPos> {
    if role(pos.parent()) == Some("row") && pos.node_after().is_some_and(|node| is_cell(&node)) {
        return Some(pos.clone());
    }
    let depth = (1..pos.depth).rev().find(|&depth| role(pos.node(depth)) == Some("row"))?;
    pos.doc().resolve(pos.before(depth + 1)).ok()
}

pub fn selected_rect(state: &EditorState) -> Option<TableRect> {
    let pos = if let Selection::Node { anchor } = state.selection {
        anchor
    } else {
        state.selection.head(&state.doc)
    };
    let head = state.doc.resolve(pos).ok()?;
    let cell = cell_around(&head)?;
    TableRect::from_cells(&state.doc, cell.pos, cell.pos)
}

pub fn row_is_header(map: &TableMap, table: &Node, row: usize) -> bool {
    (0..map.width).all(|col| at_cell(table, map, row, col).is_some_and(|node| role(node) == Some("header_cell")))
}

pub fn column_is_header(map: &TableMap, table: &Node, col: usize) -> bool {
    (0..map.height).all(|row| at_cell(table, map, row, col).is_some_and(|node| role(node) == Some("header_cell")))
}

pub fn add_column(tr: &mut Transaction, rect: &TableRect, col: usize) -> Option<()> {
    let TableRect { map, table, table_start, .. } = rect;
    if col > map.width {
        return None;
    }
    let schema = table.schema()?;
    let cell_type = table_node_types(schema)[&TableRole::Cell];
    let ref_col = col.saturating_sub(1);
    let ref_col = if column_is_header(map, table, ref_col) {
        (col > 0 && col < map.width).then_some(col)
    } else {
        Some(ref_col)
    };
    let mut row = 0;
    while row < map.height {
        let index = row * map.width + col;
        if col > 0 && col < map.width && map.map[index - 1] == map.map[index] {
            let pos = map.map[index];
            let cell = table.node_at(pos)?;
            let span = map.col_count(pos).ok()?;
            let attrs = change_colspan(cell, col - span, true);
            let mapped = tr.mapping().map(table_start + pos, 1);
            set_attrs(tr, mapped, attrs);
            row += get_span(cell, "rowspan").max(1);
        } else {
            let kind = ref_col.and_then(|reference| at_cell(table, map, row, reference))
                .and_then(Node::node_type).unwrap_or(cell_type);
            let node = kind.create_and_fill(None, None, None).ok()??;
            let pos = map.position_at(row, col, table)?;
            let mapped = tr.mapping().map(table_start + pos, 1);
            tr.insert(mapped, Fragment::from_nodes(vec![node]));
            row += 1;
        }
    }
    Some(())
}

pub fn remove_column(tr: &mut Transaction, rect: &TableRect, col: usize) -> Option<()> {
    let TableRect { map, table, table_start, .. } = rect;
    if col >= map.width {
        return None;
    }
    let map_from = tr.steps().len();
    let mut row = 0;
    while row < map.height {
        let index = row * map.width + col;
        let pos = map.map[index];
        let cell = table.node_at(pos)?;
        let start = tr.mapping().slice(map_from, None).map(table_start + pos, 1);
        if (col > 0 && map.map[index - 1] == pos) || (col + 1 < map.width && map.map[index + 1] == pos) {
            let attrs = change_colspan(cell, col - map.col_count(pos).ok()?, false);
            set_attrs(tr, start, attrs);
        } else {
            tr.delete(start, start + cell.node_size());
        }
        row += get_span(cell, "rowspan").max(1);
    }
    Some(())
}

pub fn add_row(tr: &mut Transaction, rect: &TableRect, row: usize) -> Option<()> {
    let TableRect { map, table, table_start, .. } = rect;
    if row > map.height {
        return None;
    }
    let schema = table.schema()?;
    let kinds = table_node_types(schema);
    let row_pos = table_start + row_offset(table, row);
    let ref_row = row.saturating_sub(1);
    let ref_row = if row_is_header(map, table, ref_row) {
        (row > 0 && row < map.height).then_some(row)
    } else {
        Some(ref_row)
    };
    let mut cells = Vec::new();
    let mut col = 0;
    while col < map.width {
        let index = row * map.width + col;
        if row > 0 && row < map.height && map.map[index] == map.map[index - map.width] {
            let pos = map.map[index];
            let cell = table.node_at(pos)?;
            let mut attrs = cell_attrs(cell);
            attrs.insert("rowspan".into(), json!(get_span(cell, "rowspan") + 1));
            set_attrs(tr, table_start + pos, attrs);
            col += get_span(cell, "colspan").max(1);
        } else {
            let kind = match ref_row {
                Some(reference) => at_cell(table, map, reference, col).and_then(Node::node_type),
                None => kinds.get(&TableRole::Cell).copied(),
            };
            if let Some(cell) = kind.and_then(|kind| kind.create_and_fill(None, None, None).ok().flatten()) {
                cells.push(cell);
            }
            col += 1;
        }
    }
    let row = kinds.get(&TableRole::Row)?.try_create(None, Some(Fragment::from_nodes(cells)), None).ok()?;
    tr.insert(row_pos, Fragment::from_nodes(vec![row]));
    Some(())
}

pub fn remove_row(tr: &mut Transaction, rect: &TableRect, row: usize) -> Option<()> {
    let TableRect { map, table, table_start, .. } = rect;
    let node = table.content().children.get(row)?;
    let start = table_start + row_offset(table, row);
    let map_from = tr.steps().len();
    tr.delete(start, start + node.node_size());
    let mut seen = HashSet::new();
    for col in 0..map.width {
        let index = row * map.width + col;
        let pos = map.map[index];
        if !seen.insert(pos) {
            continue;
        }
        let cell = table.node_at(pos)?;
        let above = row > 0 && pos == map.map[index - map.width];
        let below = row + 1 < map.height && pos == map.map[index + map.width];
        if above || below {
            let mut attrs = cell_attrs(cell);
            attrs.insert("rowspan".into(), json!(get_span(cell, "rowspan").saturating_sub(1)));
            if above {
                let mapped = tr.mapping().slice(map_from, None).map(table_start + pos, 1);
                set_attrs(tr, mapped, attrs);
            } else {
                let moved = cell.node_type()?.try_create(Some(Value::Object(attrs)), Some(cell.content().clone()), None).ok()?;
                let new_pos = map.position_at(row + 1, col, table)?;
                let mapped = tr.mapping().slice(map_from, None).map(table_start + new_pos, 1);
                tr.insert(mapped, Fragment::from_nodes(vec![moved]));
            }
        }
    }
    Some(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableEdit {
    AddColumnBefore,
    AddColumnAfter,
    DeleteColumns,
    AddRowBefore,
    AddRowAfter,
    DeleteRows,
}

impl TableEdit {
    pub fn apply(self, mut tr: Transaction, mut rect: TableRect) -> Option<Transaction> {
        match self {
            Self::AddColumnBefore => add_column(&mut tr, &rect, rect.rect.left)?,
            Self::AddColumnAfter => add_column(&mut tr, &rect, rect.rect.right)?,
            Self::AddRowBefore => add_row(&mut tr, &rect, rect.rect.top)?,
            Self::AddRowAfter => add_row(&mut tr, &rect, rect.rect.bottom)?,
            Self::DeleteColumns => {
                if rect.rect.left == 0 && rect.rect.right == rect.map.width {
                    return None;
                }
                for col in (rect.rect.left..rect.rect.right).rev() {
                    remove_column(&mut tr, &rect, col)?;
                    if col != rect.rect.left {
                        rect.refresh(&tr)?;
                    }
                }
            }
            Self::DeleteRows => {
                if rect.rect.top == 0 && rect.rect.bottom == rect.map.height {
                    return None;
                }
                for row in (rect.rect.top..rect.rect.bottom).rev() {
                    remove_row(&mut tr, &rect, row)?;
                    if row != rect.rect.top {
                        rect.refresh(&tr)?;
                    }
                }
            }
        }
        Some(tr)
    }

    pub fn run(self, state: &EditorState, dispatch: Dispatch<'_>) -> bool {
        let Some(rect) = selected_rect(state) else { return false };
        let Some(dispatch) = dispatch else { return true };
        let Some(tr) = self.apply(state.tr(), rect) else { return false };
        dispatch(tr);
        true
    }
}

fn at_cell<'a>(table: &'a Node, map: &TableMap, row: usize, col: usize) -> Option<&'a Node> {
    table.node_at(*map.map.get(row * map.width + col)?)
}

fn role(node: &Node) -> Option<&str> {
    node.node_type()?.spec.table_role.as_deref()
}

fn is_cell(node: &Node) -> bool {
    matches!(role(node), Some("cell" | "header_cell"))
}

fn row_offset(table: &Node, row: usize) -> usize {
    table.content().children[..row].iter().map(Node::node_size).sum()
}

fn get_span(node: &Node, name: &str) -> usize {
    node.attrs().and_then(|attrs| attrs[name].as_u64()).and_then(|value| value.try_into().ok()).unwrap_or(1)
}

fn cell_attrs(cell: &Node) -> Map<String, Value> {
    cell.attrs().and_then(Value::as_object).cloned().unwrap_or_default()
}

fn change_colspan(cell: &Node, col: usize, adding: bool) -> Map<String, Value> {
    let mut attrs = cell_attrs(cell);
    let colspan = get_span(cell, "colspan");
    attrs.insert("colspan".into(), json!(if adding { colspan + 1 } else { colspan - 1 }));
    if let Some(widths) = attrs.get_mut("colwidth").and_then(Value::as_array_mut) {
        if adding {
            widths.insert(col.min(widths.len()), json!(0));
        } else if col < widths.len() {
            widths.remove(col);
            if !widths.iter().any(|width| width.as_f64().is_some_and(|width| width > 0.0)) {
                attrs.insert("colwidth".into(), Value::Null);
            }
        }
    }
    attrs
}

fn set_attrs(tr: &mut Transaction, pos: usize, attrs: Map<String, Value>) {
    tr.apply_transform(|transform| { transform.set_node_markup(pos, None, Some(Value::Object(attrs)), None); });
}
