use js_sys::{Function, Reflect};
use prosemirror_model::Node as ModelNode;
use prosemirror_state::EditorState;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, DomRect, DomRectList, Element, FocusOptions, HtmlElement, Node, Range};

use super::selection::{
    active_element, browser_flags, contains, native_selection, node_size, same, uneditable,
    write_selection, WriteSelectionOptions,
};
use super::viewdesc::DocView;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewportPoint {
    pub left: f64,
    pub top: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewportRect {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

impl ViewportRect {
    fn nonzero(self) -> bool {
        self.top < self.bottom || self.left < self.right
    }

    fn contains(self, point: ViewportPoint) -> bool {
        point.left >= self.left - 1.0
            && point.left <= self.right + 1.0
            && point.top >= self.top - 1.0
            && point.top <= self.bottom + 1.0
    }

    fn flatten_vertical(self, left: bool) -> Self {
        let x = if left { self.left } else { self.right };
        Self { left: x, right: x, ..self }
    }

    fn flatten_horizontal(self, top: bool) -> Self {
        let y = if top { self.top } else { self.bottom };
        Self { top: y, bottom: y, ..self }
    }
}

impl From<DomRect> for ViewportRect {
    fn from(rect: DomRect) -> Self {
        Self {
            left: rect.left(),
            right: rect.right(),
            top: rect.top(),
            bottom: rect.bottom(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PositionAtCoords {
    pub pos: usize,
    pub inside: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextblockDirection {
    Up,
    Down,
    Left,
    Right,
    Forward,
    Backward,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollSides {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl From<f64> for ScrollSides {
    fn from(value: f64) -> Self {
        Self { top: value, right: value, bottom: value, left: value }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollOptions {
    pub threshold: ScrollSides,
    pub margin: ScrollSides,
}

impl Default for ScrollOptions {
    fn default() -> Self {
        Self { threshold: 0.0.into(), margin: 5.0.into() }
    }
}

fn property(target: &JsValue, name: &str) -> Option<JsValue> {
    Reflect::get(target, &JsValue::from_str(name)).ok()
}

fn method(target: &JsValue, name: &str) -> Option<Function> {
    property(target, name)?.dyn_into().ok()
}

fn parent_node(node: &Node) -> Option<Node> {
    property(node.as_ref(), "assignedSlot").and_then(|slot| slot.dyn_into().ok())
        .or_else(|| node.parent_node())
        .or_else(|| property(node.as_ref(), "host")?.dyn_into().ok())
}

fn window_rect(doc: &Document) -> ViewportRect {
    if let Some(viewport) = doc.default_view().and_then(|window| window.visual_viewport()) {
        return ViewportRect { right: viewport.width(), bottom: viewport.height(), ..ViewportRect::default() };
    }
    doc.document_element().map_or_else(ViewportRect::default, |root| ViewportRect {
        right: f64::from(root.client_width()),
        bottom: f64::from(root.client_height()),
        ..ViewportRect::default()
    })
}

fn client_rect(element: &Element) -> ViewportRect {
    let rect = element.get_bounding_client_rect();
    let scale = |size, offset| if offset > 0.0 && size > 0.0 { size / offset } else { 1.0 };
    let width = property(element.as_ref(), "offsetWidth").and_then(|value| value.as_f64()).unwrap_or(0.0);
    let height = property(element.as_ref(), "offsetHeight").and_then(|value| value.as_f64()).unwrap_or(0.0);
    ViewportRect {
        left: rect.left(),
        right: rect.left() + f64::from(element.client_width()) * scale(rect.width(), width),
        top: rect.top(),
        bottom: rect.top() + f64::from(element.client_height()) * scale(rect.height(), height),
    }
}

fn scroll_offset(element: &Element, axis: &str) -> f64 {
    property(element.as_ref(), axis).and_then(|value| value.as_f64()).unwrap_or(0.0)
}

fn set_scroll_offset(element: &Element, axis: &str, value: f64) {
    let _ = Reflect::set(element.as_ref(), &JsValue::from_str(axis), &JsValue::from_f64(value));
}

pub(crate) fn scroll_rect_into_view(
    root: &HtmlElement,
    mut rect: ViewportRect,
    start_dom: &Node,
    options: ScrollOptions,
) {
    if !rect.nonzero() && rect.left == 0.0 {
        return;
    }
    let Some(doc) = root.owner_document() else { return };
    let Some(window) = doc.default_view() else { return };
    let body = doc.body();
    let mut current = Some(start_dom.clone());
    while let Some(dom) = current {
        let Some(element) = dom.dyn_ref::<Element>() else {
            current = parent_node(&dom);
            continue;
        };
        let at_top = body.as_ref().is_some_and(|body| same(&dom, body.as_ref()));
        let bounding = if at_top { window_rect(&doc) } else { client_rect(element) };
        let threshold = options.threshold;
        let margin = options.margin;
        let x = if rect.left < bounding.left + threshold.left {
            rect.left - bounding.left - margin.left
        } else if rect.right > bounding.right - threshold.right {
            rect.right - bounding.right + margin.right
        } else { 0.0 };
        let y = if rect.top < bounding.top + threshold.top {
            rect.top - bounding.top - margin.top
        } else if rect.bottom > bounding.bottom - threshold.bottom {
            if rect.bottom - rect.top > bounding.bottom - bounding.top {
                rect.top + margin.top - bounding.top
            } else {
                rect.bottom - bounding.bottom + margin.bottom
            }
        } else { 0.0 };
        if x != 0.0 || y != 0.0 {
            if at_top {
                window.scroll_by_with_x_and_y(x, y);
            } else {
                let old_x = scroll_offset(element, "scrollLeft");
                let old_y = scroll_offset(element, "scrollTop");
                if x != 0.0 {
                    set_scroll_offset(element, "scrollLeft", old_x + x);
                }
                if y != 0.0 {
                    set_scroll_offset(element, "scrollTop", old_y + y);
                }
                let delta_x = scroll_offset(element, "scrollLeft") - old_x;
                let delta_y = scroll_offset(element, "scrollTop") - old_y;
                rect.left -= delta_x;
                rect.right -= delta_x;
                rect.top -= delta_y;
                rect.bottom -= delta_y;
            }
        }

        let position = window.get_computed_style(element).ok().flatten()
            .and_then(|style| style.get_property_value("position").ok());
        current = match position.as_deref() {
            _ if at_top => break,
            Some("fixed" | "sticky") => break,
            Some("absolute") => property(element.as_ref(), "offsetParent").and_then(|parent| parent.dyn_into().ok()),
            _ => parent_node(&dom),
        };
    }
}

struct ScrollEntry {
    element: Element,
    top: f64,
    left: f64,
}

fn scroll_stack(node: &Node) -> Vec<ScrollEntry> {
    let mut stack = Vec::new();
    let mut current = Some(node.clone());
    while let Some(dom) = current {
        current = parent_node(&dom);
        if let Ok(element) = dom.dyn_into::<Element>() {
            stack.push(ScrollEntry {
                top: scroll_offset(&element, "scrollTop"),
                left: scroll_offset(&element, "scrollLeft"),
                element,
            });
        }
    }
    stack
}

fn restore_scroll_stack(stack: Vec<ScrollEntry>, delta_top: f64) {
    for ScrollEntry { element, top, left } in stack {
        if scroll_offset(&element, "scrollTop") != top + delta_top {
            set_scroll_offset(&element, "scrollTop", top + delta_top);
        }
        if scroll_offset(&element, "scrollLeft") != left {
            set_scroll_offset(&element, "scrollLeft", left);
        }
    }
}

pub(crate) struct ScrollPosition {
    reference: Option<(Element, f64)>,
    stack: Vec<ScrollEntry>,
}

pub(crate) fn store_scroll_position(root: &HtmlElement) -> ScrollPosition {
    let rect = root.get_bounding_client_rect();
    let mut reference = None;
    if let Some(doc) = root.owner_document() {
        let start = rect.top().max(0.0);
        let mut top = start + 1.0;
        while top < rect.bottom().min(window_rect(&doc).bottom) {
            let point = ViewportPoint { left: (rect.left() + rect.right()) / 2.0, top };
            if let Some(element) = element_at_point(root, &doc, point) {
                if !same(root.as_ref(), element.as_ref()) && contains(root.as_ref(), element.as_ref()) {
                    let element_top = element.get_bounding_client_rect().top();
                    if element_top >= start - 20.0 {
                        reference = Some((element, element_top));
                        break;
                    }
                }
            }
            top += 5.0;
        }
    }
    ScrollPosition { reference, stack: scroll_stack(root.as_ref()) }
}

pub(crate) fn reset_scroll_position(scroll: ScrollPosition) {
    let delta = scroll.reference.map_or(0.0, |(reference, start)| {
        let top = reference.get_bounding_client_rect().top();
        if top == 0.0 { 0.0 } else { top - start }
    });
    restore_scroll_stack(scroll.stack, delta);
}

pub(crate) fn focus_prevent_scroll(root: &HtmlElement) -> Result<(), JsValue> {
    let stack = scroll_stack(root.as_ref());
    let options = FocusOptions::new();
    options.set_prevent_scroll(true);
    let result = root.focus_with_options(&options).or_else(|_| root.focus());
    restore_scroll_stack(stack, 0.0);
    result
}

fn call_at_point(target: &JsValue, name: &str, point: ViewportPoint) -> Option<JsValue> {
    method(target, name)?.call2(target, &point.left.into(), &point.top.into()).ok()
}

fn element_at_point(root: &HtmlElement, doc: &Document, point: ViewportPoint) -> Option<Element> {
    let tree = root.get_root_node();
    let target = if method(tree.as_ref(), "elementFromPoint").is_some() { tree.as_ref() } else { doc.as_ref() };
    call_at_point(target, "elementFromPoint", point)?.dyn_into().ok()
}

fn caret_at_point(doc: &Document, point: ViewportPoint) -> Option<(Node, u32)> {
    let caret_position = call_at_point(doc.as_ref(), "caretPositionFromPoint", point).and_then(|caret| {
        let node: Node = property(&caret, "offsetNode")?.dyn_into().ok()?;
        let offset = property(&caret, "offset")?.as_f64()? as u32;
        Some((node, offset))
    });
    let (node, offset) = caret_position.or_else(|| {
        let range: Range = call_at_point(doc.as_ref(), "caretRangeFromPoint", point)?.dyn_into().ok()?;
        Some((range.start_container().ok()?, range.start_offset().ok()?))
    })?;
    let offset = offset.min(node_size(&node));
    Some((node, offset))
}

fn text_range(node: &Node, from: u32, to: u32) -> Option<Range> {
    let range = node.owner_document()?.create_range().ok()?;
    range.set_end(node, to).ok()?;
    range.set_start(node, from).ok()?;
    Some(range)
}

fn rects(list: Option<DomRectList>) -> impl Iterator<Item = ViewportRect> {
    (0..list.as_ref().map_or(0, DomRectList::length))
        .filter_map(move |index| list.as_ref()?.item(index).map(ViewportRect::from))
}

fn node_rects(node: &Node) -> impl Iterator<Item = ViewportRect> {
    let list = if node.node_type() == Node::TEXT_NODE {
        text_range(node, 0, node_size(node)).and_then(|range| range.get_client_rects())
    } else {
        node.dyn_ref::<Element>().map(Element::get_client_rects)
    };
    rects(list)
}

fn single_rect(list: Option<DomRectList>, bias: i32, fallback: impl FnOnce() -> DomRect) -> ViewportRect {
    let mut boxes = rects(list).peekable();
    if bias < 0 {
        return boxes.find(|rect| rect.nonzero()).unwrap_or_else(|| fallback().into());
    }
    let first = boxes.peek().copied();
    let mut first_nonzero = first.filter(|rect| rect.nonzero());
    let mut last = first;
    for rect in boxes {
        if first_nonzero.is_none() && rect.nonzero() {
            first_nonzero = Some(rect);
        }
        last = Some(rect);
    }
    last.filter(|rect| rect.nonzero()).or(first_nonzero).unwrap_or_else(|| fallback().into())
}

fn range_rect(range: &Range, bias: i32) -> ViewportRect {
    single_rect(range.get_client_rects(), bias, || range.get_bounding_client_rect())
}

fn element_rect(element: &Element, bias: i32) -> ViewportRect {
    single_rect(Some(element.get_client_rects()), bias, || element.get_bounding_client_rect())
}

fn find_offset_in_text(node: Node, point: ViewportPoint) -> (Node, u32) {
    let size = node_size(&node);
    if let Some(range) = text_range(&node, 0, size) {
        for index in 0..size {
            if range.set_end(&node, index + 1).is_err() || range.set_start(&node, index).is_err() {
                break;
            }
            let rect = range_rect(&range, 1);
            if rect.top != rect.bottom && rect.contains(point) {
                return (node, index + u32::from(point.left >= (rect.left + rect.right) / 2.0));
            }
        }
    }
    (node, 0)
}

fn find_offset_in_node(node: Node, point: ViewportPoint) -> (Node, u32) {
    let mut closest = None;
    let mut closest_distance = f64::INFINITY;
    let mut offset = 0;
    let mut row_bottom = point.top;
    let mut row_top = point.top;
    let mut below = None;
    let children = node.child_nodes();
    for child_index in 0..children.length() {
        let Some(child) = children.item(child_index) else { continue };
        for rect in node_rects(&child) {
            if rect.top <= row_bottom && rect.bottom >= row_top {
                row_bottom = rect.bottom.max(row_bottom);
                row_top = rect.top.min(row_top);
                let distance = (rect.left - point.left).max(point.left - rect.right).max(0.0);
                if distance < closest_distance {
                    closest_distance = distance;
                    let closest_point = if child.node_type() == Node::TEXT_NODE && distance != 0.0 {
                        ViewportPoint { left: point.left.clamp(rect.left, rect.right), ..point }
                    } else { point };
                    closest = Some((child.clone(), closest_point));
                    if child.node_type() == Node::ELEMENT_NODE && distance != 0.0 {
                        offset = child_index + u32::from(point.left >= (rect.left + rect.right) / 2.0);
                    }
                    continue;
                }
            } else if rect.top > point.top && below.is_none() && rect.left <= point.left && rect.right >= point.left {
                below = Some((child.clone(), ViewportPoint { left: point.left, top: rect.top }));
            }
            if closest.is_none() && (point.left >= rect.right && point.top >= rect.top
                || point.left >= rect.left && point.top >= rect.bottom)
            {
                offset = child_index + 1;
            }
        }
    }
    if closest.is_none() && below.is_some() {
        closest = below;
        closest_distance = 0.0;
    }
    match closest {
        Some((child, point)) if child.node_type() == Node::TEXT_NODE => find_offset_in_text(child, point),
        Some((child, point)) if closest_distance == 0.0 => find_offset_in_node(child, point),
        _ => (node, offset),
    }
}

fn position_from_element(view: &DocView, element: Element, point: ViewportPoint) -> Option<usize> {
    let (node, offset) = find_offset_in_node(element.into(), point);
    let mut bias = -1;
    if let Some(element) = node.dyn_ref::<Element>().filter(|_| !node.has_child_nodes()) {
        let rect = element.get_bounding_client_rect();
        if rect.left() != rect.right() && point.left > (rect.left() + rect.right()) / 2.0 {
            bias = 1;
        }
    }
    view.pos_at_dom(&node, offset, bias)
}

fn position_from_caret(
    root: &HtmlElement,
    view: &DocView,
    node: &Node,
    offset: u32,
    point: ViewportPoint,
) -> Option<usize> {
    let mut outside_block = None;
    let mut saw_block = false;
    let mut current = Some(node.clone());
    while let Some(dom) = current {
        if same(&dom, root.as_ref()) {
            break;
        }
        let desc = view.nearest_node_desc(&dom)?;
        let block_with_parent = desc.node.is_block() && !desc.is_root;
        if let Some(element) = desc.dom.dyn_ref::<Element>().filter(|_| block_with_parent || desc.content_dom.is_none()) {
            let rect = ViewportRect::from(element.get_bounding_client_rect());
            if rect.nonzero() {
                if block_with_parent && !matches!(desc.dom.node_name().as_str(), "TR" | "TBODY" | "THEAD" | "TFOOT") {
                    if (!saw_block && rect.left > point.left) || rect.top > point.top {
                        outside_block = Some(desc.pos_before);
                    } else if (!saw_block && rect.right < point.left) || rect.bottom < point.top {
                        outside_block = Some(desc.pos_after);
                    }
                    saw_block = true;
                }
                if desc.content_dom.is_none() && outside_block.is_none() && !desc.node.is_text() {
                    let before = if desc.node.is_block() {
                        point.top < (rect.top + rect.bottom) / 2.0
                    } else {
                        point.left < (rect.left + rect.right) / 2.0
                    };
                    return Some(if before { desc.pos_before } else { desc.pos_after });
                }
            }
        }
        current = desc.dom.parent_node();
    }
    outside_block.or_else(|| view.pos_at_dom(node, offset, -1))
}

fn find_element_at_point(element: Element, point: ViewportPoint, bounds: ViewportRect) -> Element {
    let children = element.child_nodes();
    let len = children.length();
    if len > 0 && bounds.top < bounds.bottom {
        let start = (f64::from(len) * (point.top - bounds.top) / (bounds.bottom - bounds.top))
            .floor() as u32;
        let start = start.saturating_sub(2).min(len - 1);
        for offset in 0..len {
            let index = (start + offset) % len;
            let Some(child) = children.item(index).and_then(|node| node.dyn_into::<Element>().ok()) else { continue };
            if let Some(rect) = rects(Some(child.get_client_rects())).find(|rect| rect.contains(point)) {
                return find_element_at_point(child, point, rect);
            }
        }
    }
    element
}

pub(crate) fn pos_at_coords(
    root: &HtmlElement,
    doc: &ModelNode,
    view: &DocView,
    point: ViewportPoint,
) -> Option<PositionAtCoords> {
    let owner = root.owner_document()?;
    let mut caret = caret_at_point(&owner, point);
    let mut element = match element_at_point(root, &owner, point)
        .filter(|elt| contains(root.as_ref(), elt.as_ref()))
    {
        Some(hit) => hit,
        None => {
            let bounds = ViewportRect::from(root.get_bounding_client_rect());
            if !bounds.contains(point) {
                return None;
            }
            find_element_at_point(root.clone().into(), point, bounds)
        }
    };
    let browser = browser_flags(root);
    if browser.safari && caret.is_some() {
        let mut current = Some(Node::from(element.clone()));
        while let Some(node) = current {
            if node.dyn_ref::<HtmlElement>().is_some_and(HtmlElement::draggable) {
                caret = None;
                break;
            }
            current = parent_node(&node);
        }
    }
    if let Some(parent) = element.parent_element() {
        if parent.tag_name().eq_ignore_ascii_case("li") && point.left < element.get_bounding_client_rect().left() {
            element = parent;
        }
    }

    let pos = caret.and_then(|(node, mut offset)| {
        let children = node.child_nodes();
        let is_element = node.node_type() == Node::ELEMENT_NODE;
        let child_element = |index| children.item(index)?.dyn_into::<Element>().ok();
        if browser.gecko && is_element {
            offset = offset.min(children.length());
            if let Some(image) = child_element(offset).filter(|next| next.tag_name() == "IMG") {
                let rect = image.get_bounding_client_rect();
                if rect.right() <= point.left && rect.bottom() > point.top {
                    offset += 1;
                }
            }
        }
        if browser.webkit && offset > 0 && is_element {
            if let Some(prev) = child_element(offset - 1).filter(|prev| uneditable(prev.as_ref())) {
                if prev.get_bounding_client_rect().top() >= point.top {
                    offset -= 1;
                }
            }
        }
        if same(&node, root.as_ref()) && offset + 1 == children.length()
            && child_element(offset).is_some_and(|last| point.top > last.get_bounding_client_rect().bottom())
        {
            return Some(doc.content().size());
        }
        if offset == 0 || !is_element
            || children.item(offset - 1).is_none_or(|prev| prev.node_name() != "BR")
        {
            position_from_caret(root, view, &node, offset, point)
        } else { None }
    }).or_else(|| position_from_element(view, element.clone(), point))?;
    let inside = view.nearest_node_desc(element.as_ref())
        .filter(|desc| !desc.is_root)
        .map(|desc| desc.pos_before);
    Some(PositionAtCoords { pos, inside })
}

pub(crate) fn coords_at_pos(
    root: &HtmlElement,
    doc: &ModelNode,
    view: &DocView,
    pos: usize,
    side: i32,
) -> Option<ViewportRect> {
    let (node, offset) = view.dom_at_pos(pos, if side < 0 { -1 } else { 1 })?;
    let flags = browser_flags(root);
    let supports_empty = flags.webkit || flags.gecko;
    if node.node_type() == Node::TEXT_NODE {
        let text = node.node_value().unwrap_or_default();
        let size = node_size(&node);
        let bidi = text.chars().any(|ch| matches!(ch, '\u{590}'..='\u{5f4}' | '\u{600}'..='\u{6ff}' | '\u{700}'..='\u{8ac}'));
        if supports_empty && (bidi || (if side < 0 { offset == 0 } else { offset == size })) {
            let rect = range_rect(&text_range(&node, offset, offset)?, side);
            if flags.gecko && offset != 0 && offset < size
                && text.encode_utf16().nth((offset - 1) as usize)
                    .and_then(|unit| char::from_u32(u32::from(unit))).is_some_and(char::is_whitespace)
            {
                let before = range_rect(&text_range(&node, offset - 1, offset - 1)?, -1);
                if before.top == rect.top {
                    let after = range_rect(&text_range(&node, offset, offset + 1)?, -1);
                    if after.top != rect.top {
                        return Some(after.flatten_vertical(after.left < before.left));
                    }
                }
            }
            return Some(rect);
        }
        let (from, to, bias) = if (side < 0 && offset != 0) || offset == size {
            (offset.saturating_sub(1), offset, 1)
        } else {
            (offset, (offset + 1).min(size), -1)
        };
        return Some(range_rect(&text_range(&node, from, to)?, bias).flatten_vertical(bias < 0));
    }
    let atom = view.nearest_node_desc(&node).filter(|desc| {
        !desc.is_root && same(&node, &desc.dom) && desc.content_dom.is_none() && pos > desc.pos_before
    });
    let resolved = doc.resolve(atom.as_ref().map_or(pos, |desc| desc.pos_before)).ok()?;
    let children = node.child_nodes();
    let size = node_size(&node);
    let before = (atom.is_none() && offset != 0 && (side < 0 || offset == size))
        .then(|| children.item(offset - 1)).flatten();
    let after = (atom.is_none() && offset < size).then(|| children.item(offset)).flatten();
    if !resolved.parent().inline_content() {
        if let Some(element) = before.and_then(|child| child.dyn_into::<Element>().ok()) {
            return Some(ViewportRect::from(element.get_bounding_client_rect()).flatten_horizontal(false));
        }
        if let Some(element) = after.and_then(|child| child.dyn_into::<Element>().ok()) {
            return Some(ViewportRect::from(element.get_bounding_client_rect()).flatten_horizontal(true));
        }
        let element = node.dyn_ref::<Element>()?;
        return Some(ViewportRect::from(element.get_bounding_client_rect()).flatten_horizontal(side >= 0));
    }

    if let Some(before) = before {
        if before.node_type() == Node::TEXT_NODE {
            let size = node_size(&before);
            let range = text_range(&before, size.saturating_sub(u32::from(!supports_empty)), size)?;
            return Some(range_rect(&range, 1).flatten_vertical(false));
        }
        if let Some(element) = before.dyn_ref::<Element>() {
            if before.node_name() != "BR" || before.next_sibling().is_none() {
                return Some(element_rect(element, 1).flatten_vertical(false));
            }
        }
    }
    let mut current = after;
    while let Some(after) = current {
        if view.ignore_for_coords(&after) {
            current = after.next_sibling();
            continue;
        }
        if after.node_type() == Node::TEXT_NODE {
            let range = text_range(&after, 0, u32::from(!supports_empty).min(node_size(&after)))?;
            return Some(range_rect(&range, -1).flatten_vertical(true));
        }
        if let Some(element) = after.dyn_ref::<Element>() {
            return Some(element_rect(element, -1).flatten_vertical(true));
        }
        break;
    }
    Some(element_rect(node.dyn_ref::<Element>()?, -side).flatten_vertical(side >= 0))
}

fn end_of_textblock_vertical(
    root: &HtmlElement,
    doc: &ModelNode,
    view: &DocView,
    pos: usize,
    up: bool,
) -> Option<bool> {
    let (mut dom, _) = view.dom_at_pos(pos, if up { -1 } else { 1 })?;
    while let Some(desc) = view.nearest_node_desc(&dom) {
        if desc.node.is_block() {
            dom = desc.content_dom.unwrap_or(desc.dom);
            break;
        }
        let Some(parent) = desc.dom.parent_node() else { break };
        dom = parent;
    }
    let caret = coords_at_pos(root, doc, view, pos, 1)?;
    let children = dom.child_nodes();
    for bounds in (0..children.length()).filter_map(|index| children.item(index)).flat_map(|child| node_rects(&child)) {
        if bounds.bottom > bounds.top + 1.0 && if up {
            caret.top - bounds.top > (bounds.bottom - caret.top) * 2.0
        } else {
            bounds.bottom - caret.bottom > (caret.bottom - bounds.top) * 2.0
        } {
            return Some(false);
        }
    }
    Some(true)
}

fn end_of_textblock_horizontal(
    root: &HtmlElement,
    state: &EditorState,
    view: &DocView,
    direction: TextblockDirection,
) -> Option<bool> {
    let head = state.doc.resolve(state.selection.head(&state.doc)).ok()?;
    let parent = head.parent();
    if !parent.is_textblock() {
        return Some(false);
    }
    let at_start = head.parent_offset == 0;
    let at_end = head.parent_offset == parent.content().size();
    let left = matches!(direction, TextblockDirection::Left | TextblockDirection::Backward);
    let fallback = if left { at_start } else { at_end };
    let Some(selection) = native_selection(root) else { return Some(at_start || at_end) };
    let bidi = parent.text_content().chars().any(|ch| ('\u{590}'..='\u{8ac}').contains(&ch));
    let Some(modify) = method(selection.as_ref(), "modify").filter(|_| bidi) else { return Some(fallback) };
    let previous_active = active_element(root).filter(|element| !same(root.as_ref(), element.as_ref()));
    if previous_active.is_some() || selection.focus_node().is_none() {
        focus_prevent_scroll(root).ok()?;
        write_selection(root, state, view, WriteSelectionOptions {
            editable: true, force: false, cursor_wrapper: None,
        }).ok()?;
    }
    let focus = selection.focus_node();
    let focus_offset = selection.focus_offset();
    let anchor = selection.anchor_node();
    let anchor_offset = selection.anchor_offset();
    let bidi_level = property(selection.as_ref(), "caretBidiLevel");
    let dir = match direction {
        TextblockDirection::Left => "left",
        TextblockDirection::Right => "right",
        TextblockDirection::Forward => "forward",
        TextblockDirection::Backward => "backward",
        _ => unreachable!("vertical directions are handled separately"),
    };
    let modified = modify.call3(selection.as_ref(), &"move".into(), &dir.into(), &"character".into()).is_ok();
    let result = if modified {
        let parent_dom = if head.depth > 0 { view.node_dom(head.before(head.depth)) } else { Some(root.clone().into()) };
        let next_focus = selection.focus_node();
        let next_offset = selection.focus_offset();
        let unchanged = match (&focus, &next_focus) {
            (Some(a), Some(b)) => same(a, b) && focus_offset == next_offset,
            (None, None) => focus_offset == next_offset,
            _ => false,
        };
        unchanged || parent_dom.as_ref().zip(next_focus.as_ref()).is_some_and(|(parent, next)| !contains(parent, next))
    } else { fallback };
    if selection.collapse_with_offset(anchor.as_ref(), anchor_offset).is_ok() {
        if let Some(focus) = focus.as_ref().filter(|focus| {
            !anchor.as_ref().is_some_and(|anchor| same(anchor, focus) && anchor_offset == focus_offset)
        }) {
            let _ = selection.extend_with_offset(focus, focus_offset);
        }
    }
    if let Some(level) = bidi_level.filter(|value| !value.is_null() && !value.is_undefined()) {
        let _ = Reflect::set(selection.as_ref(), &"caretBidiLevel".into(), &level);
    }
    if let Some(active) = previous_active.and_then(|element| element.dyn_into::<HtmlElement>().ok()) {
        let _ = focus_prevent_scroll(&active);
    }
    Some(result)
}

pub(crate) fn end_of_textblock(
    root: &HtmlElement,
    state: &EditorState,
    view: &DocView,
    direction: TextblockDirection,
) -> bool {
    let doc = &state.doc;
    match direction {
        TextblockDirection::Up => end_of_textblock_vertical(root, doc, view, state.selection.from(), true),
        TextblockDirection::Down => end_of_textblock_vertical(root, doc, view, state.selection.to(doc), false),
        _ => end_of_textblock_horizontal(root, state, view, direction),
    }.unwrap_or(false)
}
