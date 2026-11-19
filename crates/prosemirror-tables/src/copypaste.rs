//! Clipboard table areas from prosemirror-tables@1.8.5/src/copypaste.ts.

use prosemirror_model::{Fragment, Node, NodeType, Schema, Slice};
use prosemirror_state::{EditorState, Selection, Transaction};
use prosemirror_transform::Transform;
use serde_json::{json, Value};

use crate::commands::{cell_attrs, get_span, remove_col_span, role, set_attrs};
use crate::{table_node_types, Rect, TableMap, TableRole};

#[derive(Clone, Debug, PartialEq)]
pub struct Area {
    pub width: usize,
    pub height: usize,
    pub rows: Vec<Fragment>,
}

pub fn pasted_cells(slice: &Slice) -> Option<Area> {
    if slice.size() == 0 {
        return None;
    }
    let mut content = &slice.content;
    let (mut start, mut end) = (slice.open_start, slice.open_end);
    while content.child_count() == 1 && ((start > 0 && end > 0) || role(content.child(0)) == Some("table")) {
        start = start.saturating_sub(1);
        end = end.saturating_sub(1);
        content = content.child(0).content();
    }
    let first = content.children.first()?;
    let schema = first.schema()?;
    let kinds = table_node_types(schema);
    let mut rows = Vec::new();
    match role(first)? {
        "row" => {
            for (index, node) in content.children.iter().enumerate() {
                let left = if index == 0 { start.saturating_sub(1) } else { 0 };
                let right = if index + 1 == content.child_count() { end.saturating_sub(1) } else { 0 };
                let cells = node.content().clone();
                if left > 0 || right > 0 {
                    let row = fit_slice(kinds.get(&TableRole::Row)?, Slice::new(cells, left, right))?;
                    rows.push(row.content().clone());
                } else {
                    rows.push(cells);
                }
            }
        }
        "cell" | "header_cell" => {
            if start > 0 || end > 0 {
                let row = fit_slice(kinds.get(&TableRole::Row)?, Slice::new(content.clone(), start, end))?;
                rows.push(row.content().clone());
            } else {
                rows.push(content.clone());
            }
        }
        _ => return None,
    }
    ensure_rectangular(schema, rows)
}

fn ensure_rectangular(schema: &Schema, mut rows: Vec<Fragment>) -> Option<Area> {
    let mut widths = vec![0usize; rows.len()];
    for (index, row) in rows.iter().enumerate() {
        for cell in row.children.iter().rev() {
            let end = index.checked_add(get_span(cell, "rowspan"))?;
            if widths.len() < end {
                widths.try_reserve(end - widths.len()).ok()?;
                widths.resize(end, 0);
            }
            for width in &mut widths[index..end] {
                *width = width.checked_add(get_span(cell, "colspan"))?;
            }
        }
    }
    let width = widths.iter().copied().max().unwrap_or(0);
    let cell = table_node_types(schema).get(&TableRole::Cell)?.create_and_fill(None, None, None).ok()??;
    rows.resize_with(widths.len(), Fragment::empty);
    for (row, row_width) in rows.iter_mut().zip(widths) {
        if row_width < width {
            *row = row.append(&Fragment::from_nodes(vec![cell.clone(); width - row_width]));
        }
    }
    Some(Area { width, height: rows.len(), rows })
}

pub fn fit_slice(kind: &NodeType, slice: Slice) -> Option<Node> {
    let node = kind.create_and_fill(None, None, None).ok()??;
    let mut tr = Transform::new(node);
    tr.replace(0, tr.doc.content().size(), slice);
    Some(tr.doc)
}

pub fn clip_cells(mut area: Area, new_width: usize, new_height: usize) -> Option<Area> {
    if area.width != new_width {
        let mut covered = vec![0; area.rows.len()];
        let mut rows = Vec::new();
        for (row, source) in area.rows.iter().enumerate() {
            let mut cells = Vec::new();
            let mut col = covered[row];
            let mut index = 0;
            while col < new_width {
                let mut cell = source.children.get(index % source.child_count().max(1))?.clone();
                let span = get_span(&cell, "colspan").max(1);
                if col + span > new_width {
                    let attrs = remove_col_span(cell_attrs(&cell), span, col + span - new_width);
                    cell = cell.node_type()?.create_checked(Some(Value::Object(attrs)), Some(cell.content().clone()), None).ok()?;
                }
                let width = get_span(&cell, "colspan").max(1);
                let height = get_span(&cell, "rowspan");
                for filled in covered.iter_mut().skip(row + 1).take(height.saturating_sub(1)) {
                    *filled += width;
                }
                cells.push(cell);
                col += width;
                index += 1;
            }
            rows.push(Fragment::from_nodes(cells));
        }
        area.width = new_width;
        area.rows = rows;
    }
    if area.height != new_height {
        let mut rows = Vec::new();
        for row in 0..new_height {
            let source = area.rows.get(row % area.height.max(1))?;
            let mut cells = Vec::new();
            for cell in &source.children {
                let height = get_span(cell, "rowspan");
                if row.saturating_add(height) > new_height {
                    let mut attrs = cell_attrs(cell);
                    attrs.insert("rowspan".into(), json!(new_height.saturating_sub(height).max(1)));
                    cells.push(cell.node_type()?.try_create(Some(Value::Object(attrs)), Some(cell.content().clone()), None).ok()?);
                } else {
                    cells.push(cell.clone());
                }
            }
            rows.push(Fragment::from_nodes(cells));
        }
        area.rows = rows;
        area.height = new_height;
    }
    Some(area)
}

pub fn insert_cells(state: &EditorState, start: usize, rect: Rect, cells: Area) -> Option<Transaction> {
    if cells.width == 0 || cells.height == 0 || cells.height != cells.rows.len() {
        return None;
    }
    let mut tr = state.tr();
    let mut table = PasteTable::new(&tr, start)?;
    let target = Rect {
        right: rect.left.checked_add(cells.width)?,
        bottom: rect.top.checked_add(cells.height)?,
        ..rect
    };
    table.grow(&mut tr, &state.schema, target.right, target.bottom)?;
    table = PasteTable::new(&tr, start)?;
    for horizontal in [target.top, target.bottom] {
        if table.isolate_horizontal(&mut tr, target, horizontal)? {
            table = PasteTable::new(&tr, start)?;
        }
    }
    for vertical in [target.left, target.right] {
        if table.isolate_vertical(&mut tr, target, vertical)? {
            table = PasteTable::new(&tr, start)?;
        }
    }
    for (row, content) in (target.top..target.bottom).zip(cells.rows) {
        let from = table.map.position_at(row, target.left, &table.node)?;
        let to = table.map.position_at(row, target.right, &table.node)?;
        let from = table.map_pos(&tr, from);
        let to = table.map_pos(&tr, to);
        tr.replace(from, to, Slice::new(content, 0, 0));
    }
    let table = PasteTable::new(&tr, start)?;
    let anchor = start + table.map.position_at(target.top, target.left, &table.node)?;
    let head = start + table.map.position_at(target.bottom - 1, target.right - 1, &table.node)?;
    let selection = Selection::cell(tr.doc(), anchor, head).ok()?;
    tr.set_selection(selection);
    Some(tr)
}

struct PasteTable {
    node: Node,
    map: TableMap,
    start: usize,
    map_from: usize,
}

impl PasteTable {
    fn new(tr: &Transaction, start: usize) -> Option<Self> {
        let node = if start == 0 { tr.doc() } else { tr.doc().node_at(start - 1)? }.clone();
        let map = TableMap::get(&node).ok()?;
        Some(Self { node, map, start, map_from: tr.steps().len() })
    }

    fn map_pos(&self, tr: &Transaction, pos: usize) -> usize {
        tr.mapping().slice(self.map_from, tr.steps().len()).map(self.start + pos, 1)
    }

    fn grow(&self, tr: &mut Transaction, schema: &Schema, width: usize, height: usize) -> Option<()> {
        if width <= self.map.width && height <= self.map.height {
            return Some(());
        }
        let kinds = table_node_types(schema);
        let cell = kinds.get(&TableRole::Cell)?.create_and_fill(None, None, None).ok()??;
        let header = kinds.get(&TableRole::HeaderCell)?.create_and_fill(None, None, None).ok()??;
        if width > self.map.width {
            let mut end = 0;
            for row in &self.node.content().children {
                end += row.node_size();
                let template = if row.content().children.last().and_then(role) == Some("header_cell") { &header } else { &cell };
                let nodes = vec![template.clone(); width - self.map.width];
                let at = self.map_pos(tr, end - 1);
                tr.insert(at, Fragment::from_nodes(nodes));
            }
        }
        if height > self.map.height {
            let mut cells = Vec::new();
            for col in 0..width.max(self.map.width) {
                let last = self.map.height.checked_sub(1).filter(|_| col < self.map.width)
                    .and_then(|row| self.map.map.get(row * self.map.width + col))
                    .and_then(|&pos| self.node.node_at(pos));
                cells.push(if last.and_then(role) == Some("header_cell") { header.clone() } else { cell.clone() });
            }
            let row = kinds.get(&TableRole::Row)?.try_create(None, Some(Fragment::from_nodes(cells)), None).ok()?;
            let at = self.map_pos(tr, self.node.content().size());
            tr.insert(at, Fragment::from_nodes(vec![row; height - self.map.height]));
        }
        Some(())
    }

    fn isolate_horizontal(&self, tr: &mut Transaction, rect: Rect, at: usize) -> Option<bool> {
        if at == 0 || at >= self.map.height {
            return Some(false);
        }
        let mut col = rect.left;
        let mut changed = false;
        while col < rect.right {
            let index = at * self.map.width + col;
            let pos = self.map.map[index];
            if self.map.map[index - self.map.width] != pos {
                col += 1;
                continue;
            }
            let cell = self.node.node_at(pos)?;
            let dims = self.map.find_cell(pos).ok()?;
            let mut attrs = cell_attrs(cell);
            attrs.insert("rowspan".into(), json!(at - dims.top));
            let mapped_cell = self.map_pos(tr, pos);
            set_attrs(tr, mapped_cell, attrs.clone());
            attrs.insert("rowspan".into(), json!(dims.bottom - at));
            let after = cell.node_type()?.create_and_fill(Some(Value::Object(attrs)), None, None).ok()??;
            let insertion = self.map.position_at(at, dims.left, &self.node)?;
            let insertion = self.map_pos(tr, insertion);
            tr.insert(insertion, Fragment::from_nodes(vec![after]));
            changed = true;
            col += get_span(cell, "colspan").max(1);
        }
        Some(changed)
    }

    fn isolate_vertical(&self, tr: &mut Transaction, rect: Rect, at: usize) -> Option<bool> {
        if at == 0 || at >= self.map.width {
            return Some(false);
        }
        let mut row = rect.top;
        let mut changed = false;
        while row < rect.bottom {
            let index = row * self.map.width + at;
            let pos = self.map.map[index];
            if self.map.map[index - 1] != pos {
                row += 1;
                continue;
            }
            let cell = self.node.node_at(pos)?;
            let left = self.map.col_count(pos).ok()?;
            let span = get_span(cell, "colspan");
            let left_span = at - left;
            let mapped_cell = self.map_pos(tr, pos);
            set_attrs(tr, mapped_cell, remove_col_span(cell_attrs(cell), left_span, span - left_span));
            let attrs = remove_col_span(cell_attrs(cell), 0, left_span);
            let after = cell.node_type()?.create_and_fill(Some(Value::Object(attrs)), None, None).ok()??;
            tr.insert(mapped_cell + cell.node_size(), Fragment::from_nodes(vec![after]));
            changed = true;
            row += get_span(cell, "rowspan").max(1);
        }
        Some(changed)
    }
}
