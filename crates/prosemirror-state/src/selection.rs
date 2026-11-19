use std::any::Any;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use prosemirror_model::{Fragment, ModelError, Node, ResolvedPos, Slice};
use prosemirror_transform::Mappable;
use serde_json::Value;

use crate::custom_selection::{self, RegisteredBookmark};
use crate::gap_cursor::gap_cursor_or_near;
use crate::{BookmarkDecoder, CellSelection, CustomSelection, CustomSelectionBookmark, RegisteredSelection, SelectionDecoder};

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
    #[serde(rename = "gapcursor")]
    GapCursor { pos: usize },
    #[serde(rename = "cell")]
    Cell(CellSelection),
    /// Deserialize custom selections with [`Selection::from_json`] and the current document.
    #[serde(untagged, skip_deserializing)]
    Custom(RegisteredSelection),
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

    pub fn gap_cursor(pos: usize) -> Self {
        Self::GapCursor { pos }
    }

    pub fn cell(doc: &Node, anchor: usize, head: usize) -> Result<Self, ModelError> {
        CellSelection::create(doc, anchor, head).map(Self::Cell)
    }

    /// Register the document-aware decoder for a custom selection JSON ID.
    pub fn json_id(id: impl Into<String>, decode: SelectionDecoder) -> Result<(), ModelError> {
        custom_selection::register_selection(id.into(), decode)
    }

    /// Wrap a custom selection using the JSON ID of its decoder.
    pub fn custom(id: impl Into<String>, selection: impl CustomSelection) -> Result<Self, ModelError> {
        RegisteredSelection::new(id.into(), Arc::new(selection)).map(Self::Custom)
    }

    pub fn as_custom<T: CustomSelection>(&self) -> Option<&T> {
        let Self::Custom(selection) = self else { return None };
        let selection: &dyn Any = selection.implementation.as_ref();
        selection.downcast_ref::<T>()
    }

    pub fn from(&self) -> usize {
        match self {
            Self::Text { anchor, head } => (*anchor).min(*head),
            Self::Node { anchor } => *anchor,
            Self::All => 0,
            Self::GapCursor { pos } => *pos,
            Self::Cell(cell) => cell.from(),
            Self::Custom(selection) => selection.ranges[0].from.pos,
        }
    }

    pub fn to(&self, doc: &Node) -> usize {
        match self {
            Self::Text { anchor, head } => (*anchor).max(*head),
            Self::Node { anchor } => *anchor + self.selected_node(doc).map_or(0, |node| node.node_size()),
            Self::All => doc.content().size(),
            Self::GapCursor { pos } => *pos,
            Self::Cell(cell) => cell.to(doc),
            Self::Custom(selection) => selection.ranges[0].to.pos,
        }
    }

    pub fn anchor(&self) -> usize {
        match self {
            Self::Text { anchor, .. } => *anchor,
            Self::Node { anchor } => *anchor,
            Self::All => 0,
            Self::GapCursor { pos } => *pos,
            Self::Cell(cell) => cell.from(),
            Self::Custom(selection) => selection.implementation.anchor().pos,
        }
    }

    pub fn head(&self, doc: &Node) -> usize {
        match self {
            Self::Text { head, .. } => *head,
            Self::Node { .. } => self.to(doc),
            Self::All => doc.content().size(),
            Self::GapCursor { pos } => *pos,
            Self::Cell(cell) => cell.to(doc),
            Self::Custom(selection) => selection.implementation.head().pos,
        }
    }

    pub fn empty(&self) -> bool {
        match self {
            Self::Text { anchor, head } => anchor == head,
            Self::Node { .. } => false,
            Self::All => false,
            Self::GapCursor { .. } => true,
            Self::Cell(_) => false,
            Self::Custom(selection) => selection.ranges.iter().all(|range| range.from.pos == range.to.pos),
        }
    }

    pub fn empty_in(&self, doc: &Node) -> bool {
        match self {
            Self::Cell(cell) => cell.ranges(doc)
                .is_ok_and(|ranges| ranges.iter().all(|range| range.from.pos == range.to.pos)),
            Self::Custom(_) => self.empty(),
            _ => self.from() == self.to(doc),
        }
    }

    pub fn cursor_pos(&self) -> Option<usize> {
        match self {
            Self::Text { anchor, head } if anchor == head => Some(*head),
            _ => None,
        }
    }

    pub fn visible(&self) -> bool {
        match self {
            Self::Node { .. } | Self::GapCursor { .. } | Self::Cell(_) => false,
            Self::Custom(selection) => selection.implementation.visible(),
            _ => true,
        }
    }

    pub fn range(&self, doc: &Node) -> Result<SelectionRange, ModelError> {
        if let Self::Custom(selection) = self {
            return Ok(selection.ranges[0].clone());
        }
        Ok(SelectionRange {
            from: doc.resolve(self.from())?,
            to: doc.resolve(self.to(doc))?,
        })
    }

    pub fn ranges(&self, doc: &Node) -> Result<Vec<SelectionRange>, ModelError> {
        match self {
            Self::Cell(cell) => cell.ranges(doc),
            Self::Custom(selection) => Ok(selection.ranges.clone()),
            _ => self.range(doc).map(|range| vec![range]),
        }
    }

    pub fn selected_node(&self, doc: &Node) -> Option<Node> {
        match self {
            Self::Node { anchor } => doc.resolve(*anchor).ok()?.node_after(),
            _ => None,
        }
    }

    pub fn content(&self, doc: &Node) -> Result<Slice, ModelError> {
        if matches!(self, Self::GapCursor { .. }) {
            return Ok(Slice::empty());
        }
        if let Self::Cell(cell) = self {
            return cell.content(doc);
        }
        if let Self::Custom(selection) = self {
            return selection.implementation.content();
        }
        if let Some(node) = self.selected_node(doc) {
            return Ok(Slice::new(Fragment::from_nodes(vec![node]), 0, 0));
        }
        doc.slice(self.from(), self.to(doc), true)
    }

    pub fn from_json(doc: &Node, json: &Value) -> Result<Self, ModelError> {
        let id = json.get("type").and_then(Value::as_str).filter(|id| !id.is_empty())
            .ok_or_else(|| ModelError::Serialization("Invalid input for Selection.fromJSON".into()))?;
        if !custom_selection::is_builtin_id(id) {
            return RegisteredSelection::from_json(doc, id, json).map(Self::Custom);
        }
        let selection: Self = serde_json::from_value(json.clone())
            .map_err(|error| ModelError::Serialization(error.to_string()))?;
        if let Self::Cell(cell) = &selection {
            return Self::cell(doc, cell.anchor, cell.head);
        }
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

    pub fn map(&self, doc: &Node, mapping: &dyn Mappable) -> Self {
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
            Self::GapCursor { pos } => gap_cursor_or_near(&resolve_in(doc, mapping.map(*pos, 1))),
            Self::Cell(cell) => cell.map(doc, mapping),
            Self::Custom(selection) => selection.implementation.map(doc, mapping),
        }
    }

    pub fn bookmark(&self) -> SelectionBookmark {
        if let Self::Custom(selection) = self {
            return selection.implementation.bookmark();
        }
        SelectionBookmark(Bookmark::Selection(self.clone()))
    }
}

#[derive(Clone, Debug)]
pub struct SelectionRange {
    pub from: ResolvedPos,
    pub to: ResolvedPos,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SelectionBookmark(Bookmark);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
enum Bookmark {
    Selection(Selection),
    Custom(RegisteredBookmark),
}

impl SelectionBookmark {
    /// Register the decoder used to restore a custom bookmark from stored history.
    pub fn json_id(id: impl Into<String>, decode: BookmarkDecoder) -> Result<(), ModelError> {
        custom_selection::register_bookmark(id.into(), decode)
    }

    pub fn custom(id: impl Into<String>, bookmark: impl CustomSelectionBookmark) -> Self {
        Self(Bookmark::Custom(RegisteredBookmark::new(id.into(), Arc::new(bookmark))))
    }

    pub fn as_custom<T: CustomSelectionBookmark>(&self) -> Option<&T> {
        let Bookmark::Custom(bookmark) = &self.0 else { return None };
        let bookmark: &dyn Any = bookmark.implementation.as_ref();
        bookmark.downcast_ref::<T>()
    }

    pub fn map(&self, mapping: &dyn Mappable) -> Self {
        let selection = match &self.0 {
            Bookmark::Selection(selection) => selection,
            Bookmark::Custom(bookmark) => return bookmark.implementation.map(mapping),
        };
        Self(Bookmark::Selection(match *selection {
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
            Selection::GapCursor { pos } => Selection::gap_cursor(mapping.map(pos, 1)),
            Selection::Cell(ref cell) => Selection::Cell(cell.map_bookmark(mapping)),
            Selection::Custom(_) => unreachable!("custom selection bookmarks are separate"),
        }))
    }

    pub fn resolve(&self, doc: &Node) -> Selection {
        let selection = match &self.0 {
            Bookmark::Selection(selection) => selection,
            Bookmark::Custom(bookmark) => return bookmark.implementation.resolve(doc),
        };
        match *selection {
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
            Selection::GapCursor { pos } => gap_cursor_or_near(&resolve_in(doc, pos)),
            Selection::Cell(ref cell) => cell.bookmark().resolve(doc),
            Selection::Custom(_) => unreachable!("custom selection bookmarks are separate"),
        }
    }
}

pub(crate) fn resolve_in(doc: &Node, pos: usize) -> ResolvedPos {
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
