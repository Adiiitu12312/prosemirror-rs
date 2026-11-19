//! Host-facing table input commands from prosemirror-tables@1.8.5/src/input.ts.

use prosemirror_model::{Fragment, ResolvedPos, Slice};
use prosemirror_state::{EditorState, Selection};

use crate::commands::{cell_around, role, selected_rect, Dispatch, TableRect};
use crate::copypaste::{clip_cells, fit_slice, insert_cells, pasted_cells, Area};
use crate::{table_node_types, Axis, TableRole};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction { Backward, Forward }

impl Direction {
    fn bias(self) -> i32 {
        match self { Self::Backward => -1, Self::Forward => 1 }
    }
}

pub fn arrow(
    state: &EditorState, dispatch: Dispatch<'_>, axis: Axis, direction: Direction,
    at_textblock_edge: impl FnOnce(Axis, Direction) -> bool,
) -> bool {
    let bias = direction.bias();
    if let Selection::Cell(cells) = &state.selection {
        let Ok(head) = state.doc.resolve(cells.head) else { return false };
        return select(state, dispatch, Selection::near(&head, bias));
    }
    if axis == Axis::Vertical && !state.selection.empty_in(&state.doc) {
        return false;
    }
    let Some(cell) = at_end_of_cell(state, direction).filter(|_| at_textblock_edge(axis, direction))
        else { return false };
    let next = match axis {
        Axis::Horizontal => state.selection.head(&state.doc).checked_add_signed(bias as isize)
            .and_then(|pos| state.doc.resolve(pos).ok())
            .map(|pos| Selection::near(&pos, bias)),
        Axis::Vertical => next_cell(state, cell.pos, axis, direction).map(|pos| Selection::near(&pos, 1))
            .or_else(|| {
                let depth = cell.depth.checked_sub(1)?;
                let pos = if depth == 0 {
                    if bias < 0 { 0 } else { state.doc.content().size() }
                } else if bias < 0 {
                    cell.before(depth)
                } else {
                    cell.after(depth)
                };
                state.doc.resolve(pos).ok().map(|p| Selection::near(&p, bias))
            }),
    };
    next.is_some_and(|selection| select(state, dispatch, selection))
}

pub fn shift_arrow(
    state: &EditorState, dispatch: Dispatch<'_>, axis: Axis, direction: Direction,
    at_textblock_edge: impl FnOnce(Axis, Direction) -> bool,
) -> bool {
    let (anchor, head) = if let Selection::Cell(cells) = &state.selection {
        (cells.anchor, cells.head)
    } else {
        let Some(cell) = at_end_of_cell(state, direction).filter(|_| at_textblock_edge(axis, direction))
            else { return false };
        (cell.pos, cell.pos)
    };
    let Some(next) = next_cell(state, head, axis, direction) else { return false };
    Selection::cell(&state.doc, anchor, next.pos).is_ok_and(|selection| select(state, dispatch, selection))
}

pub fn handle_triple_click(state: &EditorState, dispatch: Dispatch<'_>, pos: usize) -> bool {
    let Some(cell) = state.doc.resolve(pos).ok().and_then(|pos| cell_around(&pos)) else { return false };
    let Ok(selection) = Selection::cell(&state.doc, cell.pos, cell.pos) else { return false };
    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        tr.set_selection(selection);
        dispatch(tr);
    }
    true
}

pub fn handle_paste(state: &EditorState, dispatch: Dispatch<'_>, slice: Slice) -> bool {
    let Some(rect) = selected_rect(state) else { return false };
    let area = pasted_cells(&slice);
    let cells = if matches!(state.selection, Selection::Cell(_)) {
        let area = match area {
            Some(cells) => cells,
            None => {
                let kinds = table_node_types(&state.schema);
                let Some(cell) = kinds.get(&TableRole::Cell).and_then(|kind| fit_slice(kind, slice))
                    else { return false };
                Area { width: 1, height: 1, rows: vec![Fragment::from_nodes(vec![cell])] }
            }
        };
        clip_cells(area, rect.rect.right - rect.rect.left, rect.rect.bottom - rect.rect.top)
    } else {
        area
    };
    let Some(cells) = cells else { return false };
    let Some(tr) = insert_cells(state, rect.table_start, rect.rect, cells) else { return false };
    if let Some(dispatch) = dispatch {
        dispatch(tr);
    }
    true
}

fn select(state: &EditorState, dispatch: Dispatch<'_>, selection: Selection) -> bool {
    if state.selection == selection {
        return false;
    }
    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        tr.set_selection(selection).scroll_into_view();
        dispatch(tr);
    }
    true
}

fn next_cell(state: &EditorState, pos: usize, axis: Axis, direction: Direction) -> Option<ResolvedPos> {
    let tab = TableRect::from_cells(&state.doc, pos, pos)?;
    let next = tab.map.next_cell(pos - tab.table_start, axis, direction.bias()).ok()??;
    state.doc.resolve(tab.table_start + next).ok()
}

fn at_end_of_cell(state: &EditorState, direction: Direction) -> Option<ResolvedPos> {
    let Selection::Text { head, .. } = state.selection else { return None };
    let head = state.doc.resolve(head).ok()?;
    for depth in (0..head.depth).rev() {
        let node = head.node(depth);
        let at_edge = match direction {
            Direction::Backward => head.index(depth) == 0,
            Direction::Forward => head.index_after(depth) == node.child_count(),
        };
        if !at_edge {
            return None;
        }
        if matches!(role(node), Some("cell" | "header_cell")) {
            return state.doc.resolve(head.before(depth)).ok();
        }
    }
    None
}
