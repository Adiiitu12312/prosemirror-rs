use crate::error::ModelError;
use crate::node::{Fragment, Node};
use crate::resolved_pos::ResolvedPos;
use crate::slice::Slice;

pub fn replace(from: &ResolvedPos, to: &ResolvedPos, slice: &Slice) -> Result<Node, ModelError> {
    if slice.open_start > from.depth {
        return Err(ModelError::InvalidSlice(slice.open_start, from.depth));
    }
    if from.depth.saturating_sub(slice.open_start) != to.depth.saturating_sub(slice.open_end) {
        return Err(ModelError::InvalidSlice(slice.open_start, slice.open_end));
    }
    replace_outer(from, to, slice, 0)
}

pub fn replace_at_pos(doc: &Node, from_pos: usize, to_pos: usize, slice: &Slice) -> Result<Node, ModelError> {
    let from = doc.resolve(from_pos)?;
    let to = doc.resolve(to_pos)?;
    replace(&from, &to, slice)
}

fn replace_outer(from: &ResolvedPos, to: &ResolvedPos, slice: &Slice, depth: usize) -> Result<Node, ModelError> {
    let index = from.index(depth);
    let node = from.node(depth);

    if index == to.index(depth) && depth < from.depth.saturating_sub(slice.open_start) {
        let inner = replace_outer(from, to, slice, depth + 1)?;
        let new_content = node.content().replace_child(index, inner);
        return Ok(node.copy(Some(new_content)));
    }

    if slice.content.size() == 0 {
        let new_content = replace_two_way(from, to, depth);
        return Ok(node.copy(Some(new_content)));
    }

    if slice.open_start == 0 && slice.open_end == 0 && from.depth == depth && to.depth == depth {
        let parent = from.parent();
        let content = parent.content();
        let new_content = content
            .cut(0, from.parent_offset)
            .append(&slice.content)
            .append(&content.cut(to.parent_offset, content.size()));
        return Ok(parent.copy(Some(new_content)));
    }

    // General fallback: join left cut, middle slice, right cut
    let content = node.content();
    let left = content.cut(0, from.pos);
    let right = content.cut(to.pos, content.size());
    let joined = left.append(&slice.content).append(&right);
    Ok(node.copy(Some(joined)))
}

fn replace_two_way(from: &ResolvedPos, to: &ResolvedPos, depth: usize) -> Fragment {
    let mut result = Vec::new();
    add_range(None, Some(from), depth, &mut result);
    if from.depth > depth {
        let inner = replace_two_way(from, to, depth + 1);
        let joined_node = from.node(depth + 1).copy(Some(inner));
        result.push(joined_node);
    }
    add_range(Some(to), None, depth, &mut result);
    Fragment::from_nodes(result)
}

fn add_range(start: Option<&ResolvedPos>, end: Option<&ResolvedPos>, depth: usize, target: &mut Vec<Node>) {
    let node = end.or(start).unwrap().node(depth);
    let mut start_idx = 0;
    let end_idx = if let Some(e) = end {
        e.index(depth)
    } else {
        node.child_count()
    };

    if let Some(s) = start {
        start_idx = s.index(depth);
        if s.depth > depth {
            start_idx += 1;
        } else if s.text_offset() > 0 {
            if let Some(node_after) = s.node_after() {
                target.push(node_after);
            }
            start_idx += 1;
        }
    }

    for i in start_idx..end_idx {
        target.push(node.child(i).clone());
    }

    if let Some(e) = end {
        if e.depth == depth && e.text_offset() > 0 {
            if let Some(node_before) = e.node_before() {
                target.push(node_before);
            }
        }
    }
}
