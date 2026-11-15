use crate::error::ModelError;
use crate::node::{Fragment, Node};
use crate::resolved_pos::ResolvedPos;
use crate::slice::Slice;

pub fn replace(from: &ResolvedPos, to: &ResolvedPos, slice: &Slice) -> Result<Node, ModelError> {
    if slice.open_start > from.depth {
        return Err(ModelError::ReplaceError(
            "Inserted content deeper than insertion position".to_string(),
        ));
    }
    if from.depth.saturating_sub(slice.open_start) != to.depth.saturating_sub(slice.open_end) {
        return Err(ModelError::ReplaceError(
            "Inconsistent open depths".to_string(),
        ));
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
    } else if slice.content.size() == 0 {
        let new_content = replace_two_way(from, to, depth)?;
        return Ok(node.copy(Some(new_content)));
    } else if slice.open_start == 0 && slice.open_end == 0 && from.depth == depth && to.depth == depth {
        let parent = from.parent();
        let content = parent.content();
        let new_content = content
            .cut(0, from.parent_offset)
            .append(&slice.content)
            .append(&content.cut(to.parent_offset, content.size()));
        return Ok(parent.copy(Some(new_content)));
    } else {
        let (start, end) = prepare_slice_for_replace(slice, from)?;
        let new_content = replace_three_way(from, &start, &end, to, depth)?;
        return Ok(node.copy(Some(new_content)));
    }
}

fn joinable(before: &ResolvedPos, after: &ResolvedPos, depth: usize) -> Result<Node, ModelError> {
    let node = before.node(depth);
    let other = after.node(depth);
    if node.type_name() != other.type_name() {
        return Err(ModelError::ReplaceError(format!(
            "Cannot join {} onto {}",
            other.type_name(),
            node.type_name()
        )));
    }
    Ok(node.clone())
}

fn add_node(child: Node, target: &mut Vec<Node>) {
    if let Some(last) = target.last_mut() {
        if child.is_text() && child.same_markup(last) {
            let merged = format!("{}{}", last.text().unwrap_or(""), child.text().unwrap_or(""));
            *last = Node::new_text(merged, last.marks().to_vec());
            return;
        }
    }
    target.push(child);
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
                add_node(node_after, target);
            }
            start_idx += 1;
        }
    }

    for i in start_idx..end_idx {
        add_node(node.child(i).clone(), target);
    }

    if let Some(e) = end {
        if e.depth == depth && e.text_offset() > 0 {
            if let Some(node_before) = e.node_before() {
                add_node(node_before, target);
            }
        }
    }
}

fn replace_two_way(from: &ResolvedPos, to: &ResolvedPos, depth: usize) -> Result<Fragment, ModelError> {
    let mut content = Vec::new();
    add_range(None, Some(from), depth, &mut content);
    if from.depth > depth {
        let node = joinable(from, to, depth + 1)?;
        let inner = replace_two_way(from, to, depth + 1)?;
        add_node(node.copy(Some(inner)), &mut content);
    }
    add_range(Some(to), None, depth, &mut content);
    Ok(Fragment::from_nodes(content))
}

fn replace_three_way(
    from: &ResolvedPos,
    start: &ResolvedPos,
    end: &ResolvedPos,
    to: &ResolvedPos,
    depth: usize,
) -> Result<Fragment, ModelError> {
    let open_start = if from.depth > depth {
        joinable(from, start, depth + 1).ok()
    } else {
        None
    };
    let open_end = if to.depth > depth {
        joinable(end, to, depth + 1).ok()
    } else {
        None
    };

    let mut content = Vec::new();
    add_range(None, Some(from), depth, &mut content);

    if open_start.is_some() && open_end.is_some() && start.index(depth) == end.index(depth) {
        let n = open_start.unwrap();
        let inner = replace_three_way(from, start, end, to, depth + 1)?;
        add_node(n.copy(Some(inner)), &mut content);
    } else {
        if let Some(n) = open_start {
            let inner = replace_two_way(from, start, depth + 1)?;
            add_node(n.copy(Some(inner)), &mut content);
        }
        add_range(Some(start), Some(end), depth, &mut content);
        if let Some(n) = open_end {
            let inner = replace_two_way(end, to, depth + 1)?;
            add_node(n.copy(Some(inner)), &mut content);
        }
    }

    add_range(Some(to), None, depth, &mut content);
    Ok(Fragment::from_nodes(content))
}

fn prepare_slice_for_replace(slice: &Slice, along: &ResolvedPos) -> Result<(ResolvedPos, ResolvedPos), ModelError> {
    let extra = along.depth.saturating_sub(slice.open_start);
    let parent = along.node(extra);
    let mut node = parent.copy(Some(slice.content.clone()));
    for i in (0..extra).rev() {
        let wrapper = along.node(i);
        node = wrapper.copy(Some(Fragment::from_nodes(vec![node])));
    }
    let start_pos = slice.open_start + extra;
    let end_pos = node.content().size().saturating_sub(slice.open_end + extra);
    let start = node.resolve(start_pos)?;
    let end = node.resolve(end_pos)?;
    Ok((start, end))
}
