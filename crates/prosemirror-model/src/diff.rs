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
        if let (Some(left), Some(right)) = (left.text(), right.text()) {
            if left != right {
                let prefix = left.encode_utf16()
                    .zip(right.encode_utf16())
                    .take_while(|(a, b)| a == b)
                    .count();
                return Some(pos + prefix);
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
        if let (Some(left), Some(right)) = (left.text(), right.text()) {
            if left != right {
                let left: Vec<_> = left.encode_utf16().collect();
                let right: Vec<_> = right.encode_utf16().collect();
                let suffix = left.iter().rev()
                    .zip(right.iter().rev())
                    .take_while(|(a, b)| a == b)
                    .count();
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
