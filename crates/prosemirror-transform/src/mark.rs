//! Mark range operations, ported from prosemirror-transform/src/mark.ts.

use prosemirror_model::{ContentMatch, Fragment, Mark, Node, NodeType, Slice};

use crate::{replace::singleton, step::node_at, Step, Transform};

#[derive(Clone, Copy, Debug, Default)]
pub enum MarkFilter<'a> {
    #[default]
    All,
    Type(&'a str),
    Exact(&'a Mark),
}

pub(crate) fn add_mark(transform: &mut Transform, from: usize, to: usize, mark: Mark) {
    if from >= to || to > transform.doc.content().size() {
        return;
    }
    let mark = transform.doc.schema().and_then(|schema| schema.mark_type(&mark.type_name))
        .and_then(|ty| ty.try_create(mark.attrs.clone()).ok()).unwrap_or(mark);
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    transform.doc.nodes_between(from, to, |node, pos, parent, _| {
        if !node.is_inline() || mark.is_in_set(node.marks())
            || parent.is_some_and(|parent| !parent.allows_mark_type(&mark.type_name))
        {
            return true;
        }
        let start = pos.max(from);
        let end = (pos + node.node_size()).min(to);
        let new_set = Mark::add_to_set(node.marks(), mark.clone());
        for previous in node.marks() {
            if !previous.is_in_set(&new_set) {
                if let Some(Step::RemoveMark { to, mark, .. }) = removed.last_mut() {
                    if *to == start && mark == previous {
                        *to = end;
                        continue;
                    }
                }
                removed.push(Step::remove_mark(start, end, previous.clone()));
            }
        }
        if let Some(Step::AddMark { to, .. }) = added.last_mut() {
            if *to == start {
                *to = end;
                return true;
            }
        }
        added.push(Step::add_mark(start, end, mark.clone()));
        true
    });
    for step in removed.into_iter().chain(added) {
        transform.maybe_step(step);
    }
}

pub(crate) fn remove_mark(transform: &mut Transform, from: usize, to: usize, filter: MarkFilter<'_>) {
    if from >= to || to > transform.doc.content().size() {
        return;
    }
    struct Matched {
        mark: Mark,
        from: usize,
        to: usize,
        index: usize,
    }
    let mut matched: Vec<Matched> = Vec::new();
    let mut index = 0;
    transform.doc.nodes_between(from, to, |node, pos, _, _| {
        if !node.is_inline() {
            return true;
        }
        index += 1;
        let start = pos.max(from);
        let end = (pos + node.node_size()).min(to);
        for mark in node.marks() {
            let matches = match filter {
                MarkFilter::All => true,
                MarkFilter::Type(type_name) => mark.type_name == type_name,
                MarkFilter::Exact(expected) => mark == expected,
            };
            if matches {
                if let Some(previous) = matched.iter_mut().rev().find(|m| m.index == index - 1 && m.mark == *mark) {
                    previous.to = end;
                    previous.index = index;
                } else {
                    matched.push(Matched { mark: mark.clone(), from: start, to: end, index });
                }
            }
        }
        true
    });
    for entry in matched {
        transform.maybe_step(Step::remove_mark(entry.from, entry.to, entry.mark));
    }
}

pub(crate) fn clear_incompatible(
    transform: &mut Transform,
    pos: usize,
    parent_type: &NodeType,
    matched: Option<ContentMatch>,
    clear_newlines: bool,
) {
    let Some(node) = node_at(&transform.doc, pos).cloned() else { return };
    let Some(schema) = transform.doc.schema().cloned() else { return };
    let mut matched = matched.unwrap_or_else(|| parent_type.content_match().clone());
    let mut replacement_steps = Vec::new();
    let mut start = pos + 1;
    for child in &node.content().children {
        let end = start + child.node_size();
        if let Some(allowed) = matched.match_type(child.type_name()) {
            matched = allowed;
            for mark in child.marks().iter().filter(|mark| !parent_type.allows_mark_type(&mark.type_name)) {
                transform.maybe_step(Step::remove_mark(start, end, mark.clone()));
            }
            if clear_newlines && parent_type.whitespace() != "pre" {
                if let Some(text) = child.text() {
                    let ranges = newline_ranges(text);
                    if !ranges.is_empty() {
                        let space = schema.text(" ", Some(parent_type.allowed_marks(child.marks())));
                        let slice = Slice::new(singleton(space), 0, 0);
                        for (offset, length) in ranges {
                            replacement_steps.push(Step::replace(start + offset, start + offset + length, slice.clone()));
                        }
                    }
                }
            }
        } else {
            replacement_steps.push(Step::replace(start, end, Slice::empty()));
        }
        start = end;
    }

    if !matched.valid_end() {
        if let Some(fill) = matched.fill_before(&schema, &Fragment::empty(), true, 0) {
            transform.replace(start, start, Slice::new(fill, 0, 0));
        }
    }
    for step in replacement_steps.into_iter().rev() {
        transform.maybe_step(step);
    }
}

pub(crate) fn newline_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut units = text.encode_utf16().enumerate().peekable();
    let mut ranges = Vec::new();
    while let Some((offset, unit)) = units.next() {
        match unit {
            13 if units.peek().is_some_and(|(_, next)| *next == 10) => {
                units.next();
                ranges.push((offset, 2));
            }
            10 | 13 => ranges.push((offset, 1)),
            _ => {}
        }
    }
    ranges
}

pub(crate) fn translate_newlines(transform: &mut Transform, node: &Node, pos: usize, map_from: usize, into_breaks: bool) {
    let Some(schema) = transform.doc.schema().cloned() else { return };
    let Some(linebreak) = schema.linebreak_replacement() else { return };
    let mut start = pos + 1;
    for child in &node.content().children {
        if into_breaks {
            if let Some(text) = child.text() {
                for (offset, _) in newline_ranges(text) {
                    let mapped = transform.mapping.slice(map_from, transform.steps.len()).map(start + offset, 1);
                    transform.replace_with(mapped, mapped + 1, singleton(linebreak.create(None, None, None)));
                }
            }
        } else if child.type_name() == linebreak.name {
            let mapped = transform.mapping.slice(map_from, transform.steps.len()).map(start, 1);
            transform.replace_with(mapped, mapped + 1, singleton(schema.text("\n", None)));
        }
        start += child.node_size();
    }
}

pub(crate) fn newline_conversion(node_type: &NodeType, linebreak: Option<&NodeType>) -> Option<bool> {
    let linebreak = linebreak?;
    let preformatted = node_type.whitespace() == "pre";
    let supports_break = node_type.content_match().match_type(&linebreak.name).is_some();
    match (preformatted, supports_break) {
        (true, false) => Some(false),
        (false, true) => Some(true),
        _ => None,
    }
}
