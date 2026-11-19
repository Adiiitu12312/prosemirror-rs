use crate::Fragment;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffEnd {
    pub a: usize,
    pub b: usize,
}

pub(crate) fn find_diff_start(a: &Fragment, b: &Fragment, mut pos: usize) -> Option<usize> {
    for (left, right) in a.children.iter().zip(&b.children) {
        if !left.same_markup(right) {
            return Some(pos);
        }
        if !left.same_text(right) {
            if let (Some(left), Some(right)) = (left.text_utf16(), right.text_utf16()) {
                let prefix = left.iter()
                    .zip(right.iter())
                    .take_while(|(a, b)| a == b)
                    .count();
                let inside_pair = prefix < right.len() && splits_surrogate_pair(&left, prefix);
                return Some(pos + prefix - usize::from(inside_pair));
            }
        }
        if left.content().size() > 0 || right.content().size() > 0 {
            if let Some(inner) = find_diff_start(left.content(), right.content(), pos + 1) {
                return Some(inner);
            }
        }
        pos += left.node_size();
    }
    (a.child_count() != b.child_count()).then_some(pos)
}

pub(crate) fn find_diff_end(
    a: &Fragment,
    b: &Fragment,
    mut pos_a: usize,
    mut pos_b: usize,
) -> Option<DiffEnd> {
    for (left, right) in a.children.iter().rev().zip(b.children.iter().rev()) {
        if !left.same_markup(right) {
            return Some(DiffEnd { a: pos_a, b: pos_b });
        }
        if !left.same_text(right) {
            if let (Some(left), Some(right)) = (left.text_utf16(), right.text_utf16()) {
                let suffix = left.iter().rev()
                    .zip(right.iter().rev())
                    .take_while(|(a, b)| a == b)
                    .count();
                let inside_pair = suffix < right.len() && splits_surrogate_pair(&left, left.len() - suffix);
                let suffix = suffix - usize::from(inside_pair);
                return Some(DiffEnd { a: pos_a - suffix, b: pos_b - suffix });
            }
        }
        if left.content().size() > 0 || right.content().size() > 0 {
            if let Some(inner) = find_diff_end(left.content(), right.content(), pos_a - 1, pos_b - 1) {
                return Some(inner);
            }
        }
        pos_a -= left.node_size();
        pos_b -= right.node_size();
    }
    (a.child_count() != b.child_count()).then_some(DiffEnd { a: pos_a, b: pos_b })
}

fn splits_surrogate_pair(text: &[u16], at: usize) -> bool {
    at > 0 && at < text.len() && (0xd800..0xdc00).contains(&text[at - 1]) && (0xdc00..0xe000).contains(&text[at])
}
