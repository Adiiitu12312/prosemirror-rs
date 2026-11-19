//! Replacement and metadata span merging from prosemirror-changeset 2.4.0 change.ts.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span<D> {
    pub length: usize,
    pub data: D,
}

impl<D> Span<D> {
    pub fn new(length: usize, data: D) -> Self {
        Self { length, data }
    }

    pub fn total_length(spans: &[Self]) -> usize {
        spans.iter().map(|span| span.length).sum()
    }
}

impl<D: Clone> Span<D> {
    pub fn slice(spans: &[Self], from: usize, to: usize) -> Vec<Self> {
        if from == to {
            return Vec::new();
        }
        if from == 0 && to == Self::total_length(spans) {
            return spans.to_vec();
        }
        let mut result = Vec::new();
        let mut offset = 0;
        for span in spans {
            if offset >= to {
                break;
            }
            let end = offset + span.length;
            let length = to.min(end).saturating_sub(from.max(offset));
            if length > 0 {
                result.push(Self::new(length, span.data.clone()));
            }
            offset = end;
        }
        result
    }

    pub(crate) fn append(
        left: &mut Vec<Self>,
        right: &[Self],
        combine: &(impl Fn(&D, &D) -> Option<D> + ?Sized),
    ) {
        let mut start = 0;
        if let (Some(last), Some(first)) = (left.last_mut(), right.first()) {
            if let Some(joined) = combine(&last.data, &first.data) {
                last.length += first.length;
                last.data = joined;
                start = 1;
            }
        }
        left.extend_from_slice(&right[start..]);
    }
}

/// A replacement described in the original document (A) and the current document (B).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change<D> {
    pub from_a: usize,
    pub to_a: usize,
    pub from_b: usize,
    pub to_b: usize,
    pub deleted: Vec<Span<D>>,
    pub inserted: Vec<Span<D>>,
}

impl<D> Change<D> {
    pub fn new(
        from_a: usize,
        to_a: usize,
        from_b: usize,
        to_b: usize,
        deleted: Vec<Span<D>>,
        inserted: Vec<Span<D>>,
    ) -> Self {
        Self { from_a, to_a, from_b, to_b, deleted, inserted }
    }

    pub fn len_a(&self) -> usize {
        self.to_a - self.from_a
    }

    pub fn len_b(&self) -> usize {
        self.to_b - self.from_b
    }

    pub fn ranges(&self) -> [usize; 4] {
        [self.from_a, self.to_a, self.from_b, self.to_b]
    }

    fn offset(&self) -> isize {
        self.to_b as isize - self.to_a as isize
    }
}

impl<D: Clone> Change<D> {
    pub fn slice(&self, start_a: usize, end_a: usize, start_b: usize, end_b: usize) -> Self {
        if start_a == 0 && start_b == 0 && end_a == self.len_a() && end_b == self.len_b() {
            return self.clone();
        }
        Self::new(
            self.from_a + start_a,
            self.from_a + end_a,
            self.from_b + start_b,
            self.from_b + end_b,
            Span::slice(&self.deleted, start_a, end_a),
            Span::slice(&self.inserted, start_b, end_b),
        )
    }

    /// Merge sequences whose middle document is shared.
    pub fn merge(
        x: &[Self],
        y: &[Self],
        combine: &(impl Fn(&D, &D) -> Option<D> + ?Sized),
    ) -> Vec<Self> {
        if x.is_empty() {
            return y.to_vec();
        }
        if y.is_empty() {
            return x.to_vec();
        }
        let mut result = Vec::new();
        let (mut ix, mut iy) = (0, 0);
        while let (Some(a), Some(b)) = (x.get(ix), y.get(iy)) {
            if a.to_b < b.from_a {
                result.push(a.move_b(previous_offset(y, iy)));
                ix += 1;
            } else if b.to_a < a.from_b {
                result.push(b.move_a(-previous_offset(x, ix)));
                iy += 1;
            } else {
                let from_a = a.from_a.min(shift(b.from_a, -previous_offset(x, ix)));
                let from_b = b.from_b.min(shift(a.from_b, previous_offset(y, iy)));
                let mut merged = Self::new(from_a, from_a, from_b, from_b, Vec::new(), Vec::new());
                let mut pos = a.from_b.min(b.from_a);
                let (mut entered_x, mut entered_y) = (false, false);

                loop {
                    let cur_x = x.get(ix);
                    let cur_y = y.get(iy);
                    let in_x = cur_x.filter(|change| pos >= change.from_b);
                    let in_y = cur_y.filter(|change| pos >= change.from_a);
                    if in_x.is_none() && in_y.is_none() {
                        break;
                    }
                    let next_x = cur_x.map_or(usize::MAX, |c| if in_x.is_some() { c.to_b } else { c.from_b });
                    let next_y = cur_y.map_or(usize::MAX, |c| if in_y.is_some() { c.to_a } else { c.from_a });
                    let next = next_x.min(next_y);

                    if let Some(c) = in_x {
                        if pos == c.from_b && !entered_x {
                            Span::append(&mut merged.deleted, &c.deleted, combine);
                            merged.to_a += c.len_a();
                            entered_x = true;
                        }
                        if in_y.is_none() {
                            let retained = Span::slice(&c.inserted, pos - c.from_b, next - c.from_b);
                            Span::append(&mut merged.inserted, &retained, combine);
                            merged.to_b += next - pos;
                        }
                    }
                    if let Some(c) = in_y {
                        if pos == c.from_a && !entered_y {
                            Span::append(&mut merged.inserted, &c.inserted, combine);
                            merged.to_b += c.len_b();
                            entered_y = true;
                        }
                        if in_x.is_none() {
                            let retained = Span::slice(&c.deleted, pos - c.from_a, next - c.from_a);
                            Span::append(&mut merged.deleted, &retained, combine);
                            merged.to_a += next - pos;
                        }
                    }
                    if in_x.is_some_and(|c| next == c.to_b) {
                        ix += 1;
                        entered_x = false;
                    }
                    if in_y.is_some_and(|c| next == c.to_a) {
                        iy += 1;
                        entered_y = false;
                    }
                    pos = next;
                }
                if merged.len_a() > 0 || merged.len_b() > 0 {
                    result.push(merged);
                }
            }
        }
        result.extend(x[ix..].iter().map(|change| change.move_b(previous_offset(y, iy))));
        result.extend(y[iy..].iter().map(|change| change.move_a(-previous_offset(x, ix))));
        result
    }

    fn move_a(&self, delta: isize) -> Self {
        let mut change = self.clone();
        change.from_a = shift(change.from_a, delta);
        change.to_a = shift(change.to_a, delta);
        change
    }

    fn move_b(&self, delta: isize) -> Self {
        let mut change = self.clone();
        change.from_b = shift(change.from_b, delta);
        change.to_b = shift(change.to_b, delta);
        change
    }
}

fn previous_offset<D>(changes: &[Change<D>], i: usize) -> isize {
    i.checked_sub(1).and_then(|previous| changes.get(previous)).map_or(0, Change::offset)
}

pub(crate) fn shift(pos: usize, delta: isize) -> usize {
    pos.checked_add_signed(delta).expect("valid change coordinates")
}
