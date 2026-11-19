//! Grid utilities from prosemirror-tables 1.8.5 src/utils.

use prosemirror_model::{Fragment, ModelError, Node};
use thiserror::Error;

use crate::{TableMap, TableMapError};

pub type CellRows = Vec<Vec<Option<Node>>>;

#[derive(Debug, Error)]
pub enum TableConversionError {
    #[error(transparent)]
    Grid(#[from] TableMapError),
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error("The replacement grid has no row {0}")]
    MissingRow(usize),
}

/// Swap the rows and columns of a rectangular grid.
pub fn transpose<T: Clone>(rows: &[Vec<T>]) -> Vec<Vec<T>> {
    let columns = rows.first().map_or(0, Vec::len);
    (0..columns).map(|col| rows.iter().map(|row| row[col].clone()).collect()).collect()
}

/// Store a spanned cell only in its top-left slot.
pub fn convert_table_node_to_array_of_rows(table: &Node) -> Result<CellRows, TableMapError> {
    let map = TableMap::get(table)?;
    let mut rows = Vec::with_capacity(map.height);
    for row in 0..map.height {
        let mut cells = Vec::with_capacity(map.width);
        for col in 0..map.width {
            let index = row * map.width + col;
            let position = map.map[index];
            let carried = (row > 0 && map.map[index - map.width] == position)
                || (col > 0 && map.map[index - 1] == position);
            cells.push(if carried { None } else { table.node_at(position).cloned() });
        }
        rows.push(cells);
    }
    Ok(rows)
}

/// Rebuild the table using supplied cell attributes and content and the destination
/// cell types. This keeps header and body cell roles fixed during row/column moves.
pub fn convert_array_of_rows_to_table_node(
    table: &Node,
    rows: &[Vec<Option<Node>>],
) -> Result<Node, TableConversionError> {
    let map = TableMap::get(table)?;
    let mut next_rows = Vec::with_capacity(map.height);
    for row in 0..map.height {
        let old_row = table.child(row);
        let cells = rows.get(row).ok_or(TableConversionError::MissingRow(row))?;
        let mut next_cells = Vec::new();
        for (col, source) in cells.iter().take(map.width).enumerate() {
            let Some(source) = source else { continue };
            let Some(destination) = table.node_at(map.map[row * map.width + col]) else { continue };
            next_cells.push(recreate(destination, source, source.content().clone())?);
        }
        next_rows.push(recreate(old_row, old_row, Fragment::from_nodes(next_cells))?);
    }
    recreate(table, table, Fragment::from_nodes(next_rows)).map_err(TableConversionError::from)
}

fn recreate(destination: &Node, source: &Node, content: Fragment) -> Result<Node, ModelError> {
    let ty = destination.node_type().ok_or_else(|| ModelError::UnknownNodeType(destination.type_name().to_owned()))?;
    ty.create_checked(source.attrs().cloned(), Some(content), Some(source.marks().to_vec()))
}

/// Move the consecutive rows identified by `origin` within the vector.
pub fn move_row_in_array_of_rows<'a, T>(
    rows: &'a mut Vec<T>,
    origin: &[usize],
    target: &[usize],
    direction_override: i8,
) -> &'a mut Vec<T> {
    let (Some(&start), Some(&target_start), Some(&target_end)) =
        (origin.first(), target.first(), target.last())
    else {
        return rows;
    };
    let backward = start > target_start;
    let start = start.min(rows.len());
    let end = start.saturating_add(origin.len()).min(rows.len());
    let moved = rows.drain(start..end).collect::<Vec<_>>();
    let offset = isize::from(moved.len().is_multiple_of(2));
    let target = match (direction_override, backward) {
        (-1, false) => target_start as isize - 1,
        (1, true) => target_end as isize - offset + 1,
        (_, true) => target_start as isize,
        (_, false) => target_end as isize - offset,
    };
    // Array.splice counts a negative index back from the end of the array.
    let insert = if target < 0 {
        rows.len().saturating_sub(target.unsigned_abs())
    } else {
        (target as usize).min(rows.len())
    };
    rows.splice(insert..insert, moved);
    rows
}
