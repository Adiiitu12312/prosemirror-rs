//! Cell, header, navigation, and deletion commands from prosemirror-tables 1.8.5 commands.ts.

use std::collections::HashSet;

use prosemirror_model::{Fragment, Node, NodeType, ResolvedPos, Schema, Slice};
use prosemirror_state::{EditorState, Selection, Transaction};
use serde_json::{json, Value};

use crate::commands::{
    cell_around, cell_attrs as attrs, get_span as span, role, selected_rect, Dispatch, TableRect,
};
use crate::{table_node_types, Rect, TableMap, TableRole};

fn set_markup(tr: &mut Transaction, pos: usize, kind: Option<&NodeType>, attrs: Option<Value>) -> Option<()> {
    let before = tr.steps().len();
    tr.apply_transform(|transform| {
        transform.set_node_markup(pos, kind.map(|kind| kind.name.as_str()), attrs, None);
    });
    (tr.steps().len() > before).then_some(())
}

fn in_table(state: &EditorState) -> bool {
    state.doc.resolve(state.selection.head(&state.doc)).is_ok_and(|head| {
        (1..=head.depth).rev().any(|depth| role(head.node(depth)) == Some("row"))
    })
}

fn selection_cell(state: &EditorState) -> Option<ResolvedPos> {
    if let Selection::Cell(selection) = &state.selection {
        return state.doc.resolve(selection.anchor.max(selection.head)).ok();
    }
    if let Selection::Node { anchor } = state.selection {
        if state.doc.node_at(anchor).is_some_and(|node| role(node) == Some("cell")) {
            return state.doc.resolve(anchor).ok();
        }
    }
    let head = state.doc.resolve(state.selection.head(&state.doc)).ok()?;
    if let Some(cell) = cell_around(&head) {
        return Some(cell);
    }
    let mut pos = head.pos;
    let next = head.node_after();
    let mut next = next.as_ref();
    while let Some(node) = next {
        if matches!(role(node), Some("cell" | "header_cell")) {
            return state.doc.resolve(pos).ok();
        }
        pos += 1;
        next = node.first_child();
    }
    let mut pos = head.pos;
    let previous = head.node_before();
    let mut previous = previous.as_ref();
    while let Some(node) = previous {
        if matches!(role(node), Some("cell" | "header_cell")) {
            return state.doc.resolve(pos.checked_sub(node.node_size())?).ok();
        }
        pos = pos.checked_sub(1)?;
        previous = node.last_child();
    }
    None
}

fn overlaps_rectangle(map: &TableMap, rect: Rect) -> bool {
    let slot = |row: usize, col: usize| map.map[row * map.width + col];
    (rect.top..rect.bottom).any(|row| {
        (rect.left > 0 && slot(row, rect.left) == slot(row, rect.left - 1))
            || (rect.right < map.width && slot(row, rect.right - 1) == slot(row, rect.right))
    }) || (rect.left..rect.right).any(|col| {
        (rect.top > 0 && slot(rect.top, col) == slot(rect.top - 1, col))
            || (rect.bottom < map.height && slot(rect.bottom - 1, col) == slot(rect.bottom, col))
    })
}

fn empty_cell(node: &Node) -> bool {
    node.child_count() == 1 && node.child(0).is_textblock() && node.child(0).child_count() == 0
}

/// Merge a rectangular cell selection and retain the top-left cell's attributes.
pub fn merge_cells(state: &EditorState, dispatch: Dispatch<'_>) -> bool {
    let Selection::Cell(selection) = &state.selection else { return false };
    if selection.anchor == selection.head {
        return false;
    }
    let Some(rect) = TableRect::from_cells(&state.doc, selection.anchor, selection.head) else {
        return false;
    };
    if overlaps_rectangle(&rect.map, rect.rect) {
        return false;
    }
    let Some(dispatch) = dispatch else { return true };
    let Some(tr) = merge_cells_transaction(state.tr(), &rect) else { return false };
    dispatch(tr);
    true
}

fn merge_cells_transaction(mut tr: Transaction, table: &TableRect) -> Option<Transaction> {
    let TableRect { rect, map, table_start, table } = table;
    let mut positions = (rect.top..rect.bottom).flat_map(|row| {
        (rect.left..rect.right).map(move |col| map.map[row * map.width + col])
    });
    let first_pos = positions.next()?;
    let first_cell = table.node_at(first_pos)?;
    let mut seen = HashSet::from([first_pos]);
    let mut content = Fragment::empty();
    for position in positions {
        if !seen.insert(position) {
            continue;
        }
        if let Some(cell) = table.node_at(position) {
            if !empty_cell(cell) {
                content = content.append(cell.content());
            }
            let from = tr.mapping().map(table_start + position, 1);
            tr.delete(from, from + cell.node_size());
        }
    }

    let mut next_attrs = attrs(first_cell);
    let width = rect.right - rect.left;
    let first_width = span(first_cell, "colspan");
    next_attrs.insert("colspan".into(), json!(width));
    next_attrs.insert("rowspan".into(), json!(rect.bottom - rect.top));
    if let Some(widths) = next_attrs.get_mut("colwidth").and_then(Value::as_array_mut) {
        let at = first_width.min(widths.len());
        widths.splice(at..at, std::iter::repeat_n(json!(0), width.saturating_sub(first_width)));
    }
    let position = first_pos + table_start;
    set_markup(&mut tr, position, None, Some(Value::Object(next_attrs)))?;
    if content.size() > 0 {
        let end = position + 1 + first_cell.content().size();
        let start = if empty_cell(first_cell) { position + 1 } else { end };
        tr.replace_with(start, end, content);
    }
    let selection = Selection::cell(tr.doc(), position, position).ok()?;
    tr.set_selection(selection);
    Some(tr)
}

pub struct GetCellTypeOptions<'a> {
    pub node: &'a Node,
    pub row: usize,
    pub col: usize,
    pub schema: &'a Schema,
}

pub fn split_cell(state: &EditorState, dispatch: Dispatch<'_>) -> bool {
    split_cell_with_type(|options| {
        let role = if role(options.node) == Some("header_cell") {
            TableRole::HeaderCell
        } else {
            TableRole::Cell
        };
        table_node_types(options.schema).get(&role).copied()
    })(state, dispatch)
}

/// Split a spanning cell, using the given callback to choose each new cell's node type.
pub fn split_cell_with_type(
    get_type: impl for<'a> Fn(GetCellTypeOptions<'a>) -> Option<&'a NodeType>,
) -> impl Fn(&EditorState, Dispatch<'_>) -> bool {
    move |state: &EditorState, dispatch: Dispatch<'_>| {
        let (position, cell_selection) = match &state.selection {
            Selection::Cell(selection) if selection.anchor != selection.head => return false,
            Selection::Cell(selection) => (selection.anchor, true),
            _ => {
                let Ok(from) = state.doc.resolve(state.selection.from()) else { return false };
                if !(1..=from.depth).rev().any(|depth| matches!(role(from.node(depth)), Some("cell" | "header_cell"))) {
                    return false;
                }
                let Some(cell) = cell_around(&from) else { return false };
                (cell.pos, false)
            }
        };
        let Some(cell) = state.doc.node_at(position) else { return false };
        if span(cell, "colspan") == 1 && span(cell, "rowspan") == 1 {
            return false;
        }
        let Some(dispatch) = dispatch else { return true };
        let Some(table) = selected_rect(state) else { return false };
        let mut tr = state.tr();
        let Some(last_cell) = split_cell_transaction(&mut tr, position, cell, table, &state.schema, &get_type) else {
            return false;
        };
        if cell_selection {
            let Ok(selection) = Selection::cell(tr.doc(), position, last_cell) else { return false };
            tr.set_selection(selection);
        }
        dispatch(tr);
        true
    }
}

fn split_cell_transaction(
    tr: &mut Transaction,
    position: usize,
    cell: &Node,
    table: TableRect,
    schema: &Schema,
    get_type: &impl for<'a> Fn(GetCellTypeOptions<'a>) -> Option<&'a NodeType>,
) -> Option<usize> {
    let TableRect { rect, map, table_start, table } = table;
    let mut base = attrs(cell);
    let colwidth = base.get("colwidth").and_then(Value::as_array).cloned();
    base.insert("colspan".into(), json!(1));
    base.insert("rowspan".into(), json!(1));
    let cell_attrs: Vec<_> = (0..rect.right - rect.left).map(|col| {
        let mut attrs = base.clone();
        if let Some(widths) = &colwidth {
            let width = widths.get(col).filter(|value| value.as_f64().is_some_and(|width| width != 0.0));
            attrs.insert("colwidth".into(), width.map_or(Value::Null, |width| json!([width])));
        }
        Value::Object(attrs)
    }).collect();
    let mut last = position;
    for row in rect.top..rect.bottom {
        let mut at = map.position_at(row, rect.left, &table)? + table_start;
        if row == rect.top {
            at += cell.node_size();
        }
        for col in rect.left..rect.right {
            if row == rect.top && col == rect.left {
                continue;
            }
            let ty = get_type(GetCellTypeOptions { node: cell, row, col, schema })?;
            let node = ty.create_and_fill(Some(cell_attrs[col - rect.left].clone()), None, None).ok()??;
            last = tr.mapping().map(at, 1);
            tr.insert(last, Fragment::from_nodes(vec![node]));
        }
    }
    let top_left = get_type(GetCellTypeOptions { node: cell, row: rect.top, col: rect.left, schema })?;
    set_markup(tr, position, Some(top_left), Some(cell_attrs.first()?.clone()))?;
    Some(last)
}

pub fn set_cell_attr(name: impl Into<String>, value: Value) -> impl Fn(&EditorState, Dispatch<'_>) -> bool {
    let name = name.into();
    move |state: &EditorState, dispatch: Dispatch<'_>| {
        if !in_table(state) {
            return false;
        }
        let Some(cell_pos) = selection_cell(state) else { return false };
        let Some(cell) = state.doc.node_at(cell_pos.pos) else { return false };
        let current = |node: &Node| node.attrs().and_then(|attrs| attrs.get(&name)) == Some(&value);
        if current(cell) {
            return false;
        }
        let Some(dispatch) = dispatch else { return true };
        let mut tr = state.tr();
        let mut set = |cell: &Node, pos| {
            if !current(cell) {
                let mut attrs = attrs(cell);
                attrs.insert(name.clone(), value.clone());
                set_markup(&mut tr, pos, None, Some(Value::Object(attrs)));
            }
        };
        if let Selection::Cell(selection) = &state.selection {
            if selection.for_each_cell(&state.doc, &mut set).is_err() {
                return false;
            }
        } else {
            set(cell, cell_pos.pos);
        }
        dispatch(tr);
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToggleHeaderType {
    Row,
    Column,
    Cell,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToggleHeaderOptions {
    pub use_deprecated_logic: bool,
}

pub fn toggle_header(
    kind: ToggleHeaderType,
    options: ToggleHeaderOptions,
) -> impl Fn(&EditorState, Dispatch<'_>) -> bool {
    move |state: &EditorState, dispatch: Dispatch<'_>| {
        if !in_table(state) {
            return false;
        }
        let Some(dispatch) = dispatch else { return true };
        let Some(table) = selected_rect(state) else { return false };
        let TableRect { rect, map, table_start, table } = table;
        let types = table_node_types(&state.schema);
        let (Some(&cell), Some(&header)) = (types.get(&TableRole::Cell), types.get(&TableRole::HeaderCell)) else {
            return false;
        };
        let mut tr = state.tr();
        if options.use_deprecated_logic {
            let cells = map.cells_in_rect(match kind {
                ToggleHeaderType::Column => Rect { top: 0, bottom: map.height, ..rect },
                ToggleHeaderType::Row => Rect { left: 0, right: map.width, ..rect },
                ToggleHeaderType::Cell => rect,
            });
            let headers: Vec<_> = cells.iter().filter_map(|&pos| {
                table.node_at(pos).filter(|node| role(node) == Some("header_cell"))
                    .map(|node| (pos, node))
            }).collect();
            if headers.is_empty() {
                for pos in cells {
                    if let Some(node) = table.node_at(pos) {
                        set_markup(&mut tr, table_start + pos, Some(header), node.attrs().cloned());
                    }
                }
            } else {
                for (pos, node) in headers {
                    set_markup(&mut tr, table_start + pos, Some(cell), node.attrs().cloned());
                }
            }
        } else {
            let all_headers = |rect: Rect| {
                map.cells_in_rect(rect).into_iter().all(|pos| {
                    table.node_at(pos).is_none_or(|node| role(node) == Some("header_cell"))
                })
            };
            let row_enabled = all_headers(Rect { left: 0, top: 0, right: map.width, bottom: 1 });
            let column_enabled = all_headers(Rect { left: 0, top: 0, right: 1, bottom: map.height });
            let (cells, enable) = match kind {
                ToggleHeaderType::Column => (
                    Rect { left: 0, top: usize::from(row_enabled), right: 1, bottom: map.height },
                    !column_enabled,
                ),
                ToggleHeaderType::Row => (
                    Rect { left: usize::from(column_enabled), top: 0, right: map.width, bottom: 1 },
                    !row_enabled,
                ),
                ToggleHeaderType::Cell => (rect, false),
            };
            let ty = if enable { header } else { cell };
            for pos in map.cells_in_rect(cells) {
                if let Some(node) = tr.doc().node_at(table_start + pos) {
                    let attrs = node.attrs().cloned();
                    set_markup(&mut tr, table_start + pos, Some(ty), attrs);
                }
            }
        }
        dispatch(tr);
        true
    }
}

pub fn toggle_header_row(state: &EditorState, dispatch: Dispatch<'_>) -> bool {
    toggle_header(ToggleHeaderType::Row, ToggleHeaderOptions { use_deprecated_logic: true })(state, dispatch)
}

pub fn toggle_header_column(state: &EditorState, dispatch: Dispatch<'_>) -> bool {
    toggle_header(ToggleHeaderType::Column, ToggleHeaderOptions { use_deprecated_logic: true })(state, dispatch)
}

pub fn toggle_header_cell(state: &EditorState, dispatch: Dispatch<'_>) -> bool {
    toggle_header(ToggleHeaderType::Cell, ToggleHeaderOptions { use_deprecated_logic: true })(state, dispatch)
}

fn next_cell(cell: &ResolvedPos, direction: i32) -> Option<usize> {
    let table_depth = cell.depth.checked_sub(1)?;
    let table = cell.node(table_depth);
    let row = cell.index(table_depth);
    if direction < 0 {
        if let Some(before) = cell.node_before() {
            return cell.pos.checked_sub(before.node_size());
        }
        let mut row_end = cell.before(cell.depth);
        for node in table.content().children[..row].iter().rev() {
            if let Some(last) = node.last_child() {
                return row_end.checked_sub(1 + last.node_size());
            }
            row_end = row_end.checked_sub(node.node_size())?;
        }
    } else {
        let row_node = cell.parent();
        if cell.index(cell.depth) + 1 < row_node.child_count() {
            return Some(cell.pos + cell.node_after()?.node_size());
        }
        let mut row_start = cell.after(cell.depth);
        for node in &table.content().children[row + 1..] {
            if node.child_count() > 0 {
                return Some(row_start + 1);
            }
            row_start += node.node_size();
        }
    }
    None
}

pub fn go_to_next_cell(direction: i32) -> impl Fn(&EditorState, Dispatch<'_>) -> bool {
    move |state: &EditorState, dispatch: Dispatch<'_>| {
        if !in_table(state) {
            return false;
        }
        let Some(cell) = selection_cell(state) else { return false };
        let Some(position) = next_cell(&cell, direction) else { return false };
        if let Some(dispatch) = dispatch {
            let Some(node) = state.doc.node_at(position) else { return false };
            let (Ok(from), Ok(to)) = (state.doc.resolve(position), state.doc.resolve(position + node.node_size())) else {
                return false;
            };
            let mut tr = state.tr();
            tr.set_selection(Selection::between(&from, &to, 0)).scroll_into_view();
            dispatch(tr);
        }
        true
    }
}

pub fn delete_table(state: &EditorState, dispatch: Dispatch<'_>) -> bool {
    let Ok(anchor) = state.doc.resolve(state.selection.anchor()) else { return false };
    let Some(depth) = (1..=anchor.depth).rev().find(|&depth| role(anchor.node(depth)) == Some("table")) else {
        return false;
    };
    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        tr.delete(anchor.before(depth), anchor.after(depth)).scroll_into_view();
        dispatch(tr);
    }
    true
}

pub fn delete_cell_selection(state: &EditorState, dispatch: Dispatch<'_>) -> bool {
    let Selection::Cell(selection) = &state.selection else { return false };
    let Some(dispatch) = dispatch else { return true };
    let types = table_node_types(&state.schema);
    let Some(base) = types.get(&TableRole::Cell)
        .and_then(|ty| ty.create_and_fill(None, None, None).ok().flatten())
    else {
        return false;
    };
    let content = base.content();
    let mut tr = state.tr();
    if selection.for_each_cell(&state.doc, |node, pos| {
        if node.content() != content {
            let from = tr.mapping().map(pos + 1, 1);
            let to = tr.mapping().map(pos + node.node_size() - 1, 1);
            tr.replace(from, to, Slice::new(content.clone(), 0, 0));
        }
    }).is_err() {
        return false;
    }
    if tr.doc_changed() {
        dispatch(tr);
    }
    true
}

#[derive(Clone, Copy, Debug)]
pub struct MoveTableOptions {
    pub from: usize,
    pub to: usize,
    pub select: bool,
    pub pos: Option<usize>,
}

impl MoveTableOptions {
    pub fn new(from: usize, to: usize) -> Self {
        Self { from, to, select: true, pos: None }
    }
}

pub fn move_table_row(options: MoveTableOptions) -> impl Fn(&EditorState, Dispatch<'_>) -> bool {
    move |state: &EditorState, dispatch: Dispatch<'_>| move_table_axis(state, dispatch, options, false)
}

pub fn move_table_column(options: MoveTableOptions) -> impl Fn(&EditorState, Dispatch<'_>) -> bool {
    move |state: &EditorState, dispatch: Dispatch<'_>| move_table_axis(state, dispatch, options, true)
}

fn table_depth(pos: &ResolvedPos) -> Option<usize> {
    (0..=pos.depth).rev().find(|&depth| role(pos.node(depth)) == Some("table"))
}

fn move_table_axis(
    state: &EditorState,
    dispatch: Dispatch<'_>,
    options: MoveTableOptions,
    column: bool,
) -> bool {
    let Some(tr) = move_table_transaction(state, options, column) else { return false };
    if let Some(dispatch) = dispatch {
        dispatch(tr);
    }
    true
}

fn move_table_transaction(state: &EditorState, options: MoveTableOptions, column: bool) -> Option<Transaction> {
    use crate::utils::{
        convert_array_of_rows_to_table_node, convert_table_node_to_array_of_rows,
        move_row_in_array_of_rows, transpose,
    };
    use prosemirror_state::CellSelection;

    let source = state.doc.resolve(state.selection.from()).ok()?;
    let source_table = source.node(table_depth(&source)?);
    let source_map = TableMap::get(source_table).ok()?;
    let origin = spanning_indexes(source_table, &source_map, options.from, column)?;
    let target = spanning_indexes(source_table, &source_map, options.to, column)?;
    if origin.contains(&options.to) {
        return None;
    }
    let position = state.doc.resolve(options.pos.unwrap_or(source.pos)).ok()?;
    let depth = table_depth(&position)?;
    if depth == 0 {
        return None;
    }
    let table = position.node(depth);
    let mut rows = convert_table_node_to_array_of_rows(table).ok()?;
    if column {
        rows = transpose(&rows);
    }
    move_row_in_array_of_rows(&mut rows, &origin, &target, 0);
    if column {
        rows = transpose(&rows);
    }
    let updated = convert_array_of_rows_to_table_node(table, &rows).ok()?;
    let mut tr = state.tr();
    let from = position.before(depth);
    tr.replace_with(from, from + table.node_size(), Fragment::from_nodes(vec![updated.clone()]));
    if !tr.doc_changed() {
        return None;
    }
    if options.select {
        let map = TableMap::get(&updated).ok()?;
        let start = position.start(depth);
        let (first, last) = if column {
            let last_row = map.height.checked_sub(1)?;
            (map.position_at(0, options.to, &updated)?, map.position_at(last_row, options.to, &updated)?)
        } else {
            let last_col = map.width.checked_sub(1)?;
            (map.position_at(options.to, 0, &updated)?, map.position_at(options.to, last_col, &updated)?)
        };
        let selection = if column {
            CellSelection::col_selection(tr.doc(), start + last, start + first)
        } else {
            CellSelection::row_selection(tr.doc(), start + last, start + first)
        }.ok()?;
        tr.set_selection(Selection::Cell(selection));
    }
    Some(tr)
}

fn stripe(map: &TableMap, index: usize, column: bool) -> Rect {
    if column {
        Rect { left: index, top: 0, right: index + 1, bottom: map.height }
    } else {
        Rect { left: 0, top: index, right: map.width, bottom: index + 1 }
    }
}

// The span expansion in the tagged selection-range.ts is based on
// Atlassian's Apache-2.0 get-selection-range-in-row and -in-column utilities.
fn spanning_indexes(table: &Node, map: &TableMap, index: usize, column: bool) -> Option<Vec<usize>> {
    let (count, span_attr) = if column { (map.width, "colspan") } else { (map.height, "rowspan") };
    if index >= count {
        return None;
    }
    let cells = |index| map.cells_in_rect(stripe(map, index, column));
    let span_end = |pos: usize, index: usize| {
        table.node_at(pos).map_or(index, |cell| index.saturating_add(span(cell, span_attr).saturating_sub(1)))
            .min(count - 1)
    };
    let (mut start, mut end) = (index, index);
    for i in (0..=index).rev() {
        for pos in cells(i) {
            let candidate = span_end(pos, i);
            if candidate >= start {
                start = i;
            }
            end = end.max(candidate);
        }
    }
    let mut i = index;
    while i <= end {
        for pos in cells(i) {
            end = end.max(span_end(pos, i));
        }
        i += 1;
    }
    let first_orthogonal = map.cells_in_rect(stripe(map, 0, !column));
    let indexes: Vec<_> = (start..=end).filter(|&i| !cells(i).is_empty()).collect();
    indexes.iter().rev().any(|&i| cells(i).first().is_some_and(|pos| first_orthogonal.contains(pos)))
        .then_some(indexes)
}
