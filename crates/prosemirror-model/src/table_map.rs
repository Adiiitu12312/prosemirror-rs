//! Port of prosemirror-tables@1.8.5/src/tablemap.ts.

use std::collections::HashSet;

use crate::Node;
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: usize,
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis { Horizontal, Vertical }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    ColwidthMismatch { pos: usize, colwidth: Vec<u64> },
    Collision { pos: usize, row: usize, n: usize },
    Missing { row: usize, n: usize },
    OverlongRowspan { pos: usize, n: usize },
    ZeroSized,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum TableMapError {
    #[error("Not a table node: {0}")]
    NotTable(String),
    #[error("No cell with offset {0} found")]
    CellNotFound(usize),
    #[error("Table size exceeds the addressable grid")]
    GridOverflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableMap {
    pub width: usize,
    pub height: usize,
    pub map: Vec<usize>,
    pub problems: Vec<Problem>,
}

impl TableMap {
    pub fn get(table: &Node) -> Result<Self, TableMapError> {
        if !table.node_type().is_some_and(|kind| kind.spec.table_role.as_deref() == Some("table")) {
            return Err(TableMapError::NotTable(table.type_name().into()));
        }
        let width = find_width(table)?;
        let height = table.child_count();
        let size = width.checked_mul(height).ok_or(TableMapError::GridOverflow)?;
        let mut map = Vec::new();
        map.try_reserve_exact(size).map_err(|_| TableMapError::GridOverflow)?;
        map.resize(size, 0);
        let mut widths = vec![None; width];
        let mut problems = Vec::new();
        let mut map_pos = 0;
        let mut pos = 0;
        for (row, row_node) in table.content().children.iter().enumerate() {
            pos += 1;
            for index in 0..=row_node.child_count() {
                while map.get(map_pos).is_some_and(|cell| *cell != 0) {
                    map_pos += 1;
                }
                let Some(cell) = row_node.content().children.get(index) else { break };
                let colspan = span(cell, "colspan");
                let rowspan = span(cell, "rowspan");
                let colwidth = cell.attrs().and_then(|attrs| attrs["colwidth"].as_array());
                let fitted_height = rowspan.min(height - row);
                for h in 0..fitted_height {
                    let start = map_pos + h * width;
                    for w in 0..colspan {
                        let slot = start + w;
                        if let Some(target) = map.get_mut(slot).filter(|cell| **cell == 0) {
                            *target = pos;
                        } else {
                            problems.push(Problem::Collision { row, pos, n: colspan - w });
                        }
                        if let Some(cell_width) = colwidth.and_then(|sizes| sizes.get(w))
                            .and_then(Value::as_u64).filter(|value| *value != 0)
                        {
                            record_width(&mut widths[slot % width], cell_width);
                        }
                    }
                }
                if fitted_height < rowspan {
                    problems.push(Problem::OverlongRowspan { pos, n: rowspan - fitted_height });
                }
                map_pos += colspan;
                pos += cell.node_size();
            }
            let row_end = (row + 1) * width;
            if map_pos < row_end {
                let missing = map[map_pos..row_end].iter().filter(|&&cell| cell == 0).count();
                if missing != 0 {
                    problems.push(Problem::Missing { row, n: missing });
                }
                map_pos = row_end;
            }
            pos += 1;
        }
        if width == 0 || height == 0 {
            problems.push(Problem::ZeroSized);
        }
        let mut result = Self { width, height, map, problems };
        if widths.iter().flatten().any(|&(_, count)| count < height) {
            result.find_bad_widths(&widths, table)?;
        }
        Ok(result)
    }

    pub fn find_cell(&self, pos: usize) -> Result<Rect, TableMapError> {
        let index = self.cell_index(pos)?;
        let left = index % self.width;
        let top = index / self.width;
        let mut right = left + 1;
        while right < self.width && self.map[top * self.width + right] == pos {
            right += 1;
        }
        let mut bottom = top + 1;
        while bottom < self.height && self.map[bottom * self.width + left] == pos {
            bottom += 1;
        }
        Ok(Rect { left, top, right, bottom })
    }

    pub fn col_count(&self, pos: usize) -> Result<usize, TableMapError> {
        Ok(self.cell_index(pos)? % self.width)
    }

    pub fn next_cell(&self, pos: usize, axis: Axis, direction: i32) -> Result<Option<usize>, TableMapError> {
        let rect = self.find_cell(pos)?;
        let (col, row) = match (axis, direction < 0) {
            (Axis::Horizontal, true) if rect.left != 0 => (rect.left - 1, rect.top),
            (Axis::Horizontal, false) if rect.right < self.width => (rect.right, rect.top),
            (Axis::Vertical, true) if rect.top != 0 => (rect.left, rect.top - 1),
            (Axis::Vertical, false) if rect.bottom < self.height => (rect.left, rect.bottom),
            _ => return Ok(None),
        };
        Ok(Some(self.map[row * self.width + col]))
    }

    pub fn rect_between(&self, a: usize, b: usize) -> Result<Rect, TableMapError> {
        let (a, b) = (self.find_cell(a)?, self.find_cell(b)?);
        Ok(Rect { left: a.left.min(b.left), top: a.top.min(b.top), right: a.right.max(b.right), bottom: a.bottom.max(b.bottom) })
    }

    pub fn cells_in_rect(&self, rect: Rect) -> Vec<usize> {
        let mut seen = HashSet::new();
        let mut cells = Vec::new();
        for row in rect.top..rect.bottom.min(self.height) {
            for col in rect.left..rect.right.min(self.width) {
                let index = row * self.width + col;
                let pos = self.map[index];
                if seen.insert(pos) && !(col == rect.left && col > 0 && self.map[index - 1] == pos)
                    && !(row == rect.top && row > 0 && self.map[index - self.width] == pos)
                {
                    cells.push(pos);
                }
            }
        }
        cells
    }

    pub fn position_at(&self, row: usize, col: usize, table: &Node) -> Option<usize> {
        if col > self.width {
            return None;
        }
        let row_node = table.content().children.get(row)?;
        let row_start: usize = table.content().children[..row].iter().map(Node::node_size).sum();
        let row_end = row_start + row_node.node_size();
        let start = row.checked_mul(self.width)?;
        Some(self.map.get(start + col..start + self.width)?
            .iter().copied().find(|&cell| cell >= row_start).unwrap_or(row_end - 1))
    }

    fn cell_index(&self, pos: usize) -> Result<usize, TableMapError> {
        self.map.iter().position(|&cell| cell == pos).ok_or(TableMapError::CellNotFound(pos))
    }

    fn find_bad_widths(&mut self, widths: &[Option<(u64, usize)>], table: &Node) -> Result<(), TableMapError> {
        let mut seen = HashSet::new();
        for (index, &pos) in self.map.iter().enumerate() {
            if pos == 0 || !seen.insert(pos) {
                continue;
            }
            let node = table.node_at(pos).ok_or(TableMapError::CellNotFound(pos))?;
            let colspan = span(node, "colspan");
            let original = node.attrs().and_then(|attrs| attrs["colwidth"].as_array());
            let mut updated = None;
            for j in 0..colspan {
                if let Some((width, _)) = widths[(index + j) % self.width] {
                    if original.and_then(|sizes| sizes.get(j)).and_then(Value::as_u64) != Some(width) {
                        let cells = updated.get_or_insert_with(|| {
                            let mut widths = original.into_iter().flatten().map(|value| value.as_u64().unwrap_or(0)).collect::<Vec<_>>();
                            widths.resize(colspan, 0);
                            widths
                        });
                        cells[j] = width;
                    }
                }
            }
            if let Some(colwidth) = updated {
                self.problems.insert(0, Problem::ColwidthMismatch { pos, colwidth });
            }
        }
        Ok(())
    }
}

fn record_width(known: &mut Option<(u64, usize)>, width: u64) {
    match known {
        Some((previous, count)) if *previous == width => *count += 1,
        None | Some((_, 1)) => *known = Some((width, 1)),
        _ => {}
    }
}

fn span(node: &Node, name: &str) -> usize {
    node.attrs().and_then(|attrs| attrs[name].as_u64()).and_then(|span| span.try_into().ok()).unwrap_or(1)
}

fn find_width(table: &Node) -> Result<usize, TableMapError> {
    let rows = &table.content().children;
    let mut width = 0;
    let mut has_rowspan = false;
    for (row, node) in rows.iter().enumerate() {
        let mut size: usize = 0;
        if has_rowspan {
            for (index, earlier_row) in rows[..row].iter().enumerate() {
                for cell in &earlier_row.content().children {
                    if span(cell, "rowspan") > row - index {
                        size = size.checked_add(span(cell, "colspan")).ok_or(TableMapError::GridOverflow)?;
                    }
                }
            }
        }
        for cell in &node.content().children {
            size = size.checked_add(span(cell, "colspan")).ok_or(TableMapError::GridOverflow)?;
            has_rowspan |= span(cell, "rowspan") > 1;
        }
        width = width.max(size);
    }
    Ok(width)
}
