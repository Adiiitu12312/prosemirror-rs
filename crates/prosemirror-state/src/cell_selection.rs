//! Selection geometry from prosemirror-tables 1.8.5, src/cellselection.ts.

use std::collections::HashSet;

use prosemirror_model::table_map::{Rect, TableMap, TableMapError};
use prosemirror_model::{Fragment, ModelError, Node, ResolvedPos, Slice};
use prosemirror_transform::Mappable;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::{Selection, SelectionRange};

/// A rectangular set of cells. The anchor and head point immediately before
/// their respective cells, while the primary selection range is inside the head.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CellSelection {
    pub anchor: usize,
    pub head: usize,
    #[serde(skip)]
    row: bool,
    #[serde(skip)]
    column: bool,
}

impl PartialEq for CellSelection {
    fn eq(&self, other: &Self) -> bool {
        self.anchor == other.anchor && self.head == other.head
    }
}

impl Eq for CellSelection {}

struct TableCells {
    table: Node,
    start: usize,
    map: TableMap,
    anchor: usize,
    head: usize,
    anchor_row: usize,
    head_row: usize,
}

impl TableCells {
    fn resolve(doc: &Node, anchor: usize, head: usize) -> Result<Self, ModelError> {
        let anchor = doc.resolve(anchor)?;
        let head = doc.resolve(head)?;
        if !points_at_cell(&anchor) || !points_at_cell(&head) || anchor.depth != head.depth {
            return Err(invalid_cells());
        }
        let depth = anchor.depth.checked_sub(1).ok_or_else(invalid_cells)?;
        let start = anchor.start(depth);
        if start != head.start(depth) {
            return Err(invalid_cells());
        }
        let table = anchor.node(depth).clone();
        let map = TableMap::get(&table).map_err(grid_error)?;
        Ok(Self {
            table,
            start,
            map,
            anchor: anchor.pos - start,
            head: head.pos - start,
            anchor_row: anchor.index(depth),
            head_row: head.index(depth),
        })
    }

    fn rect(&self) -> Result<Rect, ModelError> {
        self.map.rect_between(self.anchor, self.head).map_err(grid_error)
    }

    fn cell(&self, offset: usize) -> Result<&Node, ModelError> {
        self.table.node_at(offset).ok_or_else(|| grid_error(TableMapError::CellNotFound(offset)))
    }

    fn is_row(&self) -> Result<bool, ModelError> {
        let left = self.map.col_count(self.anchor).map_err(grid_error)?;
        let head_left = self.map.col_count(self.head).map_err(grid_error)?;
        Ok(left.min(head_left) == 0
            && (left + span(self.cell(self.anchor)?, "colspan"))
                .max(head_left + span(self.cell(self.head)?, "colspan")) == self.map.width)
    }

    fn is_col(&self) -> Result<bool, ModelError> {
        Ok(self.anchor_row.min(self.head_row) == 0
            && self.anchor_row.saturating_add(span(self.cell(self.anchor)?, "rowspan"))
                .max(self.head_row.saturating_add(span(self.cell(self.head)?, "rowspan")))
                == self.map.height)
    }
}

impl CellSelection {
    pub fn create(doc: &Node, anchor: usize, head: usize) -> Result<Self, ModelError> {
        let table = TableCells::resolve(doc, anchor, head)?;
        table.rect()?;
        Ok(Self { anchor, head, row: table.is_row()?, column: table.is_col()? })
    }

    pub fn rect(&self, doc: &Node) -> Result<Rect, ModelError> {
        TableCells::resolve(doc, self.anchor, self.head)?.rect()
    }

    pub fn from(&self) -> usize {
        self.head.saturating_add(1)
    }

    pub fn to(&self, doc: &Node) -> usize {
        let size = doc.node_at(self.head).map_or(0, |cell| cell.content().size());
        self.from().saturating_add(size)
    }

    pub fn ranges(&self, doc: &Node) -> Result<Vec<SelectionRange>, ModelError> {
        let table = TableCells::resolve(doc, self.anchor, self.head)?;
        let mut cells = table.map.cells_in_rect(table.rect()?);
        cells.retain(|&pos| pos != table.head);
        cells.insert(0, table.head);
        cells.into_iter().map(|pos| {
            let from = table.start + pos + 1;
            let to = from + table.cell(pos)?.content().size();
            Ok(SelectionRange { from: doc.resolve(from)?, to: doc.resolve(to)? })
        }).collect()
    }

    pub fn for_each_cell(
        &self,
        doc: &Node,
        mut visit: impl FnMut(&Node, usize),
    ) -> Result<(), ModelError> {
        let table = TableCells::resolve(doc, self.anchor, self.head)?;
        for pos in table.map.cells_in_rect(table.rect()?) {
            visit(table.cell(pos)?, table.start + pos);
        }
        Ok(())
    }

    pub fn is_row_selection(&self, doc: &Node) -> bool {
        TableCells::resolve(doc, self.anchor, self.head)
            .and_then(|table| table.is_row()).unwrap_or(false)
    }

    pub fn is_col_selection(&self, doc: &Node) -> bool {
        TableCells::resolve(doc, self.anchor, self.head)
            .and_then(|table| table.is_col()).unwrap_or(false)
    }

    pub fn row_selection(doc: &Node, mut anchor: usize, mut head: usize) -> Result<Self, ModelError> {
        let table = TableCells::resolve(doc, anchor, head)?;
        let a = table.map.find_cell(table.anchor).map_err(grid_error)?;
        let h = table.map.find_cell(table.head).map_err(grid_error)?;
        let width = table.map.width;
        if a.left <= h.left {
            if a.left > 0 {
                anchor = table.start + table.map.map[a.top * width];
            }
            if h.right < width {
                head = table.start + table.map.map[(h.top + 1) * width - 1];
            }
        } else {
            if h.left > 0 {
                head = table.start + table.map.map[h.top * width];
            }
            if a.right < width {
                anchor = table.start + table.map.map[(a.top + 1) * width - 1];
            }
        }
        Self::create(doc, anchor, head)
    }

    pub fn col_selection(doc: &Node, mut anchor: usize, mut head: usize) -> Result<Self, ModelError> {
        let table = TableCells::resolve(doc, anchor, head)?;
        let a = table.map.find_cell(table.anchor).map_err(grid_error)?;
        let h = table.map.find_cell(table.head).map_err(grid_error)?;
        let last_row = table.map.width * (table.map.height - 1);
        if a.top <= h.top {
            if a.top > 0 {
                anchor = table.start + table.map.map[a.left];
            }
            if h.bottom < table.map.height {
                head = table.start + table.map.map[last_row + h.right - 1];
            }
        } else {
            if h.top > 0 {
                head = table.start + table.map.map[h.left];
            }
            if a.bottom < table.map.height {
                anchor = table.start + table.map.map[last_row + a.right - 1];
            }
        }
        Self::create(doc, anchor, head)
    }

    pub fn content(&self, doc: &Node) -> Result<Slice, ModelError> {
        let table = TableCells::resolve(doc, self.anchor, self.head)?;
        let rect = table.rect()?;
        if table.is_row()? && table.is_col()? {
            return Ok(Slice::new(Fragment::from_nodes(vec![table.table]), 1, 1));
        }
        let mut seen = HashSet::new();
        let mut rows = Vec::new();
        for row in rect.top..rect.bottom {
            let mut cells = Vec::new();
            for col in rect.left..rect.right {
                let pos = table.map.map[row * table.map.width + col];
                if seen.insert(pos) {
                    let cell = table.cell(pos)?;
                    let cell_rect = table.map.find_cell(pos).map_err(grid_error)?;
                    cells.push(clip_cell(cell, cell_rect, rect)?);
                }
            }
            rows.push(table.table.child(row).copy(Some(Fragment::from_nodes(cells))));
        }
        Ok(Slice::new(Fragment::from_nodes(rows), 1, 1))
    }

    pub(crate) fn map(&self, doc: &Node, mapping: &impl Mappable) -> Selection {
        let anchor = super::selection::resolve_in(doc, mapping.map(self.anchor, 1));
        let head = super::selection::resolve_in(doc, mapping.map(self.head, 1));
        let mapped = Self::create(doc, anchor.pos, head.pos);
        if let Ok(mapped) = mapped {
            let extended = if self.row {
                Self::row_selection(doc, anchor.pos, head.pos)
            } else if self.column {
                Self::col_selection(doc, anchor.pos, head.pos)
            } else {
                return Selection::Cell(mapped);
            };
            Selection::Cell(extended.unwrap_or(mapped))
        } else {
            Selection::between(&anchor, &head, 1)
        }
    }

    pub(crate) fn map_bookmark(&self, mapping: &impl Mappable) -> Self {
        Self {
            anchor: mapping.map(self.anchor, 1),
            head: mapping.map(self.head, 1),
            row: false,
            column: false,
        }
    }
}

pub fn points_at_cell(pos: &ResolvedPos) -> bool {
    table_role(pos.parent()) == Some("row") && pos.index(pos.depth) < pos.parent().child_count()
}

pub fn cell_around(pos: &ResolvedPos) -> Option<ResolvedPos> {
    for depth in (1..pos.depth).rev() {
        if table_role(pos.node(depth)) == Some("row") {
            return pos.doc().resolve(pos.before(depth + 1)).ok();
        }
    }
    None
}

fn table_role(node: &Node) -> Option<&str> {
    node.node_type()?.spec.table_role.as_deref()
}

fn clip_cell(cell: &Node, cell_rect: Rect, selection: Rect) -> Result<Node, ModelError> {
    let left = selection.left.saturating_sub(cell_rect.left);
    let right = cell_rect.right.saturating_sub(selection.right);
    let vertical_clip = cell_rect.top < selection.top || cell_rect.bottom > selection.bottom;
    if left == 0 && right == 0 && !vertical_clip {
        return Ok(cell.clone());
    }
    let mut attrs = cell.attrs().and_then(Value::as_object).cloned().unwrap_or_default();
    let colspan = span(cell, "colspan");
    if left > 0 {
        remove_colspan(&mut attrs, 0, left, colspan);
    }
    if right > 0 {
        remove_colspan(
            &mut attrs,
            colspan.saturating_sub(left + right),
            right,
            colspan.saturating_sub(left),
        );
    }
    if vertical_clip {
        let rowspan = cell_rect.bottom.min(selection.bottom) - cell_rect.top.max(selection.top);
        attrs.insert("rowspan".into(), json!(rowspan));
    }
    let kind = cell.node_type().ok_or_else(|| ModelError::UnknownNodeType(cell.type_name().into()))?;
    if left > 0 || cell_rect.top < selection.top {
        kind.create_and_fill(Some(Value::Object(attrs)), None, None)?
            .ok_or_else(|| ModelError::InvalidContent(cell.type_name().into()))
    } else {
        kind.try_create(Some(Value::Object(attrs)), Some(cell.content().clone()), None)
    }
}

fn remove_colspan(attrs: &mut Map<String, Value>, pos: usize, amount: usize, colspan: usize) {
    attrs.insert("colspan".into(), json!(colspan.saturating_sub(amount)));
    if let Some(widths) = attrs.get_mut("colwidth").and_then(Value::as_array_mut) {
        let start = pos.min(widths.len());
        let end = start.saturating_add(amount).min(widths.len());
        widths.drain(start..end);
        if !widths.iter().any(|width| width.as_f64().is_some_and(|width| width > 0.0)) {
            attrs.insert("colwidth".into(), Value::Null);
        }
    }
}

fn span(cell: &Node, name: &str) -> usize {
    cell.attrs().and_then(|attrs| attrs[name].as_u64())
        .and_then(|span| span.try_into().ok()).unwrap_or(1)
}

fn grid_error(error: TableMapError) -> ModelError {
    ModelError::InvalidContent(error.to_string())
}

fn invalid_cells() -> ModelError {
    ModelError::InvalidContent("Cell selection must point before cells in the same table".into())
}
