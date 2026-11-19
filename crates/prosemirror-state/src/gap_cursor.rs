//! Gap cursor selection rules from prosemirror-gapcursor 1.4.0.

use prosemirror_model::{Node, ResolvedPos};

use crate::Selection;

pub fn valid_gap_cursor(pos: &ResolvedPos) -> bool {
    let parent = pos.parent();
    if parent.is_textblock() || !closed_at(pos, false) || !closed_at(pos, true) {
        return false;
    }
    if let Some(allowed) = parent.node_type().and_then(|ty| ty.spec.allow_gap_cursor) {
        return allowed;
    }
    let Some(schema) = parent.schema() else { return false };
    parent.content_match_at(pos.index(pos.depth))
        .and_then(|matched| schema.node_type(matched.default_type()?))
        .is_some_and(|ty| ty.is_textblock())
}

pub(crate) fn gap_cursor_or_near(pos: &ResolvedPos) -> Selection {
    if valid_gap_cursor(pos) {
        Selection::gap_cursor(pos.pos)
    } else {
        Selection::near(pos, 1)
    }
}

pub fn find_gap_cursor_from(
    pos: &ResolvedPos,
    direction: i32,
    mut must_move: bool,
) -> Option<ResolvedPos> {
    let doc = pos.doc();
    let mut current = pos.clone();
    let step = if direction < 0 { -1 } else { 1 };
    'search: loop {
        if !must_move && valid_gap_cursor(&current) {
            return Some(current);
        }
        let mut position = current.pos;
        let mut depth = current.depth;
        let mut next = loop {
            let parent = current.node(depth);
            let child = if step > 0 {
                parent.maybe_child(current.index_after(depth))
            } else {
                current.index(depth).checked_sub(1).and_then(|index| parent.maybe_child(index))
            };
            if let Some(child) = child {
                break child;
            }
            depth = depth.checked_sub(1)?;
            position = position.checked_add_signed(step)?;
            let resolved = doc.resolve(position).ok()?;
            if valid_gap_cursor(&resolved) {
                return Some(resolved);
            }
        };
        loop {
            let inside = if step > 0 { next.first_child() } else { next.last_child() };
            let Some(inside) = inside else {
                if next.is_atom() && !next.is_text() && !next.is_selectable() {
                    let size = isize::try_from(next.node_size()).ok()?;
                    current = doc.resolve(position.checked_add_signed(size * step)?).ok()?;
                    must_move = false;
                    continue 'search;
                }
                return None;
            };
            next = inside;
            position = position.checked_add_signed(step)?;
            let resolved = doc.resolve(position).ok()?;
            if valid_gap_cursor(&resolved) {
                return Some(resolved);
            }
        }
    }
}

fn needs_gap(node: &Node) -> bool {
    node.is_atom() || node.is_isolating()
        || node.node_type().is_some_and(|ty| ty.spec.create_gap_cursor)
}

fn closed_at(pos: &ResolvedPos, after: bool) -> bool {
    for depth in (0..=pos.depth).rev() {
        let parent = pos.node(depth);
        let side = if after {
            parent.maybe_child(pos.index_after(depth))
        } else {
            pos.index(depth).checked_sub(1).and_then(|index| parent.maybe_child(index))
        };
        if let Some(mut node) = side {
            loop {
                if (node.child_count() == 0 && !node.inline_content()) || needs_gap(node) {
                    return true;
                }
                if node.inline_content() {
                    return false;
                }
                node = (if after { node.first_child() } else { node.last_child() })
                    .expect("nonempty block node");
            }
        }
        if parent.is_isolating() {
            return true;
        }
    }
    true
}
