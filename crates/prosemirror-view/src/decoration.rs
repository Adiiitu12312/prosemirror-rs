//! Document decorations from prosemirror-view@1.42.9/src/decoration.ts.
//! Widget IDs identify widgets supplied by the rendering host.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use prosemirror_model::Node;
use prosemirror_transform::{Mappable, Mapping};
use serde_json::Value;

pub type DecorationAttrs = BTreeMap<String, String>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecorationType {
    Widget { id: u64, side: i32 },
    Inline { attrs: DecorationAttrs, inclusive_start: bool, inclusive_end: bool },
    Node { attrs: DecorationAttrs },
}

#[derive(Clone, Debug)]
pub struct Decoration {
    pub from: usize,
    pub to: usize,
    pub kind: DecorationType,
    spec: Arc<Value>,
}

impl PartialEq for Decoration {
    fn eq(&self, other: &Self) -> bool {
        if self.from != other.from || self.to != other.to {
            return false;
        }
        if matches!(self.kind, DecorationType::Widget { .. }) && matches!(other.kind, DecorationType::Widget { .. }) {
            if let (Some(left), Some(right)) = (self.spec["key"].as_str(), other.spec["key"].as_str()) {
                if !left.is_empty() && left == right {
                    return true;
                }
            }
        }
        self.kind == other.kind && self.spec == other.spec
    }
}

impl Eq for Decoration {}

impl Decoration {
    pub fn widget(pos: usize, id: u64, spec: Value) -> Self {
        let side = spec["side"].as_i64().and_then(|side| side.try_into().ok()).unwrap_or(0);
        Self::new(pos, pos, DecorationType::Widget { id, side }, spec)
    }

    pub fn inline(from: usize, to: usize, attrs: DecorationAttrs, spec: Value) -> Self {
        let inclusive_start = spec["inclusiveStart"].as_bool().unwrap_or(false);
        let inclusive_end = spec["inclusiveEnd"].as_bool().unwrap_or(false);
        Self::new(from, to, DecorationType::Inline { attrs, inclusive_start, inclusive_end }, spec)
    }

    pub fn node(from: usize, to: usize, attrs: DecorationAttrs, spec: Value) -> Self {
        Self::new(from, to, DecorationType::Node { attrs }, spec)
    }

    fn new(from: usize, to: usize, kind: DecorationType, spec: Value) -> Self {
        let spec = if spec.is_null() { Value::Object(Default::default()) } else { spec };
        Self { from, to, kind, spec: Arc::new(spec) }
    }

    pub fn spec(&self) -> &Value {
        &self.spec
    }

    fn at(&self, from: usize, to: usize) -> Self {
        Self { from, to, kind: self.kind.clone(), spec: Arc::clone(&self.spec) }
    }

    pub fn map(&self, mapping: &impl Mappable) -> Option<Self> {
        match self.kind {
            DecorationType::Widget { side, .. } => {
                let mapped = mapping.map_result(self.from, if side < 0 { -1 } else { 1 });
                (!mapped.deleted()).then(|| self.at(mapped.pos, mapped.pos))
            }
            DecorationType::Inline { inclusive_start, inclusive_end, .. } => {
                let from = mapping.map(self.from, if inclusive_start { -1 } else { 1 });
                let to = mapping.map(self.to, if inclusive_end { 1 } else { -1 });
                (from < to).then(|| self.at(from, to))
            }
            DecorationType::Node { .. } => {
                let from = mapping.map_result(self.from, 1);
                let to = mapping.map_result(self.to, -1);
                (!from.deleted() && !to.deleted() && from.pos < to.pos)
                    .then(|| self.at(from.pos, to.pos))
            }
        }
    }

    fn valid(&self, parent: &Node) -> bool {
        match self.kind {
            DecorationType::Widget { .. } => true,
            DecorationType::Inline { .. } => self.from < self.to,
            DecorationType::Node { .. } => {
                let (index, offset) = parent.content().find_index(self.from);
                offset == self.from && parent.content().children.get(index).is_some_and(|child| {
                    !child.is_text() && offset + child.node_size() == self.to
                })
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DecorationSet(Arc<DecorationTree>);

#[derive(Debug, Default, PartialEq, Eq)]
struct DecorationTree {
    local: Vec<Decoration>,
    children: Vec<ChildDecorations>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ChildDecorations {
    pub from: usize,
    pub to: usize,
    pub decorations: DecorationSet,
}

impl DecorationSet {
    pub fn create(doc: &Node, decorations: Vec<Decoration>) -> Self {
        build_tree(doc, decorations, 0, &mut |_| {})
    }

    pub fn local(&self) -> &[Decoration] {
        &self.0.local
    }

    pub fn children(&self) -> &[ChildDecorations] {
        &self.0.children
    }

    pub fn is_empty(&self) -> bool {
        self.local().is_empty() && self.children().is_empty()
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn find(&self, start: Option<usize>, end: Option<usize>) -> Vec<Decoration> {
        self.find_where(start, end, |_| true)
    }

    pub fn find_where(
        &self, start: Option<usize>, end: Option<usize>, predicate: impl FnMut(&Value) -> bool,
    ) -> Vec<Decoration> {
        let mut result = Vec::new();
        self.collect(0, start.unwrap_or(0), end.unwrap_or(usize::MAX), &mut { predicate }, &mut result);
        result
    }

    fn collect(
        &self, offset: usize, start: usize, end: usize, predicate: &mut impl FnMut(&Value) -> bool,
        result: &mut Vec<Decoration>,
    ) {
        for span in self.local() {
            let (from, to) = (span.from + offset, span.to + offset);
            if from <= end && to >= start && predicate(span.spec()) {
                result.push(span.at(from, to));
            }
        }
        for child in self.children() {
            if child.from + offset < end && child.to + offset > start {
                child.decorations.collect(offset + child.from + 1, start, end, predicate, result);
            }
        }
    }

    pub fn map(&self, mapping: &Mapping, doc: &Node) -> Self {
        self.map_with(mapping, doc, |_| {})
    }

    pub fn map_with(&self, mapping: &Mapping, doc: &Node, mut on_remove: impl FnMut(&Value)) -> Self {
        if self.is_empty() || mapping.maps.is_empty() {
            return self.clone();
        }
        let mapped = self.find(None, None).into_iter().filter_map(|deco| {
            let mapped = deco.map(mapping);
            if mapped.is_none() {
                on_remove(deco.spec());
            }
            mapped
        }).collect();
        let mut candidates = HashMap::new();
        self.collect_reuse_candidates(mapping, 0, &mut candidates);
        let mut result = build_tree(doc, mapped, 0, &mut on_remove);
        result.reuse_children(0, &candidates);
        result
    }

    fn collect_reuse_candidates<'a>(
        &'a self, mapping: &Mapping, offset: usize, candidates: &mut HashMap<(usize, usize), &'a Self>,
    ) {
        for child in self.children() {
            let start = offset + child.from;
            let from = mapping.map_result(start, 1);
            let to = mapping.map_result(offset + child.to, -1);
            if !from.deleted() && !to.deleted() && from.pos < to.pos {
                candidates.insert((from.pos, to.pos), &child.decorations);
            }
            child.decorations.collect_reuse_candidates(mapping, start + 1, candidates);
        }
    }

    fn reuse_children(&mut self, offset: usize, candidates: &HashMap<(usize, usize), &Self>) {
        for child in &mut Arc::get_mut(&mut self.0).expect("newly allocated tree").children {
            let start = offset + child.from;
            if let Some(candidate) = candidates.get(&(start, offset + child.to)) {
                if **candidate == child.decorations {
                    child.decorations = (*candidate).clone();
                    continue;
                }
            }
            child.decorations.reuse_children(start + 1, candidates);
        }
    }

    pub fn add(&self, doc: &Node, decorations: Vec<Decoration>) -> Self {
        if decorations.is_empty() {
            return self.clone();
        }
        let mut combined = self.find(None, None);
        combined.extend(decorations);
        let mut result = Self::create(doc, combined);
        let mut candidates = HashMap::new();
        self.collect_reuse_candidates(&Mapping::new(), 0, &mut candidates);
        result.reuse_children(0, &candidates);
        result
    }

    pub fn remove(&self, decorations: &[Decoration]) -> Self {
        self.remove_at(decorations, 0)
    }

    fn remove_at(&self, decorations: &[Decoration], offset: usize) -> Self {
        if decorations.is_empty() || self.is_empty() {
            return self.clone();
        }
        let local: Vec<_> = self.local().iter().filter(|span| {
            !decorations.contains(&span.at(span.from + offset, span.to + offset))
        }).cloned().collect();
        let mut changed = local.len() != self.local().len();
        let children = self.children().iter().filter_map(|child| {
            let found: Vec<_> = decorations.iter().filter(|span| {
                span.from > child.from + offset && span.to < child.to + offset
            }).cloned().collect();
            let next = child.decorations.remove_at(&found, offset + child.from + 1);
            changed |= !next.ptr_eq(&child.decorations);
            (!next.is_empty()).then_some(ChildDecorations { from: child.from, to: child.to, decorations: next })
        }).collect();
        if changed { Self(Arc::new(DecorationTree { local, children })) } else { self.clone() }
    }

    pub fn for_child(&self, offset: usize, node: &Node) -> DecorationGroup {
        if node.is_leaf() {
            return DecorationGroup::default();
        }
        let start = offset + 1;
        let end = start + node.content().size();
        let mut local: Vec<_> = self.local().iter().filter_map(|span| {
            let from = start.max(span.from);
            let to = end.min(span.to);
            (matches!(span.kind, DecorationType::Inline { .. }) && from < to)
                .then(|| span.at(from - start, to - start))
        }).collect();
        local.sort_by_key(|span| (span.from, span.to));
        let mut members = Vec::new();
        if !local.is_empty() {
            members.push(Self(Arc::new(DecorationTree { local, children: Vec::new() })));
        }
        if let Ok(index) = self.children().binary_search_by_key(&offset, |child| child.from) {
            members.push(self.children()[index].decorations.clone());
        }
        DecorationGroup { members }
    }

    pub fn locals(&self, node: &Node) -> Vec<Decoration> {
        let local = self.locals_inner(node);
        remove_overlap(&local).into_owned()
    }

    fn locals_inner(&self, node: &Node) -> Vec<Decoration> {
        self.local().iter().filter(|span| {
            node.inline_content() || !matches!(span.kind, DecorationType::Inline { .. })
        }).cloned().collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DecorationGroup {
    pub members: Vec<DecorationSet>,
}

impl DecorationGroup {
    pub fn map(&self, mapping: &Mapping, node: &Node) -> Self {
        Self { members: self.members.iter().map(|set| set.map(mapping, node)).filter(|set| !set.is_empty()).collect() }
    }

    pub fn for_child(&self, offset: usize, node: &Node) -> Self {
        Self { members: self.members.iter().flat_map(|set| set.for_child(offset, node).members).collect() }
    }

    pub fn locals(&self, node: &Node) -> Vec<Decoration> {
        let mut combined: Vec<_> = self.members.iter().flat_map(|member| member.locals_inner(node)).collect();
        combined.sort_by_key(|span| (span.from, span.to));
        remove_overlap(&combined).into_owned()
    }
}

fn build_tree(
    node: &Node, spans: Vec<Decoration>, offset: usize, on_remove: &mut impl FnMut(&Value),
) -> DecorationSet {
    let mut spans: Vec<_> = spans.into_iter().map(Some).collect();
    let mut children = Vec::new();
    node.content().for_each(|child, local_start, _| {
        if child.is_leaf() {
            return;
        }
        let start = offset + local_start;
        let end = start + child.node_size();
        let inside: Vec<_> = spans.iter_mut().filter_map(|span| {
            if span.as_ref().is_some_and(|span| span.from > start && span.to < end) { span.take() } else { None }
        }).collect();
        if !inside.is_empty() {
            let subtree = build_tree(child, inside, start + 1, on_remove);
            if !subtree.is_empty() {
                children.push(ChildDecorations { from: local_start, to: local_start + child.node_size(), decorations: subtree });
            }
        }
    });
    let mut local: Vec<_> = spans.into_iter().flatten().filter_map(|span| {
        let span = span.at(span.from - offset, span.to - offset);
        if span.valid(node) {
            Some(span)
        } else {
            on_remove(span.spec());
            None
        }
    }).collect();
    local.sort_by_key(|span| (span.from, span.to));
    DecorationSet(Arc::new(DecorationTree { local, children }))
}

pub fn remove_overlap(spans: &[Decoration]) -> Cow<'_, [Decoration]> {
    let mut working = Cow::Borrowed(spans);
    let mut i = 0;
    while i + 1 < working.len() {
        let span = working[i].clone();
        if span.from != span.to {
            let mut j = i + 1;
            while j < working.len() {
                let next = &working[j];
                if next.from == span.from {
                    if next.to != span.to {
                        let first = next.at(next.from, span.to);
                        let rest = next.at(span.to, next.to);
                        working.to_mut()[j] = first;
                        insert_ahead(working.to_mut(), j + 1, rest);
                    }
                } else {
                    if next.from < span.to {
                        let pos = next.from;
                        working.to_mut()[i] = span.at(span.from, pos);
                        insert_ahead(working.to_mut(), j, span.at(pos, span.to));
                    }
                    break;
                }
                j += 1;
            }
        }
        i += 1;
    }
    working
}

fn insert_ahead(spans: &mut Vec<Decoration>, mut at: usize, span: Decoration) {
    while at < spans.len() && (span.from, span.to) > (spans[at].from, spans[at].to) {
        at += 1;
    }
    spans.insert(at, span);
}
