//! Native rendering and platform services for the editor.
//!
//! All document positions are ProseMirror UTF-16 positions. Geometry is in a
//! consistent host coordinate space, with the origin and scale chosen by the
//! host. Display data owns its contents and can outlive the editor update that
//! produced it.

use std::ops::Range;

use prosemirror_model::{Mark, Node};
use prosemirror_state::{EditorState, Selection};
use prosemirror_view::{
    AccessibilityNode, AccessibilityRole, AccessibilityTreeUpdate, Decoration, DecorationGroup, DecorationType,
};

use crate::Attributes;

/// Version of the native host and display-list contract.
pub const HOST_INTERFACE_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Rect {
    pub fn width(self) -> f64 {
        self.right - self.left
    }

    pub fn height(self) -> f64 {
        self.bottom - self.top
    }

    pub fn center(self) -> Point {
        Point {
            x: self.left + self.width() / 2.0,
            y: self.top + self.height() / 2.0,
        }
    }

    pub fn contains(self, point: Point) -> bool {
        (self.left..=self.right).contains(&point.x) && (self.top..=self.bottom).contains(&point.y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PositionAtCoords {
    pub pos: usize,
    /// Position immediately before the innermost node at the point.
    pub inside: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineBox {
    pub from: usize,
    pub to: usize,
    pub rect: Rect,
}

/// The host's measured geometry for the current editor frame.
pub trait Layout {
    /// At a boundary, a negative side uses the preceding content and a
    /// positive side uses the following content.
    fn coords_at_pos(&self, pos: usize, side: i32) -> Option<Rect>;

    fn pos_at_coords(&self, point: Point) -> Option<PositionAtCoords>;

    /// Returns full visual line boxes overlapping the requested document range.
    fn line_boxes(&self, _from: usize, _to: usize) -> Vec<LineBox> {
        Vec::new()
    }

    /// Tests the logical edge of the measured visual line at a position.
    /// At a wrap boundary, a negative side checks the start of the following
    /// line and a positive side checks the end of the preceding line.
    fn end_of_line(&self, pos: usize, side: i32) -> bool {
        let caret_side = if side < 0 { 1 } else { -1 };
        let Some(caret) = self.coords_at_pos(pos, caret_side) else { return false };
        self.line_boxes(pos, pos).into_iter().any(|line| {
            caret.top < line.rect.bottom && caret.bottom > line.rect.top
                && pos == if side < 0 { line.from } else { line.to }
        })
    }

    /// Measures the block whose opening boundary is at `pos`.
    fn block_bounds(&self, _pos: usize) -> Option<Rect> {
        None
    }

    /// Measures the preedit caret using its UTF-16 selection in the overlay.
    fn composition_caret(&self, _composition: &CompositionDisplay) -> Option<Rect> {
        None
    }

    /// Override this method to return disjoint highlights for bidirectional
    /// text. The default clips visual lines at their measured caret positions.
    fn selection_rects(&self, from: usize, to: usize) -> Vec<Rect> {
        if from >= to {
            return Vec::new();
        }
        self.line_boxes(from, to).into_iter().filter_map(|line| {
            if from >= line.to || to <= line.from {
                return None;
            }
            let start = self.coords_at_pos(from.max(line.from), 1)
                .map_or(line.rect.left, |caret| caret.left);
            let end = self.coords_at_pos(to.min(line.to), -1)
                .map_or(line.rect.right, |caret| caret.right);
            let clip = |x: f64| x.max(line.rect.left).min(line.rect.right);
            Some(Rect {
                left: clip(start.min(end)),
                right: clip(start.max(end)),
                ..line.rect
            })
        }).collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClipboardContent {
    pub plain_text: String,
    /// An opaque, editor-specific representation for rich round trips.
    pub internal: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorIcon {
    #[default]
    Default,
    Text,
    Pointer,
    Grab,
    Grabbing,
    Move,
    ColumnResize,
    RowResize,
    NotAllowed,
}

/// Platform adapter for layout, painting, and native services. The editor
/// supplies content, decorations, and selection state. The host supplies text
/// shaping, viewport coordinates, and platform resources.
pub trait Host: Layout {
    /// Measure the new frame before the editor asks for its selection geometry.
    /// Called even for selection, decoration, and preedit-only changes.
    fn update_layout(&mut self, _frame: &DisplayList, _changed: &BlockUpdate) {}

    /// Paints the measured frame. A host that caches block drawing should also
    /// account for decoration changes, which are separate from model changes.
    fn draw(&mut self, frame: &DisplayList, changed: &BlockUpdate);

    fn read_clipboard(&mut self) -> Option<ClipboardContent> {
        None
    }

    fn write_clipboard(&mut self, _content: &ClipboardContent) {}

    fn set_ime_candidate_rect(&mut self, _rect: Option<Rect>) {}

    fn set_cursor(&mut self, _cursor: CursorIcon) {}

    fn request_focus(&mut self) {}

    fn viewport(&self) -> Option<Rect> {
        None
    }

    fn scroll_by(&mut self, _delta: Point) {}

    /// Scrolls the minimum distance to reveal a rectangle in the viewport.
    fn scroll_into_view(&mut self, rect: Rect) {
        if let Some(viewport) = self.viewport() {
            let delta = Point {
                x: scroll_delta(rect.left, rect.right, viewport.left, viewport.right),
                y: scroll_delta(rect.top, rect.bottom, viewport.top, viewport.bottom),
            };
            if delta != Point::default() {
                self.scroll_by(delta);
            }
        }
    }

    fn update_accessibility(&mut self, _update: &AccessibilityTreeUpdate) {}
}

fn scroll_delta(from: f64, to: f64, view_from: f64, view_to: f64) -> f64 {
    if from < view_from {
        from - view_from
    } else if to > view_to {
        (to - view_to).min(from - view_from)
    } else {
        0.0
    }
}

/// Contiguous top-level child index ranges to remove and draw. Unchanged
/// leading and trailing nodes are recognized by persistent node identity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockUpdate {
    pub previous: Range<usize>,
    pub current: Range<usize>,
}

impl BlockUpdate {
    pub fn between(previous: Option<&Node>, current: &Node) -> Self {
        let current = &current.content().children;
        let Some(previous) = previous else {
            return Self { previous: 0..0, current: 0..current.len() };
        };
        let previous = &previous.content().children;
        let prefix = previous.iter().zip(current).take_while(|(left, right)| left.ptr_eq(right)).count();
        if prefix == previous.len() && prefix == current.len() {
            return Self::default();
        }
        let suffix = previous[prefix..].iter().rev().zip(current[prefix..].iter().rev())
            .take_while(|(left, right)| left.ptr_eq(right)).count();
        Self {
            previous: prefix..previous.len() - suffix,
            current: prefix..current.len() - suffix,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayList {
    pub document_size: usize,
    pub attributes: Attributes,
    pub editable: bool,
    /// One entry per top-level document child, in model order.
    pub blocks: Vec<DisplayNode>,
    /// Zero-width widgets between top-level children.
    pub root_widgets: Vec<DisplayWidget>,
    pub selection: SelectionDisplay,
    /// Ephemeral text projected over the indicated document range.
    pub composition: Option<CompositionDisplay>,
    pub composition_caret: Option<Rect>,
    /// Whether this editor owns native keyboard focus.
    pub focused: bool,
    pub drag: Option<DragDisplay>,
    pub touch_handles: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayNode {
    pub node: Node,
    /// Boundaries of the native node view, including the model node tokens.
    pub from: usize,
    pub to: usize,
    pub content_from: usize,
    pub content_to: usize,
    pub bounds: Option<Rect>,
    pub decorations: Vec<Decoration>,
    pub children: Vec<DisplayItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DisplayItem {
    Node(DisplayNode),
    Text(DisplayTextRun),
    Widget(DisplayWidget),
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayTextRun {
    pub from: usize,
    pub to: usize,
    /// Original UTF-16 units, including isolated surrogates if a range splits a pair.
    pub text: Vec<u16>,
    /// Ordered from the outermost mark to the innermost mark.
    pub marks: Vec<Mark>,
    pub decorations: Vec<Decoration>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayWidget {
    pub pos: usize,
    pub id: u64,
    pub side: i32,
    pub marks: Vec<Mark>,
    pub decoration: Decoration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionDisplay {
    pub from: usize,
    pub to: usize,
    pub text: String,
    /// UTF-16 offsets within `text`.
    pub selection: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragDisplay {
    pub source: usize,
    pub over: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionKind {
    Text,
    Node,
    All,
    Gap,
    Cell,
    Custom,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectionDisplay {
    pub kind: SelectionKind,
    pub anchor: usize,
    pub head: usize,
    pub ranges: Vec<Range<usize>>,
    /// Starting positions of model nodes given native selection styling.
    pub selected_nodes: Vec<usize>,
    pub highlights: Vec<Rect>,
    pub carets: Vec<SelectionCaret>,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionCaret {
    pub pos: usize,
    pub rect: Rect,
}

impl DisplayList {
    /// Projects the document and measures it with an already up-to-date layout.
    pub fn build(
        doc: &Node, selection: &Selection, decorations: &DecorationGroup, layout: &(impl Layout + ?Sized),
    ) -> Self {
        let mut frame = Self::unmeasured(doc, selection, decorations);
        frame.measure(layout);
        frame
    }

    /// Projects model data for the host's next layout pass.
    pub fn unmeasured(doc: &Node, selection: &Selection, decorations: &DecorationGroup) -> Self {
        let local = decorations.locals(doc);
        let mut blocks = Vec::with_capacity(doc.child_count());
        let mut root_widgets = Vec::new();
        let mut preceding = &[][..];
        let mut offset = 0;
        for child in &doc.content().children {
            root_widgets.extend(widgets(doc, &local, offset, 0, preceding, child.marks()));
            blocks.push(display_node(doc, child, 0, offset, &local, decorations));
            preceding = child.marks();
            offset += child.node_size();
        }
        let document_size = doc.content().size();
        root_widgets.extend(widgets(doc, &local, document_size, 0, preceding, &[]));
        Self {
            document_size, attributes: Attributes::new(), editable: true,
            blocks, root_widgets, selection: SelectionDisplay::new(doc, selection),
            composition: None, composition_caret: None, focused: false, drag: None, touch_handles: false,
        }
    }

    pub fn measure(&mut self, layout: &(impl Layout + ?Sized)) {
        for node in &mut self.blocks {
            node.measure(layout);
        }
        self.selection.measure(layout);
        self.composition_caret = self.composition.as_ref().and_then(|composition| {
            layout.composition_caret(composition).or_else(|| layout.coords_at_pos(composition.from, 1))
        });
    }

    /// Returns the native node view starting at a document position.
    pub fn node_at(&self, pos: usize) -> Option<&DisplayNode> {
        self.blocks.iter().find_map(|node| node.node_at(pos))
    }
}

impl DisplayNode {
    fn measure(&mut self, layout: &(impl Layout + ?Sized)) {
        self.bounds = self.node.is_block().then(|| layout.block_bounds(self.from)).flatten();
        for child in &mut self.children {
            if let DisplayItem::Node(node) = child {
                node.measure(layout);
            }
        }
    }

    fn node_at(&self, pos: usize) -> Option<&Self> {
        if self.from == pos {
            return Some(self);
        }
        if pos < self.content_from || pos >= self.content_to {
            return None;
        }
        self.children.iter().find_map(|child| match child {
            DisplayItem::Node(node) => node.node_at(pos),
            _ => None,
        })
    }
}

impl SelectionDisplay {
    fn new(doc: &Node, selection: &Selection) -> Self {
        let mut selected_nodes = Vec::new();
        let kind = match selection {
            Selection::Text { .. } => SelectionKind::Text,
            Selection::Node { anchor } => {
                selected_nodes.push(*anchor);
                SelectionKind::Node
            }
            Selection::All => SelectionKind::All,
            Selection::GapCursor { .. } => SelectionKind::Gap,
            Selection::Cell(cell) => {
                let _ = cell.for_each_cell(doc, |_, pos| selected_nodes.push(pos));
                SelectionKind::Cell
            }
            Selection::Custom(_) => SelectionKind::Custom,
        };
        let ranges = selection.ranges(doc).unwrap_or_default()
            .into_iter().map(|range| range.from.pos..range.to.pos).collect();
        Self {
            kind, ranges, selected_nodes, anchor: selection.anchor(), head: selection.head(doc),
            highlights: Vec::new(), carets: Vec::new(), visible: selection.visible(),
        }
    }

    fn measure(&mut self, layout: &(impl Layout + ?Sized)) {
        self.highlights.clear();
        self.carets.clear();
        if !self.visible && self.kind == SelectionKind::Custom {
            return;
        }
        for range in &self.ranges {
            let node_pos = match self.kind {
                SelectionKind::Node => Some(range.start),
                SelectionKind::Cell => range.start.checked_sub(1),
                _ => None,
            };
            let node_bounds = node_pos.filter(|pos| self.selected_nodes.contains(pos))
                .and_then(|pos| layout.block_bounds(pos));
            if let Some(rect) = node_bounds {
                self.highlights.push(rect);
            } else if range.is_empty() {
                if let Some(rect) = layout.coords_at_pos(range.start, 1) {
                    self.carets.push(SelectionCaret { pos: range.start, rect });
                }
            } else {
                self.highlights.extend(layout.selection_rects(range.start, range.end));
            }
        }
    }
}

fn display_node(
    parent: &Node, node: &Node, base: usize, offset: usize, local: &[Decoration],
    group: &DecorationGroup,
) -> DisplayNode {
    let from = base + offset;
    let to = from + node.node_size();
    let (content_from, content_to) = if node.is_text() {
        (from, to)
    } else if node.is_leaf() {
        (from, from)
    } else {
        (from + 1, to - 1)
    };
    let children = if node.is_text() {
        text_items(parent, node, base, offset, local)
    } else if node.is_leaf() {
        Vec::new()
    } else {
        display_children(node, content_from, &group.for_child(offset, node))
    };
    DisplayNode {
        node: node.clone(), from, to, content_from, content_to, bounds: None, children,
        decorations: covering(local, base, offset, offset + node.node_size(), !node.is_text() && node.is_inline() && node.is_leaf()),
    }
}

fn display_children(node: &Node, base: usize, group: &DecorationGroup) -> Vec<DisplayItem> {
    let local = group.locals(node);
    let mut children = Vec::new();
    let mut preceding = &[][..];
    let mut offset = 0;
    for child in &node.content().children {
        children.extend(widgets(node, &local, offset, base, preceding, child.marks()).into_iter().map(DisplayItem::Widget));
        if child.is_text() {
            children.extend(text_items(node, child, base, offset, &local));
        } else {
            children.push(DisplayItem::Node(display_node(node, child, base, offset, &local, group)));
        }
        preceding = child.marks();
        offset += child.node_size();
    }
    children.extend(widgets(node, &local, node.content().size(), base, preceding, &[]).into_iter().map(DisplayItem::Widget));
    children
}

fn text_items(parent: &Node, node: &Node, base: usize, offset: usize, local: &[Decoration]) -> Vec<DisplayItem> {
    let Some(text) = node.text_utf16() else { return Vec::new() };
    let end = offset + text.len();
    let mut cuts = vec![offset, end];
    cuts.extend(local.iter().flat_map(|decoration| [decoration.from, decoration.to])
        .filter(|&pos| offset < pos && pos < end));
    cuts.sort_unstable();
    cuts.dedup();
    let mut children = Vec::new();
    for bounds in cuts.windows(2) {
        let (from, to) = (bounds[0], bounds[1]);
        if from > offset {
            children.extend(widgets(parent, local, from, base, node.marks(), node.marks())
                .into_iter().map(DisplayItem::Widget));
        }
        children.push(DisplayItem::Text(DisplayTextRun {
            from: base + from, to: base + to, text: text[from - offset..to - offset].to_vec(),
            marks: node.marks().to_vec(),
            decorations: covering(local, base, from, to, true),
        }));
    }
    children
}

fn widgets(
    parent: &Node, local: &[Decoration], offset: usize, base: usize, preceding: &[Mark], following: &[Mark],
) -> Vec<DisplayWidget> {
    let mut widgets: Vec<_> = local.iter().filter_map(|decoration| {
        let DecorationType::Widget { id, side } = decoration.kind else { return None };
        if decoration.from != offset {
            return None;
        }
        let inherited = if side < 0 { preceding } else { following };
        let marks = decoration.spec()["marks"].as_array().map_or_else(
            || inherited.to_vec(),
            |marks| marks.iter().filter_map(|mark| parent.schema()?.mark_from_json(mark).ok()).collect(),
        );
        Some(DisplayWidget { pos: base + offset, id, side, marks, decoration: absolute(decoration, base) })
    }).collect();
    widgets.sort_by_key(|widget| widget.side);
    widgets
}

fn covering(local: &[Decoration], base: usize, from: usize, to: usize, inline: bool) -> Vec<Decoration> {
    local.iter().filter(|decoration| match decoration.kind {
        DecorationType::Inline { .. } => inline && decoration.from <= from && decoration.to >= to,
        DecorationType::Node { .. } => decoration.from == from && decoration.to == to,
        DecorationType::Widget { .. } => false,
    }).map(|decoration| absolute(decoration, base)).collect()
}

fn absolute(decoration: &Decoration, base: usize) -> Decoration {
    let mut decoration = decoration.clone();
    decoration.from += base;
    decoration.to += base;
    decoration
}

/// Projects the model hierarchy and selection focus for native accessibility.
pub fn accessibility_tree(state: &EditorState) -> AccessibilityTreeUpdate {
    let target = match &state.selection {
        Selection::All => AccessibilityFocus::Document,
        Selection::Node { anchor } => AccessibilityFocus::Node(*anchor),
        Selection::Cell(cell) => AccessibilityFocus::Node(cell.head),
        Selection::GapCursor { pos } => AccessibilityFocus::Node(*pos),
        selection => AccessibilityFocus::Position(selection.head(&state.doc)),
    };
    let mut builder = AccessibilityBuilder { nodes: Vec::new(), target, focused: (0, 1) };
    let root_id = builder.add(&state.doc, 0, 0);
    AccessibilityTreeUpdate { nodes: builder.nodes, root_id, focus_id: builder.focused.1 }
}

#[derive(Clone, Copy)]
enum AccessibilityFocus {
    Document,
    Node(usize),
    Position(usize),
}

struct AccessibilityBuilder {
    nodes: Vec<AccessibilityNode>,
    target: AccessibilityFocus,
    focused: (usize, u64),
}

impl AccessibilityBuilder {
    fn add(&mut self, node: &Node, from: usize, depth: usize) -> u64 {
        let is_root = depth == 0;
        let to = from + if is_root { node.content().size() } else { node.node_size() };
        let border = usize::from(!is_root && !node.is_leaf());
        let role = match node.type_name() {
            _ if is_root => AccessibilityRole::Document,
            "heading" => AccessibilityRole::Heading,
            "blockquote" => AccessibilityRole::Blockquote,
            _ if node.is_inline() => AccessibilityRole::InlineTextBox,
            _ => AccessibilityRole::Paragraph,
        };
        let label = if is_root {
            Some("ProseMirror Editor".to_owned())
        } else if !node.is_text() && node.is_leaf() {
            node.attrs().and_then(|attrs| {
                attrs["alt"].as_str().or_else(|| attrs["title"].as_str())
            }).map(str::to_owned)
        } else if node.is_textblock() {
            Some(node.text_content())
        } else {
            None
        };
        let index = self.nodes.len();
        let id = index as u64 + 1;
        self.nodes.push(AccessibilityNode {
            id, role, label, value: node.text().map(str::to_owned), children: Vec::new(),
        });
        let contains_focus = match self.target {
            AccessibilityFocus::Document => is_root,
            AccessibilityFocus::Node(pos) => from == pos,
            AccessibilityFocus::Position(pos) => (from + border..=to - border).contains(&pos),
        };
        if contains_focus && depth >= self.focused.0 {
            self.focused = (depth, id);
        }

        let mut children = Vec::with_capacity(node.child_count());
        let mut offset = from + border;
        for child in &node.content().children {
            children.push(self.add(child, offset, depth + 1));
            offset += child.node_size();
        }
        self.nodes[index].children = children;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prosemirror_model::Fragment;
    use prosemirror_state::EditorStateConfig;
    use prosemirror_test_builder::{blockquote, doc, h1, p, test_schema};
    use prosemirror_view::DecorationSet;
    use serde_json::{json, Value};

    struct TestLayout;

    impl Layout for TestLayout {
        fn coords_at_pos(&self, pos: usize, side: i32) -> Option<Rect> {
            if !(1..=7).contains(&pos) {
                return None;
            }
            let (start, y) = if pos < 4 || pos == 4 && side < 0 {
                (1, 0.0)
            } else {
                (4, 20.0)
            };
            let x = ((pos - start) as f64) * 10.0;
            Some(rect(x, y, x, y + 20.0))
        }

        fn pos_at_coords(&self, point: Point) -> Option<PositionAtCoords> {
            let line = self.line_boxes(1, 7).into_iter().find(|line| line.rect.contains(point))?;
            let index = (point.x / 10.0).round() as usize;
            Some(PositionAtCoords { pos: (line.from + index).min(line.to), inside: Some(0) })
        }

        fn line_boxes(&self, _from: usize, _to: usize) -> Vec<LineBox> {
            vec![
                LineBox { from: 1, to: 4, rect: rect(0.0, 0.0, 30.0, 20.0) },
                LineBox { from: 4, to: 7, rect: rect(0.0, 20.0, 30.0, 40.0) },
            ]
        }

        fn block_bounds(&self, pos: usize) -> Option<Rect> {
            (pos == 0).then(|| rect(0.0, 0.0, 30.0, 40.0))
        }

        fn composition_caret(&self, _composition: &CompositionDisplay) -> Option<Rect> {
            Some(rect(7.0, 4.0, 7.0, 18.0))
        }
    }

    fn rect(left: f64, top: f64, right: f64, bottom: f64) -> Rect {
        Rect { left, top, right, bottom }
    }

    #[test]
    fn finds_the_range_between_persistent_leading_and_trailing_blocks() {
        let before = doc!(p!("first"), p!("middle"), p!("last")).node;
        let middle = doc!(p!("replacement"), p!("inserted")).node;
        let after = before.copy(Some(Fragment::from_nodes(vec![
            before.child(0).clone(), middle.child(0).clone(), middle.child(1).clone(), before.child(2).clone(),
        ])));
        let range = BlockUpdate::between(Some(&before), &after);
        assert_eq!(range, BlockUpdate { previous: 1..2, current: 1..3 });
        assert_eq!(BlockUpdate::between(Some(&after), &before), BlockUpdate { previous: 1..3, current: 1..2 });
        assert_eq!(BlockUpdate::between(Some(&before), &before.clone()), BlockUpdate::default());
        assert_eq!(BlockUpdate::between(None, &after), BlockUpdate { previous: 0..0, current: 0..4 });

        let equal_middle = doc!(p!("middle")).node;
        let structurally_equal = before.copy(Some(Fragment::from_nodes(vec![
            before.child(0).clone(), equal_middle.child(0).clone(), before.child(2).clone(),
        ])));
        assert_eq!(before, structurally_equal);
        assert_eq!(BlockUpdate::between(Some(&before), &structurally_equal),
            BlockUpdate { previous: 1..2, current: 1..2 });
    }

    #[test]
    fn exposes_nested_nodes_decorations_and_unpaired_utf16_run_boundaries() {
        let schema = test_schema();
        let em = schema.mark_type("em").unwrap().create(None);
        let strong = schema.mark_type("strong").unwrap().create(None);
        let image = schema.node("image", Some(json!({"src":"image.png"})), None, None).unwrap();
        let document = doc!(blockquote!(p!(
            schema.text("A😀", Some(vec![em, strong.clone()])),
            schema.text("B", Some(vec![strong])),
            image,
        ))).node;
        let set = DecorationSet::create(&document, vec![
            Decoration::node(0, 9, [("frame".into(), "outer".into())].into(), Value::Null),
            Decoration::node(1, 8, [("frame".into(), "inner".into())].into(), Value::Null),
            Decoration::inline(3, 6, [("emphasis".into(), "background".into())].into(), Value::Null),
            Decoration::widget(0, 9, Value::Null),
            Decoration::widget(4, 1, Value::Null),
            Decoration::widget(5, 3, Value::Null),
            Decoration::widget(5, 2, json!({"side":-1})),
        ]);
        let decorations = DecorationGroup { members: vec![set] };
        let frame = DisplayList::unmeasured(&document, &Selection::cursor(2), &decorations);
        assert_eq!(frame.document_size, 9);
        assert_eq!(frame.root_widgets[0].id, 9);
        let quote = frame.node_at(0).unwrap();
        let para = frame.node_at(1).unwrap();
        let image = frame.node_at(6).unwrap();
        assert_eq!((quote.from, quote.to, quote.content_from, quote.content_to), (0, 9, 1, 8));
        assert_eq!((para.from, para.to, para.content_from, para.content_to), (1, 8, 2, 7));
        assert_eq!(image.node.type_name(), "image");
        assert_eq!(quote.decorations[0].from..quote.decorations[0].to, 0..9);
        assert_eq!(para.decorations[0].from..para.decorations[0].to, 1..8);
        let runs: Vec<_> = para.children.iter().filter_map(|item| match item {
            DisplayItem::Text(run) => Some(run),
            _ => None,
        }).collect();
        let units = |index: usize| runs[index].text.as_slice();
        assert_eq!(units(0), &[b'A' as u16]);
        assert_eq!(units(1), &[0xd83d]);
        assert_eq!(units(2), &[0xde00]);
        assert_eq!(units(3), &[b'B' as u16]);
        assert_eq!(runs[1].from..runs[1].to, 3..4);
        assert_eq!(runs[2].from..runs[2].to, 4..5);
        assert!(runs[0].decorations.is_empty());
        assert_eq!(runs[1].decorations[0].from..runs[1].decorations[0].to, 3..4);
        let marks = |index: usize| runs[index].marks.iter().map(|mark| mark.type_name.as_str()).collect::<Vec<_>>();
        assert_eq!(marks(1), ["em", "strong"]);
        assert_eq!(marks(3), ["strong"]);
        let widgets: Vec<_> = para.children.iter().filter_map(|item| match item {
            DisplayItem::Widget(widget) => Some(widget),
            _ => None,
        }).collect();
        assert_eq!(widgets.iter().map(|widget| (widget.id, widget.pos)).collect::<Vec<_>>(), [(1, 4), (2, 5), (3, 5)]);
        assert_eq!(widgets[1].marks[0].type_name, "em");
        assert_eq!(widgets[2].marks[0].type_name, "strong");
    }

    #[test]
    fn clips_range_geometry_along_visual_lines() {
        let document = doc!(p!("abcdef")).node;
        let frame = DisplayList::build(&document, &Selection::text(5, 2), &DecorationGroup::default(), &TestLayout);
        assert_eq!((frame.selection.anchor, frame.selection.head), (5, 2));
        assert_eq!(frame.selection.ranges.as_slice(), std::slice::from_ref(&(2..5)));
        assert_eq!(frame.selection.highlights, [rect(10.0, 0.0, 30.0, 20.0), rect(0.0, 20.0, 10.0, 40.0)]);
        assert_eq!(frame.blocks[0].bounds, Some(rect(0.0, 0.0, 30.0, 40.0)));
    }

    #[test]
    fn resolves_the_candidate_caret_without_editing_the_document() {
        let document = doc!(p!("abcdef")).node;
        let mut frame = DisplayList::unmeasured(&document, &Selection::cursor(3), &DecorationGroup::default());
        frame.composition = Some(CompositionDisplay { from: 2, to: 4, text: "🙂".into(), selection: 2..2 });
        assert!(frame.selection.carets.is_empty());
        frame.measure(&TestLayout);
        assert_eq!(frame.selection.carets, [SelectionCaret { pos: 3, rect: rect(20.0, 0.0, 20.0, 20.0) }]);
        assert_eq!(frame.composition_caret, Some(rect(7.0, 4.0, 7.0, 18.0)));
        assert_eq!(frame.blocks[0].node.text_content(), "abcdef");
    }

    #[test]
    fn projects_node_selection_bounds() {
        let document = doc!(p!("first"), p!("second")).node;
        let frame = DisplayList::build(&document, &Selection::node(0), &DecorationGroup::default(), &TestLayout);
        assert_eq!(frame.selection.kind, SelectionKind::Node);
        assert_eq!(frame.selection.selected_nodes, [0]);
        assert_eq!(frame.selection.highlights, [rect(0.0, 0.0, 30.0, 40.0)]);
        assert!(frame.selection.carets.is_empty());
    }

    #[test]
    fn exposes_the_nested_accessibility_outline_and_active_text() {
        let document = doc!(p!("first"), "<quoted>", blockquote!(h1!("heading"), p!("in<caret>side")));
        let quote_pos = document.get_tag("quoted").unwrap();
        let caret = document.get_tag("caret").unwrap();
        let mut state = EditorState::create(EditorStateConfig {
            schema: test_schema().clone(), doc: Some(document.node), selection: Some(Selection::cursor(caret)),
            stored_marks: None, plugins: Vec::new(),
        });
        let tree = accessibility_tree(&state);
        let node = |id| tree.nodes.iter().find(|node| node.id == id).unwrap();
        let root = node(tree.root_id);
        assert_eq!(root.role, AccessibilityRole::Document);
        assert_eq!(root.children.len(), 2);
        let quote = node(root.children[1]);
        assert_eq!(quote.role, AccessibilityRole::Blockquote);
        assert_eq!(quote.children.len(), 2);
        assert_eq!(node(quote.children[0]).role, AccessibilityRole::Heading);
        let focused = node(tree.focus_id);
        assert_eq!(focused.role, AccessibilityRole::InlineTextBox);
        assert_eq!(focused.value.as_deref(), Some("inside"));

        let mut tr = state.tr();
        tr.set_selection(Selection::node(quote_pos));
        state = state.apply(&tr);
        let tree = accessibility_tree(&state);
        assert_eq!(tree.nodes.iter().find(|node| node.id == tree.focus_id).unwrap().role, AccessibilityRole::Blockquote);
    }

    #[test]
    fn scrolls_by_the_minimum_delta_to_reveal_geometry() {
        #[derive(Default)]
        struct ScrollHost(Vec<Point>);

        impl Layout for ScrollHost {
            fn coords_at_pos(&self, _: usize, _: i32) -> Option<Rect> { None }
            fn pos_at_coords(&self, _: Point) -> Option<PositionAtCoords> { None }
        }

        impl Host for ScrollHost {
            fn draw(&mut self, _: &DisplayList, _: &BlockUpdate) {}
            fn viewport(&self) -> Option<Rect> { Some(rect(0.0, 0.0, 80.0, 50.0)) }
            fn scroll_by(&mut self, delta: Point) { self.0.push(delta) }
        }

        let mut host = ScrollHost::default();
        host.scroll_into_view(rect(10.0, 2.0, 20.0, 12.0));
        host.scroll_into_view(rect(75.0, -5.0, 90.0, 20.0));
        host.scroll_into_view(rect(10.0, 54.0, 25.0, 150.0));
        assert_eq!(host.0, [Point { x: 10.0, y: -5.0 }, Point { x: 0.0, y: 54.0 }]);
    }
}
