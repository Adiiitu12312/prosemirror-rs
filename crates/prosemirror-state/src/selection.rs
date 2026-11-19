use serde::{Deserialize, Serialize};
use prosemirror_model::{Fragment, ModelError, Node, ResolvedPos, Slice};
use prosemirror_transform::Mappable;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Selection {
    #[serde(rename = "text")]
    Text {
        anchor: usize,
        head: usize,
    },
    #[serde(rename = "node")]
    Node {
        anchor: usize,
    },
    #[serde(rename = "all")]
    All,
}

impl Selection {
    pub fn cursor(pos: usize) -> Self {
        Self::Text {
            anchor: pos,
            head: pos,
        }
    }

    pub fn text(anchor: usize, head: usize) -> Self {
        Self::Text { anchor, head }
    }

    pub fn node(pos: usize) -> Self {
        Self::Node { anchor: pos }
    }

    pub fn all() -> Self {
        Self::All
    }

    pub fn from(&self) -> usize {
        match self {
            Self::Text { anchor, head } => (*anchor).min(*head),
            Self::Node { anchor } => *anchor,
            Self::All => 0,
        }
    }

    pub fn to(&self, doc: &Node) -> usize {
        match self {
            Self::Text { anchor, head } => (*anchor).max(*head),
            Self::Node { anchor } => *anchor + self.selected_node(doc).map_or(0, |node| node.node_size()),
            Self::All => doc.content().size(),
        }
    }

    pub fn anchor(&self) -> usize {
        match self {
            Self::Text { anchor, .. } => *anchor,
            Self::Node { anchor } => *anchor,
            Self::All => 0,
        }
    }

    pub fn head(&self, doc: &Node) -> usize {
        match self {
            Self::Text { head, .. } => *head,
            Self::Node { .. } => self.to(doc),
            Self::All => doc.content().size(),
        }
    }

    pub fn empty(&self) -> bool {
        match self {
            Self::Text { anchor, head } => anchor == head,
            Self::Node { .. } => false,
            Self::All => false,
        }
    }

    pub fn empty_in(&self, doc: &Node) -> bool {
        self.from() == self.to(doc)
    }

    pub fn cursor_pos(&self) -> Option<usize> {
        match self {
            Self::Text { anchor, head } if anchor == head => Some(*head),
            _ => None,
        }
    }

    pub fn visible(&self) -> bool {
        !matches!(self, Self::Node { .. })
    }

    pub fn selected_node(&self, doc: &Node) -> Option<Node> {
        match self {
            Self::Node { anchor } => doc.resolve(*anchor).ok()?.node_after(),
            _ => None,
        }
    }

    pub fn content(&self, doc: &Node) -> Result<Slice, ModelError> {
        if let Some(node) = self.selected_node(doc) {
            return Ok(Slice::new(Fragment::from_nodes(vec![node]), 0, 0));
        }
        doc.slice(self.from(), self.to(doc), true)
    }

    pub fn from_json(doc: &Node, json: &Value) -> Result<Self, ModelError> {
        let selection: Self = serde_json::from_value(json.clone())
            .map_err(|error| ModelError::Serialization(error.to_string()))?;
        doc.resolve(selection.anchor())?;
        doc.resolve(selection.head(doc))?;
        if matches!(selection, Self::Node { .. }) && selection.selected_node(doc).is_none() {
            return Err(ModelError::Serialization("Node selection has no node".into()));
        }
        Ok(selection)
    }

    pub fn find_from(pos: &ResolvedPos, dir: i32, text_only: bool) -> Option<Self> {
        let direction = if dir < 0 { -1 } else { 1 };
        let inner = find_selection_in(pos.parent(), pos.pos, pos.index(pos.depth), direction, text_only);
        if inner.is_some() {
            return inner;
        }
        for depth in (0..pos.depth).rev() {
            let (start, index) = if direction < 0 {
                (pos.before(depth + 1), pos.index(depth))
            } else {
                (pos.after(depth + 1), pos.index(depth) + 1)
            };
            let found = find_selection_in(pos.node(depth), start, index, direction, text_only);
            if found.is_some() {
                return found;
            }
        }
        None
    }

    pub fn near(pos: &ResolvedPos, bias: i32) -> Self {
        let bias = if bias < 0 { -1 } else { 1 };
        Self::find_from(pos, bias, false)
            .or_else(|| Self::find_from(pos, -bias, false))
            .unwrap_or(Self::All)
    }

    pub fn at_start(doc: &Node) -> Self {
        find_selection_in(doc, 0, 0, 1, false).unwrap_or(Self::All)
    }

    pub fn at_end(doc: &Node) -> Self {
        find_selection_in(doc, doc.content().size(), doc.child_count(), -1, false)
            .unwrap_or(Self::All)
    }

    pub fn between(anchor: &ResolvedPos, head: &ResolvedPos, bias: i32) -> Self {
        let forward = anchor.pos < head.pos;
        let equal = anchor.pos == head.pos;
        let bias = if equal && bias != 0 {
            bias.signum()
        } else if forward {
            -1
        } else {
            1
        };
        let head_pos = if head.parent().inline_content() {
            head.pos
        } else {
            let found = Self::find_from(head, bias, true)
                .or_else(|| Self::find_from(head, -bias, true));
            let Some(found) = found else {
                return Self::near(head, bias);
            };
            found.anchor()
        };
        let anchor_pos = if anchor.parent().inline_content() {
            anchor.pos
        } else if equal {
            head_pos
        } else {
            Self::find_from(anchor, -bias, true)
                .or_else(|| Self::find_from(anchor, bias, true))
                .map(|selection| selection.anchor())
                .filter(|pos| (*pos < head_pos) == forward)
                .unwrap_or(head_pos)
        };
        Self::text(anchor_pos, head_pos)
    }

    pub fn map(&self, doc: &Node, mapping: &impl Mappable) -> Self {
        match self {
            Self::Text { anchor, head } => {
                let head = resolve_in(doc, mapping.map(*head, 1));
                if !head.parent().inline_content() {
                    return Self::near(&head, 1);
                }
                let anchor = resolve_in(doc, mapping.map(*anchor, 1));
                Self::text(
                    if anchor.parent().inline_content() { anchor.pos } else { head.pos },
                    head.pos,
                )
            }
            Self::Node { anchor } => {
                let mapped = mapping.map_result(*anchor, 1);
                let pos = resolve_in(doc, mapped.pos);
                if mapped.deleted() || pos.node_after().is_none() {
                    Self::near(&pos, 1)
                } else {
                    Self::node(pos.pos)
                }
            }
            Self::All => Self::All,
        }
    }

    pub fn bookmark(&self) -> SelectionBookmark {
        SelectionBookmark(self.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SelectionBookmark(Selection);

impl SelectionBookmark {
    pub fn map(&self, mapping: &impl Mappable) -> Self {
        Self(match self.0 {
            Selection::Text { anchor, head } => {
                Selection::text(mapping.map(anchor, 1), mapping.map(head, 1))
            }
            Selection::Node { anchor } => {
                let mapped = mapping.map_result(anchor, 1);
                if mapped.deleted() {
                    Selection::cursor(mapped.pos)
                } else {
                    Selection::node(mapped.pos)
                }
            }
            Selection::All => Selection::All,
        })
    }

    pub fn resolve(&self, doc: &Node) -> Selection {
        match self.0 {
            Selection::Text { anchor, head } => {
                Selection::between(&resolve_in(doc, anchor), &resolve_in(doc, head), 1)
            }
            Selection::Node { anchor } => {
                let pos = resolve_in(doc, anchor);
                if pos.node_after().is_some_and(|node| node.is_selectable()) {
                    Selection::node(pos.pos)
                } else {
                    Selection::near(&pos, 1)
                }
            }
            Selection::All => Selection::All,
        }
    }
}

fn resolve_in(doc: &Node, pos: usize) -> ResolvedPos {
    doc.resolve(pos.min(doc.content().size())).expect("position in document")
}

fn find_selection_in(
    node: &Node,
    mut pos: usize,
    mut index: usize,
    dir: i32,
    text_only: bool,
) -> Option<Selection> {
    if node.inline_content() {
        return Some(Selection::cursor(pos));
    }
    while if dir < 0 { index > 0 } else { index < node.child_count() } {
        let child_index = if dir < 0 { index - 1 } else { index };
        let child = node.child(child_index);
        if !child.is_atom() {
            let (inside, offset) = if dir < 0 {
                (pos - 1, child.child_count())
            } else {
                (pos + 1, 0)
            };
            let found = find_selection_in(child, inside, offset, dir, text_only);
            if found.is_some() {
                return found;
            }
        } else if !text_only && child.is_selectable() {
            return Some(Selection::node(if dir < 0 { pos - child.node_size() } else { pos }));
        }
        if dir < 0 {
            index -= 1;
            pos -= child.node_size();
        } else {
            index += 1;
            pos += child.node_size();
        }
    }
    None
}
