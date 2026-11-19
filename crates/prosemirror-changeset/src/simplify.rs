//! Word-boundary simplification from prosemirror-changeset 2.4.0 simplify.ts.

use prosemirror_model::{Fragment, Node};

use crate::{Change, Span};

const MAX_SIMPLIFY_DISTANCE: usize = 30;

pub fn simplify_changes<D: Clone + PartialEq>(changes: &[Change<D>], doc: &Node) -> Vec<Change<D>> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < changes.len() {
        let start = i;
        while i + 1 < changes.len() && changes[i + 1].from_b <= changes[i].to_b + MAX_SIMPLIFY_DISTANCE {
            i += 1;
        }
        simplify_adjacent(&changes[start..=i], doc, &mut result);
        i += 1;
    }
    result
}

fn simplify_adjacent<D: Clone + PartialEq>(
    changes: &[Change<D>],
    doc: &Node,
    result: &mut Vec<Change<D>>,
) {
    let start = changes[0].from_b.saturating_sub(MAX_SIMPLIFY_DISTANCE);
    let end = doc.content().size().min(changes[changes.len() - 1].to_b + MAX_SIMPLIFY_DISTANCE);
    let mut text = Vec::new();
    collect_text(doc.content(), start, end, &mut text);

    let letter = |pos: usize| pos.checked_sub(start)
        .and_then(|index| text.get(index)).copied().is_some_and(is_letter);

    let mut i = 0;
    while i < changes.len() {
        let first = i;
        let mut deleted = changes[i].len_a();
        let mut inserted = changes[i].len_b();
        while let Some(next) = changes.get(i + 1) {
            let previous_end = changes[i].to_b;
            let mut prev_letter = previous_end != end && previous_end.checked_sub(1).is_some_and(letter);
            let mut boundary = false;
            for pos in previous_end..next.from_b {
                let next_letter = pos != end && letter(pos);
                if (!prev_letter || !next_letter) && pos != changes[first].from_b {
                    boundary = true;
                    break;
                }
                prev_letter = next_letter;
            }
            if boundary {
                break;
            }
            deleted += next.len_a();
            inserted += next.len_b();
            i += 1;
        }
        if inserted > 0 && deleted > 0 && !(inserted == 1 && deleted == 1) {
            let (mut from, mut to) = (changes[first].from_b, changes[i].to_b);
            if from < end && letter(from) {
                while from > start && letter(from - 1) {
                    from -= 1;
                }
            }
            if to > start && letter(to - 1) {
                while to < end && letter(to) {
                    to += 1;
                }
            }
            let joined = fill_change(&changes[first..=i], from, to);
            if let Some(previous) = result.last_mut().filter(|ch| ch.to_a == joined.from_a) {
                previous.to_a = joined.to_a;
                previous.to_b = joined.to_b;
                previous.deleted.extend(joined.deleted);
                previous.inserted.extend(joined.inserted);
            } else {
                result.push(joined);
            }
        } else {
            result.extend_from_slice(&changes[first..=i]);
        }
        i += 1;
    }
}

fn is_letter(unit: u16) -> bool {
    if unit < 128 {
        // Preserve the upstream ASCII tables, including the punctuation between Z and a.
        (48..=57).contains(&unit) || (65..=122).contains(&unit)
    } else {
        char::from_u32(u32::from(unit)).is_some_and(char::is_alphabetic)
    }
}

fn collect_text(frag: &Fragment, start: usize, end: usize, result: &mut Vec<u16>) {
    let mut offset = 0;
    for child in &frag.children {
        if offset >= end {
            break;
        }
        let child_end = offset + child.node_size();
        let from = offset.max(start);
        let to = child_end.min(end);
        if from < to {
            if let Some(text) = child.text_utf16() {
                result.extend_from_slice(&text[from - offset..to - offset]);
            } else if child.is_leaf() {
                result.push(b' '.into());
            } else {
                if from == offset {
                    result.push(b' '.into());
                }
                collect_text(
                    child.content(),
                    from.saturating_sub(offset + 1),
                    child.content().size().min(end - offset),
                    result,
                );
                if to == child_end {
                    result.push(b' '.into());
                }
            }
        }
        offset = child_end;
    }
}

fn fill_change<D: Clone + PartialEq>(changes: &[Change<D>], from_b: usize, to_b: usize) -> Change<D> {
    let first = &changes[0];
    let last = &changes[changes.len() - 1];
    let from_a = first.from_a - (first.from_b - from_b);
    let to_a = last.to_a + (to_b - last.to_b);
    let mut combined = Change::new(from_a, to_a, from_b, to_b, Vec::new(), Vec::new());
    let mut deleted_data = &first.deleted.first().or_else(|| first.inserted.first())
        .expect("a replacement has metadata").data;
    let mut inserted_data = &first.inserted.first().or_else(|| first.deleted.first())
        .expect("a replacement has metadata").data;
    let combine = |a: &D, b: &D| (a == b).then(|| a.clone());
    let (mut pos_a, mut pos_b) = (from_a, from_b);
    for next in changes.iter().map(Some).chain(std::iter::once(None)) {
        let (end_a, end_b) = next.map_or((to_a, to_b), |ch| (ch.from_a, ch.from_b));
        if end_a > pos_a {
            Span::append(&mut combined.deleted, &[Span::new(end_a - pos_a, deleted_data.clone())], &combine);
        }
        if end_b > pos_b {
            Span::append(&mut combined.inserted, &[Span::new(end_b - pos_b, inserted_data.clone())], &combine);
        }
        let Some(next) = next else { break };
        Span::append(&mut combined.deleted, &next.deleted, &combine);
        Span::append(&mut combined.inserted, &next.inserted, &combine);
        // Equal spans coalesce without changing the data value at the boundary.
        if let Some(span) = next.deleted.last() {
            deleted_data = &span.data;
        }
        if let Some(span) = next.inserted.last() {
            inserted_data = &span.data;
        }
        pos_a = next.to_a;
        pos_b = next.to_b;
    }
    combined
}
