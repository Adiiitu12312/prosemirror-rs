//! Atomic edits and step JSON, ported from the upstream step implementations.

use prosemirror_model::{replace_at_pos, Fragment, Mark, ModelError, Node, Schema, Slice};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::map::{Mappable, StepMap};

#[derive(Clone, Debug, PartialEq)]
pub enum StepResult {
    Ok(Node),
    Failed(String),
}

impl StepResult {
    pub fn ok(doc: Node) -> Self {
        Self::Ok(doc)
    }

    pub fn fail(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }

    pub fn from_replace(doc: &Node, from: usize, to: usize, slice: &Slice) -> Self {
        match replace_at_pos(doc, from, to, slice) {
            Ok(doc) => Self::Ok(doc),
            Err(ModelError::ReplaceError(message)) => Self::Failed(message),
            Err(error) => Self::fail(error.to_string()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "stepType")]
pub enum Step {
    #[serde(rename = "replace")]
    Replace {
        from: usize,
        to: usize,
        #[serde(default = "Slice::empty", skip_serializing_if = "empty_slice")]
        slice: Slice,
        #[serde(default, skip_serializing_if = "is_false")]
        structure: bool,
    },
    #[serde(rename = "replaceAround")]
    ReplaceAround {
        from: usize,
        to: usize,
        #[serde(rename = "gapFrom")]
        gap_from: usize,
        #[serde(rename = "gapTo")]
        gap_to: usize,
        insert: usize,
        #[serde(default = "Slice::empty", skip_serializing_if = "empty_slice")]
        slice: Slice,
        #[serde(default, skip_serializing_if = "is_false")]
        structure: bool,
    },
    #[serde(rename = "addMark")]
    AddMark { from: usize, to: usize, mark: Mark },
    #[serde(rename = "removeMark")]
    RemoveMark { from: usize, to: usize, mark: Mark },
    #[serde(rename = "addNodeMark")]
    AddNodeMark { pos: usize, mark: Mark },
    #[serde(rename = "removeNodeMark")]
    RemoveNodeMark { pos: usize, mark: Mark },
    #[serde(rename = "attr")]
    Attr {
        pos: usize,
        attr: String,
        #[serde(default)]
        value: Value,
    },
    #[serde(rename = "docAttr")]
    DocAttr {
        attr: String,
        #[serde(default)]
        value: Value,
    },
}

fn empty_slice(slice: &Slice) -> bool {
    slice.size() == 0
}

fn is_false(value: &bool) -> bool {
    !value
}

impl Step {
    pub fn from_json(schema: &Schema, json: &Value) -> Result<Self, ModelError> {
        let mut step: Self = serde_json::from_value(json.clone())
            .map_err(|error| ModelError::Serialization(error.to_string()))?;
        match &mut step {
            Self::Replace { slice, .. } | Self::ReplaceAround { slice, .. } => {
                *slice = schema.slice_from_json(&json["slice"])?;
            }
            Self::AddMark { mark, .. } | Self::RemoveMark { mark, .. }
            | Self::AddNodeMark { mark, .. } | Self::RemoveNodeMark { mark, .. } => {
                *mark = schema.mark_from_json(&json["mark"])?;
            }
            Self::Attr { .. } | Self::DocAttr { .. } => {}
        }
        Ok(step)
    }

    pub fn replace(from: usize, to: usize, slice: Slice) -> Self {
        Self::Replace { from, to, slice, structure: false }
    }

    pub fn replace_structure(from: usize, to: usize, slice: Slice) -> Self {
        Self::Replace { from, to, slice, structure: true }
    }

    pub fn replace_around(
        from: usize,
        to: usize,
        gap_from: usize,
        gap_to: usize,
        slice: Slice,
        insert: usize,
        structure: bool,
    ) -> Self {
        Self::ReplaceAround { from, to, gap_from, gap_to, insert, slice, structure }
    }

    pub fn add_mark(from: usize, to: usize, mark: Mark) -> Self {
        Self::AddMark { from, to, mark }
    }

    pub fn remove_mark(from: usize, to: usize, mark: Mark) -> Self {
        Self::RemoveMark { from, to, mark }
    }

    pub fn add_node_mark(pos: usize, mark: Mark) -> Self {
        Self::AddNodeMark { pos, mark }
    }

    pub fn remove_node_mark(pos: usize, mark: Mark) -> Self {
        Self::RemoveNodeMark { pos, mark }
    }

    pub fn attr(pos: usize, attr: impl Into<String>, value: Value) -> Self {
        Self::Attr { pos, attr: attr.into(), value }
    }

    pub fn doc_attr(attr: impl Into<String>, value: Value) -> Self {
        Self::DocAttr { attr: attr.into(), value }
    }

    pub fn apply(&self, doc: &Node) -> StepResult {
        match self {
            Self::Replace { from, to, slice, structure } => {
                if !valid_range(doc, *from, *to) {
                    return StepResult::fail("Position out of bounds");
                }
                if *structure && content_between(doc, *from, *to) {
                    return StepResult::fail("Structure replace would overwrite content");
                }
                StepResult::from_replace(doc, *from, *to, slice)
            }
            Self::ReplaceAround { from, to, gap_from, gap_to, insert, slice, structure } => {
                if !valid_range(doc, *from, *to)
                    || !(*from <= *gap_from && *gap_from <= *gap_to && *gap_to <= *to)
                {
                    return StepResult::fail("Position out of bounds");
                }
                if *structure && (content_between(doc, *from, *gap_from)
                    || content_between(doc, *gap_to, *to))
                {
                    return StepResult::fail("Structure gap-replace would overwrite content");
                }
                let gap = match doc.slice(*gap_from, *gap_to, false) {
                    Ok(gap) => gap,
                    Err(error) => return StepResult::fail(error.to_string()),
                };
                if gap.open_start != 0 || gap.open_end != 0 {
                    return StepResult::fail("Gap is not a flat range");
                }
                if *insert > slice.size() {
                    return StepResult::fail("Content does not fit in gap");
                }
                match slice.insert_at(*insert, &gap.content) {
                    Some(inserted) => StepResult::from_replace(doc, *from, *to, &inserted),
                    None => StepResult::fail("Content does not fit in gap"),
                }
            }
            Self::AddMark { from, to, mark } => {
                let mark = crate::mark::bind_mark(doc, mark);
                map_marks(doc, *from, *to, |node, parent| {
                    if node.is_atom() && parent.allows_mark_type(&mark.type_name) {
                        Mark::add_to_set(node.marks(), mark.clone())
                    } else {
                        node.marks().to_vec()
                    }
                })
            }
            Self::RemoveMark { from, to, mark } => {
                let mark = crate::mark::bind_mark(doc, mark);
                map_marks(doc, *from, *to, |node, _| mark.remove_exact_from_set(node.marks()))
            }
            Self::AddNodeMark { pos, mark } => replace_node_markup(
                doc, *pos, "No node at mark step's position",
                |node| Ok(node.with_marks(Mark::add_to_set(node.marks(), crate::mark::bind_mark(doc, mark)))),
            ),
            Self::RemoveNodeMark { pos, mark } => replace_node_markup(
                doc, *pos, "No node at mark step's position",
                |node| Ok(node.with_marks(crate::mark::bind_mark(doc, mark).remove_exact_from_set(node.marks()))),
            ),
            Self::Attr { pos, attr, value } => replace_node_markup(
                doc, *pos, "No node at attribute step's position",
                |node| set_attr(node, attr, value),
            ),
            Self::DocAttr { attr, value } => match set_attr(doc, attr, value) {
                Ok(doc) => StepResult::ok(doc),
                Err(error) => StepResult::fail(error.to_string()),
            },
        }
    }

    pub fn get_map(&self) -> StepMap {
        match self {
            Self::Replace { from, to, slice, .. } => {
                StepMap::single(*from, to - from, slice.size())
            }
            Self::ReplaceAround { from, to, gap_from, gap_to, insert, slice, .. } => {
                StepMap::new(vec![
                    *from, gap_from - from, *insert,
                    *gap_to, to - gap_to, slice.size() - insert,
                ])
            }
            _ => StepMap::empty(),
        }
    }

    pub fn invert(&self, doc: &Node) -> Self {
        match self {
            Self::Replace { from, to, slice, .. } => Self::replace(
                *from,
                from + slice.size(),
                doc.slice(*from, *to, false).expect("applied step has a valid range"),
            ),
            Self::ReplaceAround { from, to, gap_from, gap_to, insert, slice, structure } => {
                let gap = gap_to - gap_from;
                let removed_gap = doc.slice(*from, *to, false)
                    .and_then(|slice| slice.remove_between(gap_from - from, gap_to - from))
                    .expect("applied replace-around step has a flat gap");
                Self::replace_around(
                    *from,
                    from + slice.size() + gap,
                    from + insert,
                    from + insert + gap,
                    removed_gap,
                    gap_from - from,
                    *structure,
                )
            }
            Self::AddMark { from, to, mark } => Self::remove_mark(*from, *to, mark.clone()),
            Self::RemoveMark { from, to, mark } => Self::add_mark(*from, *to, mark.clone()),
            Self::AddNodeMark { pos, mark } => {
                if let Some(node) = doc.node_at(*pos) {
                    let new_set = Mark::add_to_set(node.marks(), crate::mark::bind_mark(doc, mark));
                    if new_set.len() == node.marks().len() {
                        let restore = node.marks().iter().find(|old| !old.is_in_set(&new_set));
                        return Self::add_node_mark(*pos, restore.unwrap_or(mark).clone());
                    }
                }
                Self::remove_node_mark(*pos, mark.clone())
            }
            Self::RemoveNodeMark { pos, mark } => {
                let mark = crate::mark::bind_mark(doc, mark);
                if doc.node_at(*pos).is_some_and(|node| mark.is_in_set(node.marks())) {
                    Self::add_node_mark(*pos, mark.clone())
                } else {
                    self.clone()
                }
            }
            Self::Attr { pos, attr, .. } => {
                Self::attr(*pos, attr.clone(), old_attr(doc.node_at(*pos), attr))
            }
            Self::DocAttr { attr, .. } => Self::doc_attr(attr.clone(), old_attr(Some(doc), attr)),
        }
    }

    pub fn map(&self, mapping: &impl Mappable) -> Option<Self> {
        let mut mapped = self.clone();
        match &mut mapped {
            Self::Replace { from, to, structure, .. } => {
                let left = mapping.map_result(*from, 1);
                let right = mapping.map_result(*to, -1);
                if left.deleted_across() && right.deleted_across() {
                    return None;
                }
                *from = left.pos;
                *to = left.pos.max(right.pos);
                *structure = false;
            }
            Self::ReplaceAround { from, to, gap_from, gap_to, .. } => {
                let left = mapping.map_result(*from, 1);
                let right = mapping.map_result(*to, -1);
                *gap_from = if *from == *gap_from { left.pos } else { mapping.map(*gap_from, -1) };
                *gap_to = if *to == *gap_to { right.pos } else { mapping.map(*gap_to, 1) };
                if (left.deleted_across() && right.deleted_across())
                    || *gap_from < left.pos || *gap_to > right.pos
                {
                    return None;
                }
                *from = left.pos;
                *to = right.pos;
            }
            Self::AddMark { from, to, .. } | Self::RemoveMark { from, to, .. } => {
                let left = mapping.map_result(*from, 1);
                let right = mapping.map_result(*to, -1);
                if (left.deleted() && right.deleted()) || left.pos >= right.pos {
                    return None;
                }
                *from = left.pos;
                *to = right.pos;
            }
            Self::AddNodeMark { pos, .. } | Self::RemoveNodeMark { pos, .. } | Self::Attr { pos, .. } => {
                let result = mapping.map_result(*pos, 1);
                if result.deleted_after() {
                    return None;
                }
                *pos = result.pos;
            }
            Self::DocAttr { .. } => {}
        }
        Some(mapped)
    }

    pub fn merge(&self, other: &Self) -> Option<Self> {
        match (self, other) {
            (
                Self::Replace { from, to, slice, structure: false },
                Self::Replace { from: next_from, to: next_to, slice: next, structure: false },
            ) => {
                if from + slice.size() == *next_from && slice.open_end == 0 && next.open_start == 0 {
                    Some(Self::replace(*from, to + (next_to - next_from), append_slices(slice, next)))
                } else if next_to == from && slice.open_start == 0 && next.open_end == 0 {
                    Some(Self::replace(*next_from, *to, append_slices(next, slice)))
                } else {
                    None
                }
            }
            (Self::AddMark { from, to, mark }, Self::AddMark { from: next_from, to: next_to, mark: next })
            | (Self::RemoveMark { from, to, mark }, Self::RemoveMark { from: next_from, to: next_to, mark: next })
                if mark == next && from <= next_to && to >= next_from =>
            {
                let (start, end) = ((*from).min(*next_from), (*to).max(*next_to));
                if matches!(self, Self::AddMark { .. }) {
                    Some(Self::add_mark(start, end, mark.clone()))
                } else {
                    Some(Self::remove_mark(start, end, mark.clone()))
                }
            }
            _ => None,
        }
    }
}

fn valid_range(doc: &Node, from: usize, to: usize) -> bool {
    from <= to && to <= doc.content().size()
}

fn append_slices(left: &Slice, right: &Slice) -> Slice {
    if left.size() + right.size() == 0 {
        Slice::empty()
    } else {
        Slice::new(left.content.append(&right.content), left.open_start, right.open_end)
    }
}

fn content_between(doc: &Node, from: usize, to: usize) -> bool {
    let Ok(from) = doc.resolve(from) else {
        return true;
    };
    let mut remaining = to - from.pos;
    let mut depth = from.depth;
    while remaining > 0 && depth > 0 && from.index_after(depth) == from.node(depth).child_count() {
        depth -= 1;
        remaining -= 1;
    }
    let mut next = from.node(depth).content().children.get(from.index_after(depth));
    while remaining > 0 {
        match next {
            Some(node) if !node.is_leaf() => next = node.content().children.first(),
            _ => return true,
        }
        remaining -= 1;
    }
    false
}

fn replace_node_markup(
    doc: &Node,
    pos: usize,
    failure: &str,
    update: impl FnOnce(&Node) -> Result<Node, ModelError>,
) -> StepResult {
    let Some(node) = doc.node_at(pos) else {
        return StepResult::fail(failure);
    };
    if node.is_text() {
        return StepResult::fail("Cannot change text node markup with a node step");
    }
    let updated = match update(node) {
        Ok(updated) => updated.copy(None),
        Err(error) => return StepResult::fail(error.to_string()),
    };
    let slice = Slice::new(Fragment::from_nodes(vec![updated]), 0, usize::from(!node.is_leaf()));
    StepResult::from_replace(doc, pos, pos + 1, &slice)
}

fn set_attr(node: &Node, attr: &str, value: &Value) -> Result<Node, ModelError> {
    let mut attrs = node.attrs().and_then(Value::as_object).cloned().unwrap_or_default();
    attrs.insert(attr.to_owned(), value.clone());
    let attrs = Some(Value::Object(attrs));
    let Some(ty) = node.node_type() else {
        return Ok(node.with_attrs(attrs));
    };
    ty.try_create(attrs, Some(node.content().clone()), Some(node.marks().to_vec()))
}

fn old_attr(node: Option<&Node>, attr: &str) -> Value {
    node.and_then(Node::attrs)
        .and_then(|attrs| attrs.get(attr))
        .cloned()
        .unwrap_or(Value::Null)
}

fn map_marks(
    doc: &Node,
    from: usize,
    to: usize,
    update: impl Fn(&Node, &Node) -> Vec<Mark>,
) -> StepResult {
    if !valid_range(doc, from, to) {
        return StepResult::fail("Position out of bounds");
    }
    let range = doc.slice(from, to, false);
    let start = doc.resolve(from);
    let (old_slice, start) = match (range, start) {
        (Ok(slice), Ok(start)) => (slice, start),
        (Err(error), _) | (_, Err(error)) => return StepResult::fail(error.to_string()),
    };
    let parent = start.node(start.shared_depth(to));
    let content = map_fragment(&old_slice.content, parent, &update);
    let slice = Slice::new(content, old_slice.open_start, old_slice.open_end);
    StepResult::from_replace(doc, from, to, &slice)
}

fn map_fragment(
    fragment: &Fragment,
    parent: &Node,
    update: &impl Fn(&Node, &Node) -> Vec<Mark>,
) -> Fragment {
    let nodes = fragment.children.iter().map(|child| {
        let node = if child.content().size() == 0 {
            child.clone()
        } else {
            child.copy(Some(map_fragment(child.content(), child, update)))
        };
        if node.is_inline() {
            node.with_marks(update(&node, parent))
        } else {
            node
        }
    });
    Fragment::from_nodes(nodes.collect())
}
