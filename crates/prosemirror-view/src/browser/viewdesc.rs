//! Browser DOM descriptions and reconciliation from prosemirror-view 1.42.9.

use std::cell::Cell;
use std::rc::Rc;

use prosemirror_model::{DomNode as ModelDom, DomOutputSpec, DomSerializer, Mark, Node as ModelNode};
use prosemirror_state::EditorState;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, Event, HtmlElement, MutationRecord, Node};

use super::BrowserViewProps;
use crate::{Decoration, DecorationAttrs, DecorationGroup, DecorationType};

pub type GetPos = Rc<dyn Fn() -> Option<usize>>;
pub type NodeViewFactory = Rc<
    dyn Fn(&Document, &ModelNode, GetPos, &[Decoration], &DecorationGroup) -> Result<NodeView, JsValue>,
>;
pub type MarkViewFactory = Rc<dyn Fn(&Document, &Mark, bool) -> Result<MarkView, JsValue>>;
pub type WidgetViewFactory = Rc<dyn Fn(&Document, &EditorState, &Decoration, GetPos) -> Result<WidgetView, JsValue>>;
pub type UpdateNode = Rc<dyn Fn(&ModelNode, &[Decoration], &DecorationGroup) -> bool>;
pub type UpdateMark = Rc<dyn Fn(&Mark) -> bool>;
pub type StopEvent = Rc<dyn Fn(&Event) -> bool>;
pub type IgnoreMutation = Rc<dyn Fn(&ViewMutation) -> bool>;
pub type SetSelection = Rc<dyn Fn(usize, usize, &Node) -> Result<(), JsValue>>;
pub type DestroyWidget = Rc<dyn Fn(&Node)>;

#[derive(Clone)]
pub enum ViewMutation {
    Dom(MutationRecord),
    Selection(Node),
}

impl ViewMutation {
    pub fn target(&self) -> Option<Node> {
        match self {
            Self::Dom(record) => record.target(),
            Self::Selection(target) => Some(target.clone()),
        }
    }
}

pub struct NodeView {
    pub dom: Node,
    pub content_dom: Option<Node>,
    pub update: Option<UpdateNode>,
    pub multi_type: bool,
    pub select_node: Option<Rc<dyn Fn()>>,
    pub deselect_node: Option<Rc<dyn Fn()>>,
    pub set_selection: Option<SetSelection>,
    pub stop_event: Option<StopEvent>,
    pub ignore_mutation: Option<IgnoreMutation>,
    pub destroy: Option<Rc<dyn Fn()>>,
}

impl NodeView {
    pub fn new(dom: impl Into<Node>) -> Self {
        Self {
            dom: dom.into(), content_dom: None, update: None, multi_type: false,
            select_node: None, deselect_node: None, set_selection: None,
            stop_event: None, ignore_mutation: None, destroy: None,
        }
    }

    pub fn with_content_dom(mut self, content_dom: impl Into<Node>) -> Self {
        self.content_dom = Some(content_dom.into());
        self
    }
}

pub struct MarkView {
    pub dom: Node,
    pub content_dom: Option<Node>,
    pub update: Option<UpdateMark>,
    pub ignore_mutation: Option<IgnoreMutation>,
    pub destroy: Option<Rc<dyn Fn()>>,
}

impl MarkView {
    pub fn new(dom: impl Into<Node>) -> Self {
        Self { dom: dom.into(), content_dom: None, update: None, ignore_mutation: None, destroy: None }
    }

    pub fn with_content_dom(mut self, content_dom: impl Into<Node>) -> Self {
        self.content_dom = Some(content_dom.into());
        self
    }
}

pub struct WidgetView {
    pub dom: Node,
    pub stop_event: Option<StopEvent>,
    pub destroy: Option<DestroyWidget>,
}

impl WidgetView {
    pub fn new(dom: impl Into<Node>) -> Self {
        Self { dom: dom.into(), stop_event: None, destroy: None }
    }
}

#[derive(Clone)]
pub struct ViewDescInfo {
    pub dom: Node,
    pub content_dom: Option<Node>,
    pub model: Option<ModelNode>,
    pub from: usize,
    pub to: usize,
    pub border: usize,
}

#[derive(Clone)]
pub struct DomNodeDesc {
    pub dom: Node,
    pub content_dom: Option<Node>,
    pub node: ModelNode,
    pub pos_before: usize,
    pub pos_at_start: usize,
    pub pos_after: usize,
    pub is_root: bool,
}

pub struct DomParseRange {
    pub dom_parent: Node,
    pub from: usize,
    pub to: usize,
    pub from_offset: usize,
    pub to_offset: usize,
}

pub struct DocView {
    root: HtmlElement,
    description: Option<Desc>,
    composition_node: Option<Node>,
}

impl DocView {
    pub fn new(root: HtmlElement) -> Self {
        Self { root, description: None, composition_node: None }
    }

    pub fn update(
        &mut self, state: &EditorState, decorations: &DecorationGroup, props: &BrowserViewProps,
    ) -> Result<(), JsValue> {
        let document = self.root.owner_document().ok_or_else(|| error("Editor root has no owner document"))?;
        let renderer = Renderer { document: &document, state, props, locked_text: self.composition_node.as_ref() };
        let root = self.root.clone().into();
        let mut description = self.description.take().unwrap_or_else(|| Desc {
            dom: root,
            content_dom: Some(self.root.clone().into()),
            children: Vec::new(),
            from: 0,
            kind: Kind::Root(state.doc.clone()),
        });
        description.kind = Kind::Root(state.doc.clone());
        let wanted = renderer.describe_children(&state.doc, decorations, 0);
        let rendered = renderer.reconcile(&mut description, wanted);
        self.description = Some(description);
        rendered
    }

    pub fn destroy(&mut self) {
        if let Some(description) = self.description.take() {
            description.destroy();
        }
        self.root.set_text_content(None);
        self.composition_node = None;
    }

    pub fn set_composition_node(&mut self, node: Option<Node>) {
        self.composition_node = node;
    }

    pub fn dom_at_pos(&self, pos: usize, side: i32) -> Option<(Node, u32)> {
        let root = self.description.as_ref()?;
        (pos <= root.size()).then(|| root.dom_from_pos(pos, side)).flatten()
    }

    pub fn pos_at_dom(&self, node: &Node, offset: u32, bias: i32) -> Option<usize> {
        self.nearest(node, false).map(|desc| desc.local_pos(node, offset, bias))
    }

    pub fn pos_from_dom(&self, node: &Node, offset: u32, bias: i32) -> Option<usize> {
        self.pos_at_dom(node, offset, bias)
    }

    pub fn node_dom(&self, pos: usize) -> Option<Node> {
        let desc = self.description.as_ref()?.node_at(pos)?;
        let Kind::Node(node) = &desc.kind else { return None };
        Some(node.node_dom.clone())
    }

    pub fn nearest_desc(&self, node: &Node) -> Option<ViewDescInfo> {
        let found = self.nearest(node, false)?;
        Some(ViewDescInfo {
            dom: found.dom.clone(), content_dom: found.content_dom.clone(),
            model: found.model().cloned(), from: found.from, to: found.to(), border: found.border(),
        })
    }

    pub fn nearest_node_desc(&self, node: &Node) -> Option<DomNodeDesc> {
        let found = self.nearest(node, true)?;
        Some(DomNodeDesc {
            dom: found.dom.clone(), content_dom: found.content_dom.clone(), node: found.model()?.clone(),
            pos_before: found.from, pos_at_start: found.start(), pos_after: found.to(),
            is_root: matches!(found.kind, Kind::Root(_)),
        })
    }

    pub fn parse_range(&self, from: usize, to: usize) -> Option<DomParseRange> {
        self.description.as_ref()?.parse_range(from, to)
    }

    pub fn is_widget(&self, node: &Node) -> bool {
        self.nearest(node, false).is_some_and(|desc| matches!(desc.kind, Kind::Widget(_)))
    }

    pub fn ignore_for_coords(&self, node: &Node) -> bool {
        self.nearest(node, false).is_some_and(|desc| {
            matches!(desc.kind, Kind::Widget(_) | Kind::Hack(Hack::Separator))
        })
    }

    pub fn ignore_mutation(&self, mutation: &ViewMutation) -> bool {
        mutation.target().and_then(|target| self.nearest(&target, false))
            .is_some_and(|desc| desc.ignore_mutation(mutation))
    }

    pub fn stop_event(&self, event: &Event) -> bool {
        let Some(root) = self.description.as_ref() else { return false };
        let mut current = event.target().and_then(|target| target.dyn_into::<Node>().ok());
        while let Some(node) = current {
            let desc = root.find_dom(&node).filter(|desc| same(&desc.dom, &node));
            let stop = desc.and_then(|desc| match &desc.kind {
                Kind::Node(info) => info.custom.as_ref().and_then(|custom| custom.stop_event.as_ref()),
                Kind::Widget(widget) => widget.view.stop_event.as_ref(),
                _ => None,
            });
            if stop.is_some_and(|stop| stop(event)) {
                return true;
            }
            if same(&node, &root.dom) {
                break;
            }
            current = node.parent_node();
        }
        false
    }

    pub fn set_custom_selection(&self, anchor: usize, head: usize, root: &Node) -> Result<bool, JsValue> {
        self.description.as_ref().map_or(Ok(false), |desc| desc.custom_selection(anchor, head, root))
    }

    pub fn set_node_selected(&self, dom: &Node, selected: bool) -> Result<bool, JsValue> {
        let Some(Kind::Node(info)) = self.nearest(dom, true).map(|desc| &desc.kind) else { return Ok(false) };
        let Some(custom) = &info.custom else { return Ok(false) };
        let hook = if selected { &custom.select_node } else { &custom.deselect_node };
        if let Some(hook) = hook {
            hook();
            return Ok(true);
        }
        Ok(false)
    }

    pub fn select_node(&self, previous: Option<usize>, next: Option<usize>) -> Result<(), JsValue> {
        if previous != next {
            if let Some(previous) = previous {
                self.apply_selection(previous, false)?;
            }
            if let Some(next) = next {
                self.apply_selection(next, true)?;
            }
        }
        Ok(())
    }

    fn apply_selection(&self, pos: usize, selected: bool) -> Result<(), JsValue> {
        let Some(desc) = self.description.as_ref().and_then(|root| root.node_at(pos)) else { return Ok(()) };
        let Kind::Node(info) = &desc.kind else { return Ok(()) };
        if self.set_node_selected(&desc.dom, selected)? {
            return Ok(());
        }
        if let Some(dom) = info.node_dom.dyn_ref::<HtmlElement>() {
            if selected {
                dom.class_list().add_1("ProseMirror-selectednode")?;
            } else {
                dom.class_list().remove_1("ProseMirror-selectednode")?;
            }
            if desc.content_dom.is_some() || !info.node.node_type().is_some_and(|kind| kind.spec.draggable) {
                if selected {
                    dom.set_draggable(true);
                } else {
                    dom.remove_attribute("draggable")?;
                }
            }
        }
        Ok(())
    }

    fn nearest(&self, node: &Node, only_nodes: bool) -> Option<&Desc> {
        let root = self.description.as_ref()?;
        let mut here = Some(node.clone());
        while let Some(dom) = here {
            if let Some(desc) = root.find_dom(&dom) {
                if !only_nodes || desc.model().is_some() {
                    return Some(desc);
                }
            }
            here = dom.parent_node();
        }
        None
    }
}

struct Desc {
    dom: Node,
    content_dom: Option<Node>,
    children: Vec<Desc>,
    from: usize,
    kind: Kind,
}

enum Kind {
    Root(ModelNode),
    Node(NodeDesc),
    Mark(MarkDesc),
    Widget(WidgetDesc),
    Hack(Hack),
}

struct NodeDesc {
    node: ModelNode,
    node_dom: Node,
    pos: Rc<Cell<Option<usize>>>,
    outer: Vec<Decoration>,
    inner: DecorationGroup,
    levels: Vec<DecoLevel>,
    factory: Option<NodeViewFactory>,
    custom: Option<NodeView>,
}

struct MarkDesc {
    mark: Mark,
    factory: Option<MarkViewFactory>,
    custom: Option<MarkView>,
}

struct WidgetDesc {
    decoration: Decoration,
    pos: Rc<Cell<Option<usize>>>,
    view: WidgetView,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hack {
    Separator,
    TrailingBreak,
}

impl Desc {
    fn model(&self) -> Option<&ModelNode> {
        match &self.kind {
            Kind::Root(node) | Kind::Node(NodeDesc { node, .. }) => Some(node),
            _ => None,
        }
    }

    fn size(&self) -> usize {
        match &self.kind {
            Kind::Root(node) => node.content().size(),
            Kind::Node(info) => info.node.node_size(),
            Kind::Mark(_) => self.children.iter().map(Self::size).sum(),
            Kind::Widget(_) | Kind::Hack(_) => 0,
        }
    }

    fn border(&self) -> usize {
        usize::from(matches!(&self.kind, Kind::Node(info) if !info.node.is_leaf()))
    }

    fn to(&self) -> usize { self.from + self.size() }
    fn start(&self) -> usize { self.from + self.border() }
    fn end(&self) -> usize { self.to() - self.border() }

    fn atom(&self) -> bool {
        match &self.kind {
            Kind::Widget(_) | Kind::Hack(_) => true,
            Kind::Node(info) => !info.node.is_text() && info.node.is_atom(),
            _ => false,
        }
    }

    fn find_dom(&self, dom: &Node) -> Option<&Desc> {
        if same(&self.dom, dom)
            || self.content_dom.as_ref().is_some_and(|content| same(content, dom))
            || matches!(&self.kind, Kind::Node(info) if same(&info.node_dom, dom))
        {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find_dom(dom))
    }

    fn node_at(&self, pos: usize) -> Option<&Self> {
        for child in &self.children {
            if child.from <= pos && pos < child.to() {
                if child.from == pos && child.model().is_some() {
                    return Some(child);
                }
                return child.node_at(pos);
            }
        }
        None
    }

    fn dom_from_pos(&self, pos: usize, side: i32) -> Option<(Node, u32)> {
        if let Kind::Node(info) = &self.kind {
            if info.node.is_text() {
                return Some((info.node_dom.clone(), u32::try_from(pos - self.from).ok()?));
            }
        }
        let Some(content) = &self.content_dom else { return Some((self.dom.clone(), 0)) };
        let mut index = 0;
        while let Some(child) = self.children.get(index) {
            if child.to() > pos {
                if child.from < pos {
                    return child.dom_from_pos(pos, side);
                }
                break;
            }
            index += 1;
        }
        while let Some(child) = index.checked_sub(1).and_then(|i| self.children.get(i)) {
            let skip = match &child.kind {
                Kind::Widget(info) => matches!(info.decoration.kind, DecorationType::Widget { side, .. } if side >= 0),
                Kind::Hack(_) => true,
                _ => false,
            };
            if !skip || child.from != pos {
                break;
            }
            index -= 1;
        }

        if side <= 0 {
            for prev in (0..index).rev() {
                let child = &self.children[prev];
                if child.dom.parent_node().is_some_and(|parent| same(&parent, content)) {
                    if side < 0 && prev + 1 == index && child.border() == 0 && !child.atom() {
                        return child.dom_from_pos(child.to(), side);
                    }
                    return Some((content.clone(), dom_index(&child.dom)? + 1));
                }
            }
            Some((content.clone(), 0))
        } else {
            for (next, child) in self.children.iter().enumerate().skip(index) {
                if child.dom.parent_node().is_some_and(|parent| same(&parent, content)) {
                    if next == index && child.border() == 0 && !child.atom() {
                        return child.dom_from_pos(child.from, side);
                    }
                    return Some((content.clone(), dom_index(&child.dom)?));
                }
            }
            Some((content.clone(), content.child_nodes().length()))
        }
    }

    fn local_pos(&self, node: &Node, offset: u32, bias: i32) -> usize {
        match &self.kind {
            Kind::Node(info) if info.node.is_text() && same(node, &info.node_dom) =>
                return self.from + (offset as usize).min(info.node.node_size()),
            Kind::Widget(_) | Kind::Hack(_) => return self.from,
            _ => (),
        }
        if let Some(content) = &self.content_dom {
            if contains(content, node) {
                let before = bias < 0;
                let child = if same(content, node) {
                    if before {
                        offset.checked_sub(1).and_then(|offset| content.child_nodes().item(offset))
                    } else {
                        content.child_nodes().item(offset)
                    }
                } else {
                    direct_child(content, node).and_then(|child| {
                        if before { child.previous_sibling() } else { child.next_sibling() }
                    })
                };
                let mut scan = child;
                while let Some(candidate) = scan {
                    if let Some(desc) = self.children.iter().find(|child| same(&candidate, &child.dom)) {
                        return if before { desc.to() } else { desc.from };
                    }
                    scan = if before { candidate.previous_sibling() } else { candidate.next_sibling() };
                }
                return if before { self.start() } else { self.end() };
            }
            if contains(&self.dom, content) {
                if contains(node, content) {
                    if let Some(index) = direct_child(node, content).and_then(|child| dom_index(&child)) {
                        return if offset > index { self.end() } else { self.start() };
                    }
                } else {
                    let relation = node.compare_document_position(content);
                    return if relation & Node::DOCUMENT_POSITION_FOLLOWING != 0 { self.start() } else { self.end() };
                }
            }
        }
        if offset == 0 && dom_edge(&self.dom, node, true) {
            self.start()
        } else if offset == node.child_nodes().length() && dom_edge(&self.dom, node, false) || bias > 0 {
            self.end()
        } else {
            self.start()
        }
    }

    fn parse_range(&self, mut from: usize, mut to: usize) -> Option<DomParseRange> {
        let content = self.content_dom.as_ref()?;
        for child in &self.children {
            if child.model().is_some() && from >= child.start() && to <= child.end()
                && child.content_dom.as_ref().is_some_and(|node| contains(content, node))
            {
                return child.parse_range(from, to);
            }
        }
        if self.children.is_empty() {
            return Some(DomParseRange {
                dom_parent: content.clone(), from, to, from_offset: 0, to_offset: content.child_nodes().length() as usize,
            });
        }
        let first = self.children.iter().position(|child| from <= child.to()).unwrap_or(self.children.len() - 1);
        let last = self.children.iter().rposition(|child| to > child.from).unwrap_or(first).max(first);
        from = self.children[first].from;
        to = self.children[last].to();
        let mut from_offset = 0;
        for previous in self.children[..first].iter().rev() {
            if previous.size() != 0 && previous.dom.parent_node().is_some_and(|parent| same(&parent, content))
                && !previous.empty_child_at(false)
            {
                from_offset = dom_index(&previous.dom)? as usize + 1;
                break;
            }
            from = previous.from;
        }
        let mut to_offset = content.child_nodes().length() as usize;
        for next in &self.children[last + 1..] {
            if next.size() != 0 && next.dom.parent_node().is_some_and(|parent| same(&parent, content))
                && !next.empty_child_at(true)
            {
                to_offset = dom_index(&next.dom)? as usize;
                break;
            }
            to = next.to();
        }
        Some(DomParseRange { dom_parent: content.clone(), from, to, from_offset, to_offset })
    }

    fn empty_child_at(&self, first: bool) -> bool {
        if self.border() != 0 || self.content_dom.is_none() {
            return false;
        }
        let child = if first { self.children.first() } else { self.children.last() };
        child.is_some_and(|child| child.size() == 0 || child.empty_child_at(first))
    }

    fn custom_selection(&self, anchor: usize, head: usize, root: &Node) -> Result<bool, JsValue> {
        let from = anchor.min(head);
        let to = anchor.max(head);
        for child in &self.children {
            if from > child.from && to < child.to() {
                return child.custom_selection(anchor, head, root);
            }
        }
        if let Kind::Node(info) = &self.kind {
            if let Some(callback) = info.custom.as_ref().and_then(|view| view.set_selection.as_ref()) {
                callback(anchor - self.start(), head - self.start(), root)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn ignore_mutation(&self, mutation: &ViewMutation) -> bool {
        let selection = matches!(mutation, ViewMutation::Selection(_));
        match &self.kind {
            Kind::Node(info) => {
                if let Some(callback) = info.custom.as_ref().and_then(|view| view.ignore_mutation.as_ref()) {
                    return callback(mutation);
                }
                if info.node.is_text() {
                    return matches!(mutation, ViewMutation::Dom(record) if record.type_() != "characterData");
                }
                self.content_dom.is_none() && !selection
            }
            Kind::Mark(info) => info.custom.as_ref().and_then(|view| view.ignore_mutation.as_ref())
                .is_some_and(|callback| callback(mutation)),
            Kind::Widget(info) => !selection || info.decoration.spec()["ignoreSelection"].as_bool().unwrap_or(false),
            Kind::Hack(_) => !selection,
            Kind::Root(_) => false,
        }
    }

    fn destroy(self) {
        match self.kind {
            Kind::Node(info) => {
                info.pos.set(None);
                if let Some(destroy) = info.custom.and_then(|view| view.destroy) {
                    destroy();
                }
            }
            Kind::Mark(info) => {
                if let Some(destroy) = info.custom.and_then(|view| view.destroy) {
                    destroy();
                }
            }
            Kind::Widget(info) => {
                info.pos.set(None);
                if let Some(destroy) = info.view.destroy {
                    destroy(&self.dom);
                }
            }
            Kind::Root(_) | Kind::Hack(_) => (),
        }
        for child in self.children {
            child.destroy();
        }
    }
}

struct Renderer<'a> {
    document: &'a Document,
    state: &'a EditorState,
    props: &'a BrowserViewProps,
    locked_text: Option<&'a Node>,
}

struct Want {
    from: usize,
    kind: WantKind,
}

enum WantKind {
    Node { node: ModelNode, outer: Vec<Decoration>, inner: DecorationGroup },
    Mark { mark: Mark, inline: bool, children: Vec<Want> },
    Widget(Decoration),
    Hack(Hack),
}

impl Renderer<'_> {
    fn describe_children(&self, node: &ModelNode, decoration: &DecorationGroup, base: usize) -> Vec<Want> {
        let locals = decoration.locals(node);
        let mut children = Vec::new();
        let mut offset = 0;
        let mut preceding = &[][..];
        let mut before_atom = node.inline_content() && node.is_block();
        let inline = node.inline_content();
        for child in &node.content().children {
            let end = offset + child.node_size();
            self.emit_widgets(&mut children, &locals, offset, base, preceding, child.marks(), inline);
            if inline && before_atom && child.is_atom() && !child.is_text() {
                append_marked(&mut children, child.marks(), inline, Want { from: base + offset, kind: WantKind::Hack(Hack::Separator) });
            }
            if child.is_text() {
                let mut cuts = vec![offset, end];
                for deco in &locals {
                    if deco.from > offset && deco.from < end {
                        cuts.push(deco.from);
                    }
                    if deco.to > offset && deco.to < end {
                        cuts.push(deco.to);
                    }
                }
                cuts.sort_unstable();
                cuts.dedup();
                for bounds in cuts.windows(2) {
                    let from = bounds[0];
                    let to = bounds[1];
                    if from > offset {
                        self.emit_widgets(&mut children, &locals, from, base, child.marks(), child.marks(), inline);
                    }
                    let model = if from == offset && to == end {
                        child.clone()
                    } else {
                        child.cut(from - offset, to - offset)
                    };
                    let outer = covering(&locals, from, to, true);
                    append_marked(&mut children, child.marks(), inline, Want {
                        from: base + from,
                        kind: WantKind::Node { node: model, outer, inner: DecorationGroup::default() },
                    });
                }
            } else {
                let outer = covering(&locals, offset, end, child.is_inline() && child.is_leaf());
                append_marked(&mut children, child.marks(), inline, Want {
                    from: base + offset,
                    kind: WantKind::Node { node: child.clone(), outer, inner: decoration.for_child(offset, child) },
                });
            }
            before_atom = child.is_atom() && !child.is_text();
            preceding = child.marks();
            offset = end;
        }
        self.emit_widgets(&mut children, &locals, offset, base, preceding, &[], inline);
        if node.is_textblock() {
            let last = last_content(&children);
            let needs_trailing = !matches!(last.map(|child| &child.kind),
                Some(WantKind::Node { node, .. }) if node.text().is_some_and(|text| !text.ends_with('\n'))
            );
            if needs_trailing {
                if last.is_some_and(|last| match &last.kind {
                    WantKind::Node { node, .. } => node.is_atom() && !node.is_text() && node.type_name() != "hard_break",
                    WantKind::Widget(deco) => !deco.spec()["raw"].as_bool().unwrap_or(false),
                    _ => false,
                }) && self.chrome_or_safari() {
                    append_hack_to_last_mark(&mut children, Want { from: base + offset, kind: WantKind::Hack(Hack::Separator) });
                }
                children.push(Want { from: base + offset, kind: WantKind::Hack(Hack::TrailingBreak) });
            }
        }
        children
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_widgets(
        &self, children: &mut Vec<Want>, locals: &[Decoration], offset: usize, base: usize,
        preceding: &[Mark], following: &[Mark], inline: bool,
    ) {
        let mut widgets: Vec<_> = locals.iter().filter_map(|deco| match deco.kind {
            DecorationType::Widget { side, .. } if deco.from == offset => Some((side, deco)),
            _ => None,
        }).collect();
        widgets.sort_by_key(|(side, _)| *side);
        for (side, deco) in widgets {
            let explicit = deco.spec()["marks"].as_array().map(|marks| marks.iter().filter_map(|mark| {
                self.state.schema.mark_from_json(mark).ok()
            }).collect::<Vec<_>>());
            let marks = explicit.as_deref().unwrap_or(if side < 0 { preceding } else { following });
            append_marked(children, marks, inline, Want { from: base + offset, kind: WantKind::Widget(deco.clone()) });
        }
    }

    fn chrome_or_safari(&self) -> bool {
        self.document.default_view().and_then(|window| window.navigator().user_agent().ok())
            .is_some_and(|ua| ua.contains("Chrome/") || ua.contains("Safari/"))
    }

    fn reconcile(&self, parent: &mut Desc, wanted: Vec<Want>) -> Result<(), JsValue> {
        let mut old: Vec<_> = std::mem::take(&mut parent.children).into_iter().map(Some).collect();
        let reserved = pre_match(&old, &wanted);
        let mut previous = 0;
        let mut result = Vec::with_capacity(wanted.len());
        for (index, mut want) in wanted.into_iter().enumerate() {
            let matched = (previous..old.len()).find(|&i| reserved[i] == Some(index) && old[i].is_some());
            let exact = matched.or_else(|| {
                (previous..old.len().min(previous + 5))
                    .take_while(|&i| reserved[i].is_none())
                    .find(|&i| old[i].as_ref().is_some_and(|desc| self.exact(desc, &want)))
            });
            let next = exact.or_else(|| self.update_candidate(&old, &reserved, previous, &want));
            if let Some(next) = next {
                let mut candidate = old[next].take().expect("a candidate was found");
                let applied = self.update(&mut candidate, &mut want);
                match applied {
                    Ok(true) => {
                        for previous in old[previous..next].iter_mut().filter_map(Option::take) {
                            previous.destroy();
                        }
                        previous = next + 1;
                        result.push(candidate);
                        continue;
                    }
                    Ok(false) => old[next] = Some(candidate),
                    Err(error) => {
                        result.push(candidate);
                        result.extend(old.into_iter().flatten());
                        parent.children = result;
                        return Err(error);
                    }
                }
            }

            let wrapper = next.filter(|&i| reserved[i].is_none()).filter(|&i| {
                old[i].as_ref().is_some_and(|desc| same_node_content(desc, &want))
            }).and_then(|i| old[i].take());
            match self.create(want, wrapper) {
                Ok(node) => result.push(node),
                Err(error) => {
                    result.extend(old.into_iter().flatten());
                    parent.children = result;
                    return Err(error);
                }
            }
        }
        for node in old.into_iter().flatten() {
            node.destroy();
        }
        parent.children = result;
        if let Some(content) = &parent.content_dom {
            mount_children(content, &parent.children)?;
        }
        Ok(())
    }

    fn update_candidate(
        &self, old: &[Option<Desc>], reserved: &[Option<usize>], start: usize, want: &Want,
    ) -> Option<usize> {
        for (index, desc) in old.iter().enumerate().skip(start) {
            let Some(desc) = desc else { continue };
            if reserved[index].is_some() {
                return None;
            }
            match (&want.kind, &desc.kind) {
                (WantKind::Node { .. }, Kind::Node(_)) => return Some(index),
                (WantKind::Node { .. }, Kind::Widget(_) | Kind::Hack(_)) => continue,
                (WantKind::Mark { .. }, Kind::Mark(_)) if index == start => return Some(index),
                _ => return None,
            }
        }
        None
    }

    fn exact(&self, desc: &Desc, want: &Want) -> bool {
        match (&desc.kind, &want.kind) {
            (Kind::Node(current), WantKind::Node { node, outer, inner }) => {
                current.node == *node && same_outer(&current.outer, outer) && current.inner == *inner
                    && self.node_factory_matches(current, node)
            }
            (Kind::Mark(current), WantKind::Mark { mark, .. }) => {
                current.mark == *mark && same_factory(&current.factory, self.props.mark_views.get(&mark.type_name))
            }
            (Kind::Widget(current), WantKind::Widget(widget)) => same_widget(&current.decoration, widget),
            (Kind::Hack(current), WantKind::Hack(kind)) => current == kind,
            _ => false,
        }
    }

    fn node_factory_matches(&self, current: &NodeDesc, node: &ModelNode) -> bool {
        if current.node.type_name() != node.type_name() && current.custom.as_ref().is_some_and(|view| view.multi_type) {
            same_factory(&current.factory, self.props.node_views.get(current.node.type_name()))
        } else {
            same_factory(&current.factory, self.props.node_views.get(node.type_name()))
        }
    }

    fn update(&self, desc: &mut Desc, want: &mut Want) -> Result<bool, JsValue> {
        match (&mut desc.kind, &mut want.kind) {
            (Kind::Node(current), WantKind::Node { node, outer, inner }) => {
                if !self.node_factory_matches(current, node) {
                    return Ok(false);
                }
                let same_model = current.node == *node;
                let same_deco = same_outer(&current.outer, outer) && current.inner == *inner;
                if !same_model || !same_deco {
                    if let Some(custom) = &current.custom {
                        let updater = custom.update.as_ref().filter(|_| {
                            current.node.type_name() == node.type_name() || custom.multi_type
                        });
                        if let Some(update) = updater {
                            if !update(node, outer, inner) {
                                return Ok(false);
                            }
                        } else if desc.content_dom.is_none() && !node.is_leaf() || !current.node.same_markup(node) {
                            return Ok(false);
                        }
                    } else if !current.node.same_markup(node) {
                        return Ok(false);
                    }
                }

                current.pos.set(Some(want.from));
                if node.is_text()
                    && !self.locked_text.is_some_and(|text| same(text, &current.node_dom))
                {
                    write_text(&current.node_dom, node)?;
                }
                if !same_outer(&current.outer, outer) {
                    desc.dom = patch_outer(self.document, &current.node_dom, &mut current.levels, outer, node.is_inline())?;
                    current.outer.clone_from(outer);
                }
                current.node = node.clone();
                current.inner.clone_from(inner);
                desc.from = want.from;
                if desc.content_dom.is_some() {
                    let wanted = self.describe_children(node, inner, desc.start());
                    self.reconcile(desc, wanted)?;
                }
                Ok(true)
            }
            (Kind::Mark(current), WantKind::Mark { mark, children, .. }) => {
                let same_factory = same_factory(&current.factory, self.props.mark_views.get(&mark.type_name));
                if !same_factory || current.mark != *mark && (
                    current.mark.type_name != mark.type_name
                        || !current.custom.as_ref().and_then(|spec| spec.update.as_ref()).is_some_and(|update| update(mark))
                ) {
                    return Ok(false);
                }
                current.mark = mark.clone();
                desc.from = want.from;
                self.reconcile(desc, std::mem::take(children))?;
                Ok(true)
            }
            (Kind::Widget(current), WantKind::Widget(widget)) if same_widget(&current.decoration, widget) => {
                current.pos.set(Some(want.from));
                current.decoration = widget.clone();
                desc.from = want.from;
                Ok(true)
            }
            (Kind::Hack(current), WantKind::Hack(kind)) if current == kind => {
                desc.from = want.from;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn create(&self, want: Want, wrapper: Option<Desc>) -> Result<Desc, JsValue> {
        match want.kind {
            WantKind::Node { node, outer, inner } => self.create_node(want.from, node, outer, inner, wrapper),
            WantKind::Mark { mark, inline, children } => self.create_mark(want.from, mark, inline, children),
            WantKind::Widget(deco) => self.create_widget(want.from, deco),
            WantKind::Hack(kind) => self.create_hack(want.from, kind),
        }
    }

    fn create_node(
        &self, from: usize, node: ModelNode, outer: Vec<Decoration>, inner: DecorationGroup, mut wrapper: Option<Desc>,
    ) -> Result<Desc, JsValue> {
        let (pos, get_pos) = position(from);
        let factory = self.props.node_views.get(node.type_name()).cloned();
        let custom = factory.as_ref().map(|factory| factory(self.document, &node, get_pos, &outer, &inner)).transpose()?;
        let node_type = node.node_type().or_else(|| self.state.schema.node_type(node.type_name()));
        let (node_dom, content_dom) = if let Some(custom) = &custom {
            (custom.dom.clone(), custom.content_dom.clone())
        } else if node.is_text() {
            let text: Node = self.document.create_text_node(node.text().unwrap_or_default()).into();
            write_text(&text, &node)?;
            (text, None)
        } else {
            let to_dom = node_type.and_then(|kind| kind.spec.to_dom)
                .ok_or_else(|| error(&format!("No DOM serializer for {}", node.type_name())))?;
            self.render_spec(to_dom(&node))?
        };
        if node.is_text() && node_dom.node_type() != Node::TEXT_NODE {
            return Err(error("Text must be rendered as a DOM text node"));
        }
        if content_dom.is_none() && !node.is_text() && node_dom.node_name() != "BR" {
            if let Some(dom) = node_dom.dyn_ref::<HtmlElement>() {
                if !dom.has_attribute("contenteditable") {
                    dom.set_content_editable("false");
                }
                if node_type.is_some_and(|kind| kind.spec.draggable) {
                    dom.set_draggable(true);
                }
            }
        }
        let mut levels = Vec::new();
        let dom = patch_outer(self.document, &node_dom, &mut levels, &outer, node.is_inline())?;
        let children = if content_dom.is_some() {
            wrapper.as_mut().map_or_else(Vec::new, |wrapper| std::mem::take(&mut wrapper.children))
        } else {
            Vec::new()
        };
        if let Some(wrapper) = wrapper {
            wrapper.destroy();
        }
        let mut desc = Desc {
            dom, content_dom, children, from,
            kind: Kind::Node(NodeDesc { node: node.clone(), node_dom, pos, outer, inner: inner.clone(), levels, factory, custom }),
        };
        if desc.content_dom.is_some() {
            let wanted = self.describe_children(&node, &inner, desc.start());
            self.reconcile(&mut desc, wanted)?;
        }
        Ok(desc)
    }

    fn create_mark(&self, from: usize, mark: Mark, inline: bool, children: Vec<Want>) -> Result<Desc, JsValue> {
        let factory = self.props.mark_views.get(&mark.type_name).cloned();
        let custom = factory.as_ref().map(|factory| factory(self.document, &mark, inline)).transpose()?;
        let (dom, content_dom) = if let Some(custom) = &custom {
            (custom.dom.clone(), custom.content_dom.clone())
        } else {
            let to_dom = self.state.schema.mark_type(&mark.type_name).and_then(|kind| kind.spec.to_dom)
                .ok_or_else(|| error(&format!("No DOM serializer for mark {}", mark.type_name)))?;
            self.render_spec(to_dom(&mark, inline))?
        };
        let mut desc = Desc {
            content_dom: Some(content_dom.unwrap_or_else(|| dom.clone())), dom, children: Vec::new(), from,
            kind: Kind::Mark(MarkDesc { mark, factory, custom }),
        };
        self.reconcile(&mut desc, children)?;
        Ok(desc)
    }

    fn create_widget(&self, from: usize, decoration: Decoration) -> Result<Desc, JsValue> {
        let DecorationType::Widget { id, .. } = decoration.kind else { unreachable!() };
        let (pos, get_pos) = position(from);
        let mut view = if let Some(factory) = self.props.widgets.get(&id) {
            factory(self.document, self.state, &decoration, get_pos)?
        } else {
            let fallback = self.document.create_element("span")?;
            fallback.set_attribute("data-widget-id", &id.to_string())?;
            WidgetView::new(fallback)
        };
        if !decoration.spec()["raw"].as_bool().unwrap_or(false) {
            if view.dom.node_type() != Node::ELEMENT_NODE {
                let wrapper = self.document.create_element("span")?;
                wrapper.append_child(&view.dom)?;
                view.dom = wrapper.into();
            }
            if let Some(element) = view.dom.dyn_ref::<Element>() {
                if !element.has_attribute("contenteditable") {
                    element.set_attribute("contenteditable", "false")?;
                }
                element.class_list().add_1("ProseMirror-widget")?;
            }
        }
        Ok(Desc {
            dom: view.dom.clone(), content_dom: None, children: Vec::new(), from,
            kind: Kind::Widget(WidgetDesc { decoration, pos, view }),
        })
    }

    fn create_hack(&self, from: usize, kind: Hack) -> Result<Desc, JsValue> {
        let dom = match kind {
            Hack::Separator => {
                let image = self.document.create_element("img")?;
                image.set_class_name("ProseMirror-separator");
                image.set_attribute("alt", "")?;
                image
            }
            Hack::TrailingBreak => {
                let line_break = self.document.create_element("br")?;
                line_break.set_class_name("ProseMirror-trailingBreak");
                line_break
            }
        };
        Ok(Desc { dom: dom.into(), content_dom: None, children: Vec::new(), from, kind: Kind::Hack(kind) })
    }

    fn render_spec(&self, spec: DomOutputSpec) -> Result<(Node, Option<Node>), JsValue> {
        let rendered = DomSerializer::render_spec(spec).map_err(|err| error(&err.to_string()))?;
        let dom = render_dom(self.document, &rendered.dom)?;
        let content_dom = rendered.content_path.map(|path| {
            let mut element = dom.clone();
            for index in path {
                element = element.child_nodes().item(index as u32)
                    .ok_or_else(|| error("Invalid DOM content hole"))?;
            }
            Ok::<_, JsValue>(element)
        }).transpose()?;
        Ok((dom, content_dom))
    }
}

fn covering(locals: &[Decoration], from: usize, to: usize, inline: bool) -> Vec<Decoration> {
    locals.iter().filter(|deco| match deco.kind {
        DecorationType::Inline { .. } => inline && deco.from <= from && deco.to >= to,
        DecorationType::Node { .. } => deco.from == from && deco.to == to,
        DecorationType::Widget { .. } => false,
    }).cloned().collect()
}

fn append_marked(out: &mut Vec<Want>, marks: &[Mark], inline: bool, leaf: Want) {
    let Some((mark, rest)) = marks.split_first() else {
        out.push(leaf);
        return;
    };
    let reuse = mark.is_spanning() && matches!(out.last().map(|last| &last.kind),
        Some(WantKind::Mark { mark: current, .. }) if current == mark
    );
    if !reuse {
        out.push(Want {
            from: leaf.from,
            kind: WantKind::Mark { mark: mark.clone(), inline, children: Vec::new() },
        });
    }
    let WantKind::Mark { children, .. } = &mut out.last_mut().expect("active mark").kind else { unreachable!() };
    append_marked(children, rest, inline, leaf);
}

fn last_content(nodes: &[Want]) -> Option<&Want> {
    match &nodes.last()?.kind {
        WantKind::Mark { children, .. } => last_content(children),
        _ => nodes.last(),
    }
}

fn append_hack_to_last_mark(nodes: &mut Vec<Want>, hack: Want) {
    if let Some(WantKind::Mark { children, .. }) = nodes.last_mut().map(|node| &mut node.kind) {
        append_hack_to_last_mark(children, hack);
    } else {
        nodes.push(hack);
    }
}

fn pre_match(old: &[Option<Desc>], wanted: &[Want]) -> Vec<Option<usize>> {
    let mut assigned = vec![None; old.len()];
    let existing = old.iter().enumerate().rev().filter_map(|(index, desc)| {
        let desc = desc.as_ref()?;
        matches!(desc.kind, Kind::Node(_) | Kind::Mark(_)).then_some((index, desc))
    });
    let new = wanted.iter().enumerate().rev().filter(|(_, item)| matches!(item.kind, WantKind::Node { .. } | WantKind::Mark { .. }));
    for ((old_index, desc), (new_index, want)) in existing.zip(new) {
        let matched = match (&desc.kind, &want.kind) {
            (Kind::Node(current), WantKind::Node { node, .. }) => current.node.ptr_eq(node),
            (Kind::Mark(current), WantKind::Mark { mark, .. }) => current.mark == *mark,
            _ => false,
        };
        if !matched {
            break;
        }
        assigned[old_index] = Some(new_index);
    }
    assigned
}

fn same_node_content(desc: &Desc, want: &Want) -> bool {
    matches!((&desc.kind, &want.kind),
        (Kind::Node(current), WantKind::Node { node, outer, inner })
            if !node.is_atom() && !desc.children.is_empty() && current.node.content() == node.content()
                && same_outer(&current.outer, outer) && current.inner == *inner
    )
}

fn same_factory<T: ?Sized>(old: &Option<Rc<T>>, next: Option<&Rc<T>>) -> bool {
    match (old, next) {
        (Some(old), Some(next)) => Rc::ptr_eq(old, next),
        (None, None) => true,
        _ => false,
    }
}

fn same_outer(a: &[Decoration], b: &[Decoration]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.kind == b.kind && a.spec() == b.spec())
}

fn same_widget(a: &Decoration, b: &Decoration) -> bool {
    let key = a.spec()["key"].as_str().filter(|key| !key.is_empty());
    key.is_some_and(|key| b.spec()["key"].as_str() == Some(key)) || a.kind == b.kind && a.spec() == b.spec()
}

#[derive(Default)]
struct DecoLevel {
    tag: Option<String>,
    attrs: DecorationAttrs,
    dom: Option<Node>,
}

fn decoration_levels(decorations: &[Decoration], wrap: bool, inline: bool) -> Vec<DecoLevel> {
    let mut levels = vec![DecoLevel::default()];
    for deco in decorations {
        let attrs = match &deco.kind {
            DecorationType::Node { attrs } | DecorationType::Inline { attrs, .. } => attrs,
            _ => continue,
        };
        if let Some(tag) = attrs.get("nodeName").filter(|tag| !tag.is_empty()) {
            levels.push(DecoLevel { tag: Some(tag.clone()), ..DecoLevel::default() });
        }
        for (key, value) in attrs.iter().filter(|(key, _)| key.as_str() != "nodeName") {
            if wrap && levels.len() == 1 {
                levels.push(DecoLevel {
                    tag: Some(if inline { "span" } else { "div" }.to_owned()),
                    ..DecoLevel::default()
                });
            }
            let attrs = &mut levels.last_mut().expect("base decoration level").attrs;
            let separator = match key.as_str() {
                "style" => Some(';'),
                "class" => Some(' '),
                _ => None,
            };
            if let Some(previous) = separator.and_then(|_| attrs.get_mut(key)).filter(|value| !value.is_empty()) {
                previous.push(separator.expect("special decoration attribute"));
                previous.push_str(value);
            } else {
                attrs.insert(key.clone(), value.clone());
            }
        }
    }
    levels
}

fn patch_outer(
    document: &Document, node: &Node, previous: &mut Vec<DecoLevel>,
    decorations: &[Decoration], inline: bool,
) -> Result<Node, JsValue> {
    let mut levels = decoration_levels(decorations, node.node_type() != Node::ELEMENT_NODE, inline);
    let mut dom = node.clone();
    let empty = DecorationAttrs::new();
    for (i, level) in levels.iter_mut().enumerate() {
        let mut old = previous.get(i);
        if let Some(tag) = &level.tag {
            let wrapper = old.filter(|old| old.tag.as_ref() == Some(tag))
                .and_then(|old| old.dom.as_ref())
                .filter(|parent| dom.parent_node().is_some_and(|dom_parent| same(parent, &dom_parent)));
            if let Some(parent) = wrapper {
                dom = parent.clone();
            } else {
                let parent = document.create_element(tag)?;
                parent.append_child(&dom)?;
                dom = parent.into();
                old = None;
            }
        }
        if let Some(element) = dom.dyn_ref::<Element>() {
            patch_attributes(document, element, old.map_or(&empty, |old| &old.attrs), &level.attrs)?;
        }
        level.dom = Some(dom.clone());
    }
    *previous = levels;
    Ok(dom)
}

fn patch_attributes(
    document: &Document, element: &Element, previous: &DecorationAttrs, current: &DecorationAttrs,
) -> Result<(), JsValue> {
    for key in previous.keys() {
        if !matches!(key.as_str(), "class" | "style") && !current.contains_key(key) {
            element.remove_attribute(key)?;
        }
    }
    for (key, value) in current {
        if !matches!(key.as_str(), "class" | "style") && previous.get(key) != Some(value) {
            element.set_attribute(key, value)?;
        }
    }
    if previous.get("class") != current.get("class") {
        let previous: Vec<_> = previous.get("class").into_iter().flat_map(|value| value.split_whitespace()).collect();
        let next: Vec<_> = current.get("class").into_iter().flat_map(|value| value.split_whitespace()).collect();
        let classes = element.class_list();
        for class in previous {
            if !next.contains(&class) {
                classes.remove_1(class)?;
            }
        }
        for class in next {
            classes.add_1(class)?;
        }
        if classes.length() == 0 {
            element.remove_attribute("class")?;
        }
    }
    if previous.get("style") != current.get("style") {
        if let Some(html) = element.dyn_ref::<HtmlElement>() {
            let declaration = html.style();
            if let Some(style) = previous.get("style") {
                let scratch = document.create_element("div")?.dyn_into::<HtmlElement>()?;
                let properties = scratch.style();
                properties.set_css_text(style);
                for index in 0..properties.length() {
                    declaration.remove_property(&properties.item(index))?;
                }
            }
            if let Some(style) = current.get("style") {
                let mut css = declaration.css_text();
                css.push_str(style);
                declaration.set_css_text(&css);
            }
        } else if let Some(style) = current.get("style") {
            element.set_attribute("style", style)?;
        } else {
            element.remove_attribute("style")?;
        }
    }
    Ok(())
}

fn mount_children(parent: &Node, children: &[Desc]) -> Result<(), JsValue> {
    let mut mounted = parent.first_child();
    for (index, desc) in children.iter().enumerate() {
        let target = &desc.dom;
        if target.parent_node().is_some_and(|current| same(parent, &current)) {
            while let Some(node) = mounted.as_ref().filter(|node| !same(node, target)) {
                if children[index + 1..].iter().any(|desc| same(&desc.dom, node)) {
                    parent.insert_before(target, Some(node))?;
                    break;
                }
                let next = node.next_sibling();
                parent.remove_child(node)?;
                mounted = next;
            }
            mounted = target.next_sibling();
        } else {
            parent.insert_before(target, mounted.as_ref())?;
        }
    }
    while let Some(node) = mounted {
        mounted = node.next_sibling();
        parent.remove_child(&node)?;
    }
    Ok(())
}

fn position(from: usize) -> (Rc<Cell<Option<usize>>>, GetPos) {
    let cell = Rc::new(Cell::new(Some(from)));
    let handle = cell.clone();
    (cell, Rc::new(move || handle.get()))
}

fn error(message: &str) -> JsValue {
    js_sys::Error::new(message).into()
}

fn same(a: &Node, b: &Node) -> bool {
    a.is_same_node(Some(b))
}

fn contains(parent: &Node, child: &Node) -> bool {
    parent.contains(Some(child))
}

fn dom_index(dom: &Node) -> Option<u32> {
    dom.parent_node()?;
    let mut index = 0;
    let mut previous = dom.previous_sibling();
    while let Some(node) = previous {
        index += 1;
        previous = node.previous_sibling();
    }
    Some(index)
}

fn direct_child(parent: &Node, child: &Node) -> Option<Node> {
    let mut node = child.clone();
    while let Some(current) = node.parent_node() {
        if same(&current, parent) {
            return Some(node);
        }
        node = current;
    }
    None
}

fn dom_edge(ancestor: &Node, node: &Node, first: bool) -> bool {
    let mut next = Some(node.clone());
    while let Some(node) = next {
        if same(ancestor, &node) {
            return true;
        }
        let sibling = if first { node.previous_sibling() } else { node.next_sibling() };
        if sibling.is_some() {
            return false;
        }
        next = node.parent_node();
    }
    false
}

fn write_text(dom: &Node, node: &ModelNode) -> Result<(), JsValue> {
    let text = node.text().unwrap_or_default();
    if let Some(units) = node.text_utf16().filter(|units| !text.encode_utf16().eq(units.iter().copied())) {
        let value: JsValue = js_sys::JsString::from_char_code(&units).into();
        let property = JsValue::from_str("nodeValue");
        if js_sys::Reflect::get(dom.as_ref(), &property)? != value {
            js_sys::Reflect::set(dom.as_ref(), &property, &value)?;
        }
    } else if dom.node_value().as_deref() != Some(text) {
        dom.set_node_value(Some(text));
    }
    Ok(())
}

/// Converts a rendered schema DOM spec into nodes owned by the browser
/// document that contains the editor.
pub fn render_dom(document: &Document, dom: &ModelDom) -> Result<Node, JsValue> {
    match dom {
        ModelDom::Text(text) => Ok(document.create_text_node(text).into()),
        ModelDom::Comment(text) => Ok(document.create_comment(text).into()),
        ModelDom::Fragment(children) => {
            let fragment: Node = document.create_document_fragment().into();
            for child in children {
                fragment.append_child(&render_dom(document, child)?)?;
            }
            Ok(fragment)
        }
        ModelDom::Element(model) => {
            let element = if let Some(namespace) = model.namespace.as_deref() {
                document.create_element_ns(Some(namespace), &model.name)?
            } else {
                document.create_element(&model.name)?
            };
            for attr in &model.attributes {
                if let Some(namespace) = attr.namespace.as_deref() {
                    element.set_attribute_ns(Some(namespace), &attr.name, &attr.value)?;
                } else {
                    element.set_attribute(&attr.name, &attr.value)?;
                }
            }
            for child in &model.children {
                element.append_child(&render_dom(document, child)?)?;
            }
            Ok(element.into())
        }
    }
}
