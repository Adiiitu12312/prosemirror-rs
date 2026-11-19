//! Tracking changes from position maps, ported from prosemirror-changeset 2.4.0 changeset.ts.

use std::sync::Arc;

use prosemirror_model::{Fragment, Node};
use prosemirror_transform::StepMap;

use crate::change::shift;
use crate::diff::{compute_diff, DefaultEncoder, TokenEncoder};
use crate::{Change, Span};

type Combine<D> = dyn Fn(&D, &D) -> Option<D> + Send + Sync;

struct Config<D, E> {
    start_doc: Node,
    combine: Box<Combine<D>>,
    encoder: E,
}

/// A compact record of edits from the starting document to the current document.
pub struct ChangeSet<D, E = DefaultEncoder> {
    config: Arc<Config<D, E>>,
    pub changes: Arc<[Change<D>]>,
}

impl<D, E> Clone for ChangeSet<D, E> {
    fn clone(&self) -> Self {
        Self { config: Arc::clone(&self.config), changes: Arc::clone(&self.changes) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangedRange {
    pub from: isize,
    pub to: isize,
}

impl<D: Clone + PartialEq> ChangeSet<D> {
    pub fn create(doc: Node) -> Self {
        Self::with_config(doc, |a, b| (a == b).then(|| a.clone()), DefaultEncoder, Vec::new())
    }

    /// Diff with the default token encoder.
    pub fn compute_diff(a: &Fragment, b: &Fragment, range: &Change<D>) -> Vec<Change<D>> {
        compute_diff(a, b, range, &DefaultEncoder)
    }
}

impl<D: Clone, E: TokenEncoder> ChangeSet<D, E> {
    /// Start tracking changes with a particular metadata combiner and token encoder.
    pub fn with_config(
        start_doc: Node,
        combine: impl Fn(&D, &D) -> Option<D> + Send + Sync + 'static,
        encoder: E,
        changes: Vec<Change<D>>,
    ) -> Self {
        Self {
            config: Arc::new(Config { start_doc, combine: Box::new(combine), encoder }),
            changes: changes.into(),
        }
    }

    pub fn start_doc(&self) -> &Node {
        &self.config.start_doc
    }

    pub fn add_steps(&self, new_doc: &Node, maps: &[StepMap], data: D) -> Self {
        self.add_steps_with(new_doc, maps, |_| data.clone())
    }

    pub fn add_steps_by_map(&self, new_doc: &Node, maps: &[StepMap], data: &[D]) -> Self {
        assert_eq!(maps.len(), data.len(), "a metadata value is required for each map");
        self.add_steps_with(new_doc, maps, |index| data[index].clone())
    }

    fn add_steps_with(&self, new_doc: &Node, maps: &[StepMap], data: impl Fn(usize) -> D) -> Self {
        let mut step_changes = Vec::new();
        for (index, map) in maps.iter().enumerate() {
            let data = data(index);
            let mut offset = 0;
            map.for_each(|from_a, to_a, from_b, to_b| {
                let (deleted, inserted) = (to_a - from_a, to_b - from_b);
                step_changes.push(Change::new(
                    shift(from_a, offset),
                    shift(to_a, offset),
                    from_b,
                    to_b,
                    (deleted != 0).then(|| Span::new(deleted, data.clone())).into_iter().collect(),
                    (inserted != 0).then(|| Span::new(inserted, data.clone())).into_iter().collect(),
                ));
                offset = inserted as isize - deleted as isize;
            });
        }
        if step_changes.is_empty() {
            return self.clone();
        }

        let combine = self.config.combine.as_ref();
        let added = merge_all(&step_changes, combine);
        let merged = Change::merge(&self.changes, &added, combine);
        let mut changes = Vec::new();
        for change in merged {
            if change.len_a() == 0 || change.len_b() == 0
                || !added.iter().any(|r| r.to_b > change.from_b && r.from_b < change.to_b)
            {
                changes.push(change);
                continue;
            }
            let diff = compute_diff(
                self.config.start_doc.content(), new_doc.content(), &change, &self.config.encoder,
            );
            if matches!(diff.as_slice(), [edit] if edit.from_b == 0 && edit.to_b == change.len_b()) {
                changes.push(change);
            } else {
                changes.extend(diff);
            }
        }
        Self { config: Arc::clone(&self.config), changes: changes.into() }
    }

    pub fn map(&self, mut update: impl FnMut(&Span<D>) -> D) -> Self {
        let changes = self.changes.iter().map(|change| {
            let mut map_span = |span: &Span<D>| Span::new(span.length, update(span));
            Change::new(
                change.from_a,
                change.to_a,
                change.from_b,
                change.to_b,
                change.deleted.iter().map(&mut map_span).collect(),
                change.inserted.iter().map(&mut map_span).collect(),
            )
        }).collect::<Vec<_>>();
        Self { config: Arc::clone(&self.config), changes: changes.into() }
    }
}

impl<D: PartialEq, E> ChangeSet<D, E> {
    /// Compare change sets in the later document's coordinates.
    pub fn changed_range(&self, other: &Self, maps: &[StepMap]) -> Option<ChangedRange> {
        if Arc::ptr_eq(&self.config, &other.config) && Arc::ptr_eq(&self.changes, &other.changes) {
            return None;
        }
        let touched = end_range(maps.iter()).map(|range_b| {
            let inverse: Vec<_> = maps.iter().rev().map(StepMap::invert).collect();
            let range_a = end_range(inverse.iter()).expect("inverse has the same maps");
            (range_a, range_b)
        });
        let mut changed = touched.map(|(_, (from, to))| ChangedRange { from: from as isize, to: to as isize });
        let map = |pos: usize| {
            let pos = pos as isize;
            match touched {
                Some(((from_a, to_a), (from_b, to_b))) if pos > from_a as isize => {
                    pos + (to_b - from_b) as isize - (to_a - from_a) as isize
                }
                _ => pos,
            }
        };

        let (mut ia, mut ib) = (0, 0);
        while let (Some(a), Some(b)) = (self.changes.get(ia), other.changes.get(ib)) {
            let (from, to) = (map(a.from_b), map(a.to_b));
            if from == b.from_b as isize && to == b.to_b as isize
                && a.deleted == b.deleted && a.inserted == b.inserted
            {
                ia += 1;
                ib += 1;
                continue;
            }
            let (start, end) = if from >= b.from_b as isize {
                ib += 1;
                (b.from_b as isize, b.to_b as isize)
            } else {
                ia += 1;
                (from, to)
            };
            changed = Some(changed.map_or(
                ChangedRange { from: start, to: end },
                |range| ChangedRange { from: range.from.min(start), to: range.to.max(end) },
            ));
        }
        changed.filter(|range| range.from <= range.to)
    }
}

fn merge_all<D: Clone>(changes: &[Change<D>], combine: &Combine<D>) -> Vec<Change<D>> {
    if changes.len() < 2 {
        return changes.to_vec();
    }
    let (left, right) = changes.split_at(changes.len() / 2);
    Change::merge(&merge_all(left, combine), &merge_all(right, combine), combine)
}

fn end_range<'a>(maps: impl Iterator<Item = &'a StepMap>) -> Option<(usize, usize)> {
    let mut bounds = None;
    for map in maps {
        bounds = bounds.map(|(from, to)| (map.map(from, -1), map.map(to, 1)));
        map.for_each(|_, _, start, end| {
            bounds = Some(bounds.map_or((start, end), |(from, to)| (from.min(start), to.max(end))));
        });
    }
    bounds
}
