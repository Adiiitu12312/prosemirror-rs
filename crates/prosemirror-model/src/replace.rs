use crate::error::ModelError;
use crate::node::{Fragment, Node};
use crate::resolved_pos::ResolvedPos;
use crate::slice::Slice;

pub fn replace(from: &ResolvedPos, to: &ResolvedPos, slice: &Slice) -> Result<Node, ModelError> {
    if from.pos > to.pos {
        return Err(ModelError::ReplaceError("Replacement range is reversed".to_string()));
    }
    if slice.open_start > from.depth {
        return Err(ModelError::ReplaceError(
            "Inserted content deeper than insertion position".to_string(),
        ));
    }
    if Some(from.depth - slice.open_start) != to.depth.checked_sub(slice.open_end) {
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

    if index == to.index(depth) && depth < from.depth - slice.open_start {
        let inner = replace_outer(from, to, slice, depth + 1)?;
        let new_content = node.content().replace_child(index, inner);
        Ok(node.copy(Some(new_content)))
    } else if slice.content.size() == 0 {
        let new_content = replace_two_way(from, to, depth)?;
        close(node, new_content)
    } else if slice.open_start == 0 && slice.open_end == 0 && from.depth == depth && to.depth == depth {
        let parent = from.parent();
        let content = parent.content();
        let new_content = content
            .cut(0, from.parent_offset)
            .append(&slice.content)
            .append(&content.cut(to.parent_offset, content.size()));
        close(parent, new_content)
    } else {
        let (start, end) = prepare_slice_for_replace(slice, from)?;
        let new_content = replace_three_way(from, &start, &end, to, depth)?;
        close(node, new_content)
    }
}

fn close(node: &Node, content: Fragment) -> Result<Node, ModelError> {
    if let Some(node_type) = node.node_type() {
        node_type.check_content(&content)?;
    } else if !node.can_replace(0, node.child_count(), &content) {
        return Err(ModelError::InvalidContent(node.type_name().to_string()));
    }
    Ok(node.copy(Some(content)))
}

fn check_join(node: &Node, other: &Node) -> Result<(), ModelError> {
    if !node.compatible_content(other) {
        return Err(ModelError::ReplaceError(format!(
            "Cannot join {} onto {}",
            other.type_name(),
            node.type_name()
        )));
    }
    Ok(())
}

fn joinable<'a>(before: &'a ResolvedPos, after: &ResolvedPos, depth: usize) -> Result<&'a Node, ModelError> {
    let node = before.node(depth);
    check_join(node, after.node(depth))?;
    Ok(node)
}

fn add_node(child: Node, target: &mut Vec<Node>) {
    if let Some(last) = target.last_mut() {
        if child.is_text() && child.same_markup(last) {
            let merged = format!("{}{}", last.text().unwrap_or(""), child.text().unwrap_or(""));
            *last = last.with_text(merged);
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
        add_node(close(node, inner)?, &mut content);
    }
    add_range(Some(to), None, depth, &mut content);
    Ok(Fragment::from_children(content))
}

fn replace_three_way(
    from: &ResolvedPos,
    start: &ResolvedPos,
    end: &ResolvedPos,
    to: &ResolvedPos,
    depth: usize,
) -> Result<Fragment, ModelError> {
    let open_start = if from.depth > depth {
        Some(joinable(from, start, depth + 1)?)
    } else {
        None
    };
    let open_end = if to.depth > depth {
        Some(joinable(end, to, depth + 1)?)
    } else {
        None
    };

    let mut content = Vec::new();
    add_range(None, Some(from), depth, &mut content);

    match (open_start, open_end) {
        (Some(left), Some(right)) if start.index(depth) == end.index(depth) => {
            check_join(left, right)?;
            let inner = replace_three_way(from, start, end, to, depth + 1)?;
            add_node(close(left, inner)?, &mut content);
        }
        (left, right) => {
            if let Some(left) = left {
                let inner = replace_two_way(from, start, depth + 1)?;
                add_node(close(left, inner)?, &mut content);
            }
            add_range(Some(start), Some(end), depth, &mut content);
            if let Some(right) = right {
                let inner = replace_two_way(end, to, depth + 1)?;
                add_node(close(right, inner)?, &mut content);
            }
        }
    }

    add_range(Some(to), None, depth, &mut content);
    Ok(Fragment::from_children(content))
}

fn prepare_slice_for_replace(slice: &Slice, along: &ResolvedPos) -> Result<(ResolvedPos, ResolvedPos), ModelError> {
    let extra = along.depth - slice.open_start;
    let parent = along.node(extra);
    let mut node = parent.copy(Some(slice.content.clone()));
    for i in (0..extra).rev() {
        let wrapper = along.node(i);
        node = wrapper.copy(Some(Fragment::from_nodes(vec![node])));
    }
    let start_pos = slice.open_start + extra;
    let end_pos = node.content().size().checked_sub(
        slice.open_end.checked_add(extra).ok_or(ModelError::InvalidSlice(slice.open_start, slice.open_end))?,
    ).ok_or(ModelError::InvalidSlice(slice.open_start, slice.open_end))?;
    let start = node.resolve(start_pos)?;
    let end = node.resolve(end_pos)?;
    Ok((start, end))
}
