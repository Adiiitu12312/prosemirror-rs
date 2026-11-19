//! Structural transformations, ported from prosemirror-transform/src/structure.ts.

use prosemirror_model::{Fragment, Mark, Node, NodeRange, Slice};
use serde_json::Value;

use crate::mark::{clear_incompatible, newline_conversion, translate_newlines};
use crate::replace::{after, before, compatible, singleton};
use crate::step::node_at;
use crate::{Step, Transform};

fn fragment_by_index(node: &Node, from: usize, to: usize) -> Fragment {
    Fragment::from_nodes(node.content().children[from..to].to_vec())
}

pub fn lift_target(doc: &Node, range: &NodeRange) -> Option<usize> {
    let from = doc.resolve(range.start).ok()?;
    let to = doc.resolve(range.end).ok()?;
    let parent = from.node(range.depth);
    let content = fragment_by_index(parent, range.start_index, range.end_index);
    for depth in (0..=range.depth).rev() {
        let node = from.node(depth);
        let index = from.index(depth);
        let end_index = to.index_after(depth);
        if depth < range.depth && node.can_replace(index, end_index, &content) {
            return Some(depth);
        }
        if depth == 0 || node.is_isolating() || !can_cut(node, index, end_index) {
            break;
        }
    }
    None
}

fn can_cut(node: &Node, start: usize, end: usize) -> bool {
    let empty = Fragment::empty();
    (start == 0 || node.can_replace(start, node.child_count(), &empty))
        && (end == node.child_count() || node.can_replace(0, end, &empty))
}

pub(crate) fn lift(transform: &mut Transform, range: &NodeRange, target: usize) {
    let (Ok(from), Ok(to)) = (transform.doc.resolve(range.start), transform.doc.resolve(range.end)) else {
        return;
    };
    if target >= range.depth {
        return;
    }
    let (mut start, mut end) = (range.start, range.end);
    let (mut left, mut right) = (Fragment::empty(), Fragment::empty());
    let (mut open_start, mut open_end) = (0, 0);
    for depth in (target + 1..=range.depth).rev() {
        if open_start > 0 || from.index(depth) > 0 {
            left = singleton(from.node(depth).copy(Some(left)));
            open_start += 1;
        } else {
            start -= 1;
        }
        if open_end > 0 || after(&to, depth + 1) < to.end(depth) {
            right = singleton(to.node(depth).copy(Some(right)));
            open_end += 1;
        } else {
            end += 1;
        }
    }
    let insert = left.size() - open_start;
    let slice = Slice::new(left.append(&right), open_start, open_end);
    transform.maybe_step(Step::replace_around(start, end, range.start, range.end, slice, insert, true));
}

pub fn find_wrapping(
    doc: &Node,
    range: &NodeRange,
    type_name: &str,
    attrs: Option<Value>,
    inner_range: Option<&NodeRange>,
) -> Option<Vec<Node>> {
    let schema = doc.schema()?;
    let start = doc.resolve(range.start).ok()?;
    let outer_parent = start.node(range.depth);
    let around = outer_parent.content_match_at(range.start_index)?.find_wrapping(schema, type_name)?;
    let outer_type = around.first().map_or(type_name, String::as_str);
    if !outer_parent.can_replace_with(range.start_index, range.end_index, outer_type, &[]) {
        return None;
    }

    let inner_range = inner_range.unwrap_or(range);
    let inner_start = doc.resolve(inner_range.start).ok()?;
    let inner_parent = inner_start.node(inner_range.depth);
    let first = inner_parent.content().children.get(inner_range.start_index)?;
    let type_ = schema.node_type(type_name)?;
    let inside = type_.content_match().find_wrapping(schema, first.type_name())?;
    let last_type = inside.last().map_or(Some(type_), |name| schema.node_type(name))?;
    let matched = last_type.content_match().match_fragment(
        inner_parent.content(), inner_range.start_index, inner_range.end_index,
    )?;
    if !matched.valid_end() {
        return None;
    }

    let mut wrappers = Vec::new();
    for name in around {
        wrappers.push(schema.node_type(&name)?.try_create(None, None, None).ok()?);
    }
    wrappers.push(type_.try_create(attrs, None, None).ok()?);
    for name in inside {
        wrappers.push(schema.node_type(&name)?.try_create(None, None, None).ok()?);
    }
    Some(wrappers)
}

pub(crate) fn wrap(transform: &mut Transform, range: &NodeRange, wrappers: &[Node]) {
    if wrappers.is_empty() {
        return;
    }
    let mut content = Fragment::empty();
    for wrapper in wrappers.iter().rev() {
        if content.size() > 0 && wrapper.node_type().is_some_and(|ty| !ty.valid_content(&content)) {
            return;
        }
        content = singleton(wrapper.copy(Some(content)));
    }
    transform.maybe_step(Step::replace_around(
        range.start, range.end, range.start, range.end,
        Slice::new(content, 0, 0), wrappers.len(), true,
    ));
}

pub fn can_split(doc: &Node, pos: usize, depth: usize, types_after: Option<&[Option<Node>]>) -> bool {
    let Ok(resolved) = doc.resolve(pos) else { return false };
    if depth == 0 || depth > resolved.depth || resolved.parent().is_isolating() {
        return false;
    }
    let parent = resolved.parent();
    let index = resolved.index(resolved.depth);
    let empty = Fragment::empty();
    let types = types_after.unwrap_or_default();
    let inner = types.last().and_then(Option::as_ref).unwrap_or(parent);
    if !parent.can_replace(index, parent.child_count(), &empty)
        || !valid_content(inner, &fragment_by_index(parent, index, parent.child_count()))
    {
        return false;
    }
    let base = resolved.depth - depth;
    for level in (base + 1..resolved.depth).rev() {
        let node = resolved.node(level);
        let index = resolved.index(level);
        if node.is_isolating() || index >= node.child_count() {
            return false;
        }
        let mut rest = fragment_by_index(node, index, node.child_count());
        if let Some(override_node) = types.get(level - base).and_then(Option::as_ref) {
            rest = rest.replace_child(0, override_node.copy(Some(Fragment::empty())));
        }
        let after = types.get(level - base - 1).and_then(Option::as_ref).unwrap_or(node);
        if !node.can_replace(index + 1, node.child_count(), &empty) || !valid_content(after, &rest) {
            return false;
        }
    }
    let index = resolved.index_after(base);
    let base_type = types.first().and_then(Option::as_ref).unwrap_or_else(|| resolved.node(base + 1));
    resolved.node(base).can_replace_with(index, index, base_type.type_name(), &[])
}

fn valid_content(node: &Node, content: &Fragment) -> bool {
    node.node_type().is_none_or(|ty| ty.valid_content(content))
}

pub(crate) fn split(transform: &mut Transform, pos: usize, depth: usize, types_after: Option<&[Option<Node>]>) {
    let Ok(resolved) = transform.doc.resolve(pos) else { return };
    if depth == 0 || depth > resolved.depth {
        return;
    }
    let (mut left, mut right) = (Fragment::empty(), Fragment::empty());
    let base = resolved.depth - depth;
    for level in (base + 1..=resolved.depth).rev() {
        let node = resolved.node(level);
        left = singleton(node.copy(Some(left)));
        let override_node = types_after.and_then(|types| types.get(level - base - 1)).and_then(Option::as_ref);
        right = singleton(override_node.unwrap_or(node).copy(Some(right)));
    }
    let slice = Slice::new(left.append(&right), depth, depth);
    transform.maybe_step(Step::replace_structure(pos, pos, slice));
}

pub fn can_join(doc: &Node, pos: usize) -> bool {
    let Ok(resolved) = doc.resolve(pos) else { return false };
    let index = resolved.index(resolved.depth);
    joinable(resolved.node_before().as_ref(), resolved.node_after().as_ref())
        && resolved.parent().can_replace(index, index + 1, &Fragment::empty())
}

fn joinable(left: Option<&Node>, right: Option<&Node>) -> bool {
    let (Some(left), Some(right)) = (left, right) else { return false };
    if left.is_leaf() {
        return false;
    }
    let Some(mut matched) = left.content_match_at(left.child_count()) else {
        return compatible(left, right) || (left.is_textblock() && right.is_textblock());
    };
    let linebreak = left.schema().and_then(|schema| schema.linebreak_replacement());
    for child in &right.content().children {
        let type_name = if linebreak.is_some_and(|linebreak| child.type_name() == linebreak.name) {
            "text"
        } else {
            child.type_name()
        };
        let Some(next) = matched.match_type(type_name) else { return false };
        if child.marks().iter().any(|mark| !left.allows_mark_type(&mark.type_name)) {
            return false;
        }
        matched = next;
    }
    matched.valid_end()
}

pub fn join_point(doc: &Node, pos: usize, dir: i32) -> Option<usize> {
    let resolved = doc.resolve(pos).ok()?;
    let mut point = pos;
    for depth in (0..=resolved.depth).rev() {
        let parent = resolved.node(depth);
        let mut index = resolved.index(depth);
        let (before_node, after_node) = if depth == resolved.depth {
            (resolved.node_before(), resolved.node_after())
        } else if dir > 0 {
            index += 1;
            (Some(resolved.node(depth + 1).clone()), parent.content().children.get(index).cloned())
        } else {
            (
                index.checked_sub(1).and_then(|index| parent.content().children.get(index)).cloned(),
                Some(resolved.node(depth + 1).clone()),
            )
        };
        if before_node.as_ref().is_some_and(|node| !node.is_textblock())
            && joinable(before_node.as_ref(), after_node.as_ref())
            && parent.can_replace(index, index + 1, &Fragment::empty())
        {
            return Some(point);
        }
        if depth > 0 {
            point = if dir < 0 { resolved.before(depth) } else { resolved.after(depth) };
        }
    }
    None
}

pub(crate) fn join(transform: &mut Transform, pos: usize, depth: usize) {
    let (Some(start), Some(end)) = (pos.checked_sub(depth), pos.checked_add(depth)) else { return };
    if depth == 0 || end > transform.doc.content().size() {
        return;
    }
    let Ok(left) = transform.doc.resolve(start) else { return };
    let parent_type = left.parent().node_type().cloned();
    let linebreak = transform.doc.schema().and_then(|schema| schema.linebreak_replacement());
    let conversion = parent_type.as_ref().filter(|ty| ty.inline_content()).and_then(|ty| newline_conversion(ty, linebreak));
    let map_from = transform.steps.len();
    if conversion == Some(false) {
        if let Ok(right) = transform.doc.resolve(end) {
            if right.depth > 0 {
                translate_newlines(transform, right.parent(), right.before(right.depth), map_from, false);
            }
        }
    }
    if let Some(parent_type) = parent_type.as_ref().filter(|ty| ty.inline_content()) {
        clear_incompatible(transform, end - 1, parent_type, left.parent().content_match_at(left.index(left.depth)), conversion.is_none());
    }
    let mapping = transform.mapping.slice(map_from, transform.steps.len());
    let start = mapping.map(start, 1);
    let end = mapping.map(end, -1);
    transform.maybe_step(Step::replace_structure(start, end, Slice::empty()));
    if conversion == Some(true) {
        if let Ok(full) = transform.doc.resolve(start) {
            if full.depth > 0 {
                let step_count = transform.steps.len();
                translate_newlines(transform, full.parent(), full.before(full.depth), step_count, true);
            }
        }
    }
}

pub fn insert_point(doc: &Node, pos: usize, type_name: &str) -> Option<usize> {
    let resolved = doc.resolve(pos).ok()?;
    let index = resolved.index(resolved.depth);
    if resolved.parent().can_replace_with(index, index, type_name, &[]) {
        return Some(pos);
    }
    if resolved.parent_offset == 0 {
        for depth in (0..resolved.depth).rev() {
            let index = resolved.index(depth);
            if resolved.node(depth).can_replace_with(index, index, type_name, &[]) {
                return Some(resolved.before(depth + 1));
            }
            if index > 0 {
                return None;
            }
        }
    }
    if resolved.parent_offset == resolved.parent().content().size() {
        for depth in (0..resolved.depth).rev() {
            let index = resolved.index_after(depth);
            if resolved.node(depth).can_replace_with(index, index, type_name, &[]) {
                return Some(resolved.after(depth + 1));
            }
            if index < resolved.node(depth).child_count() {
                return None;
            }
        }
    }
    None
}

pub fn drop_point(doc: &Node, pos: usize, slice: &Slice) -> Option<usize> {
    let resolved = doc.resolve(pos).ok()?;
    if slice.content.size() == 0 {
        return Some(pos);
    }
    let mut content = &slice.content;
    for _ in 0..slice.open_start {
        content = content.children.first()?.content();
    }
    let passes = if slice.open_start == 0 && slice.size() > 0 { 2 } else { 1 };
    for pass in 0..passes {
        for depth in (0..=resolved.depth).rev() {
            let direction = if depth == resolved.depth {
                0
            } else if pos <= (resolved.start(depth + 1) + resolved.end(depth + 1)) / 2 {
                -1
            } else {
                1
            };
            let index = resolved.index(depth) + usize::from(direction > 0);
            let parent = resolved.node(depth);
            let fits = if pass == 0 {
                parent.can_replace(index, index, content)
            } else {
                parent.content_match_at(index).zip(parent.schema())
                    .and_then(|(matched, schema)| matched.find_wrapping(schema, content.children.first()?.type_name()))
                    .and_then(|wrappers| wrappers.into_iter().next())
                    .is_some_and(|outer| parent.can_replace_with(index, index, &outer, &[]))
            };
            if fits {
                return Some(match direction {
                    -1 => before(&resolved, depth + 1),
                    1 => after(&resolved, depth + 1),
                    _ => pos,
                });
            }
        }
    }
    None
}

pub(crate) fn set_block_type(
    transform: &mut Transform, from: usize, to: usize, type_name: &str, mut attrs: impl FnMut(&Node) -> Option<Value>,
) {
    let doc = transform.doc.clone();
    let Some(schema) = doc.schema() else { return };
    let Some(target) = schema.node_type(type_name).filter(|ty| ty.is_textblock()) else { return };
    let map_from = transform.steps.len();
    doc.nodes_between(from, to, |node, pos, _, _| {
        let attrs = attrs(node);
        if !node.is_textblock() {
            return true;
        }
        let Ok(updated) = target.try_create(attrs, None, Some(node.marks().to_vec())) else { return true };
        if node.same_markup(&updated) {
            return true;
        }
        let mapped = transform.mapping.slice(map_from, transform.steps.len()).map(pos, 1);
        let Ok(resolved) = transform.doc.resolve(mapped) else { return true };
        let index = resolved.index(resolved.depth);
        if !resolved.parent().can_replace_with(index, index + 1, type_name, &[]) {
            return true;
        }
        let conversion = newline_conversion(target, schema.linebreak_replacement());
        if conversion == Some(false) {
            translate_newlines(transform, node, pos, map_from, false);
        }
        clear_incompatible(transform, mapped, target, None, conversion.is_none());
        let mapping = transform.mapping.slice(map_from, transform.steps.len());
        let start = mapping.map(pos, 1);
        let end = mapping.map(pos + node.node_size(), 1);
        let slice = Slice::new(singleton(updated), 0, 0);
        transform.maybe_step(Step::replace_around(start, end, start + 1, end - 1, slice, 1, true));
        if conversion == Some(true) {
            translate_newlines(transform, node, pos, map_from, true);
        }
        false
    });
}

pub(crate) fn set_node_markup(
    transform: &mut Transform, pos: usize, type_name: Option<&str>, attrs: Option<Value>, marks: Option<Vec<Mark>>,
) {
    let Some(node) = node_at(&transform.doc, pos) else { return };
    if node.is_text() {
        return;
    }
    let type_name = type_name.unwrap_or_else(|| node.type_name());
    let marks = marks.unwrap_or_else(|| node.marks().to_vec());
    let updated = match node.schema().and_then(|schema| schema.node_type(type_name)) {
        Some(target) => {
            let Ok(template) = target.try_create(attrs, None, Some(marks)) else { return };
            template
        }
        None => Node::new(type_name, attrs, Fragment::empty(), marks),
    };
    let end = pos + node.node_size();
    if node.is_leaf() {
        transform.replace_with(pos, end, singleton(updated));
    } else if valid_content(&updated, node.content()) {
        let slice = Slice::new(singleton(updated), 0, 0);
        transform.maybe_step(Step::replace_around(pos, end, pos + 1, end - 1, slice, 1, true));
    }
}
