//! Synchronization between editor selections and native DOM selections.

use js_sys::{Function, Reflect};
use prosemirror_model::ResolvedPos;
use prosemirror_state::{EditorState, Selection};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Element, HtmlElement, Node, Selection as DomSelection};

use super::viewdesc::DocView;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionOrigin {
    #[default]
    Unspecified,
    Pointer,
    Key,
}

pub(crate) struct WriteSelectionOptions<'a> {
    pub editable: bool,
    pub force: bool,
    pub cursor_wrapper: Option<&'a Node>,
}

pub(crate) type CreateBetween<'a> = dyn Fn(&ResolvedPos, &ResolvedPos) -> Option<Selection> + 'a;

#[derive(Clone, Copy)]
pub(crate) struct BrowserFlags {
    pub gecko: bool,
    pub safari: bool,
    pub webkit: bool,
    old_chrome: bool,
    ie: bool,
}

pub(crate) fn browser_flags(root: &HtmlElement) -> BrowserFlags {
    let ua = root.owner_document().and_then(|doc| doc.default_view())
        .and_then(|win| win.navigator().user_agent().ok())
        .unwrap_or_default();
    let chrome = ua.split_once("Chrome/").and_then(|(_, tail)| tail.split('.').next())
        .and_then(|major| major.parse::<u32>().ok());
    BrowserFlags {
        gecko: ua.contains("Firefox/") && ua.contains("Gecko/"),
        safari: ua.contains("Safari/") && chrome.is_none() && !ua.contains("Chromium/"),
        webkit: ua.contains("AppleWebKit/"),
        old_chrome: chrome.is_some_and(|major| major < 63),
        ie: ua.contains("MSIE ") || ua.contains("Trident/"),
    }
}

pub(crate) fn native_selection(root: &HtmlElement) -> Option<DomSelection> {
    let tree = root.get_root_node();
    let getter = Reflect::get(tree.as_ref(), &JsValue::from_str("getSelection")).ok()
        .and_then(|getter| getter.dyn_into::<Function>().ok());
    if let Some(getter) = getter {
        return getter.call0(tree.as_ref()).ok()?.dyn_into().ok();
    }
    root.owner_document()?.get_selection().ok().flatten()
}

pub(crate) fn active_element(root: &HtmlElement) -> Option<Element> {
    let tree = root.get_root_node();
    Reflect::get(tree.as_ref(), &JsValue::from_str("activeElement"))
        .ok().and_then(|element| element.dyn_into().ok())
        .or_else(|| root.owner_document()?.active_element())
}

pub(crate) fn has_focus(root: &HtmlElement) -> bool {
    let Some(active) = active_element(root) else { return false };
    let root_node: &Node = root.as_ref();
    if same(root_node, active.as_ref()) {
        return true;
    }
    if !browser_flags(root).ie || !contains(root_node, active.as_ref()) {
        return false;
    }
    let mut current: Node = active.into();
    while !same(root_node, &current) {
        if uneditable(&current) {
            return false;
        }
        let Some(parent) = current.parent_node() else { return false };
        current = parent;
    }
    true
}

pub(crate) fn has_selection(root: &HtmlElement, editable: bool) -> bool {
    native_selection(root).is_some_and(|selection| {
        let root_node: &Node = root.as_ref();
        selection.anchor_node().is_some_and(|node| contains(root_node, &node))
            && (editable || selection.focus_node().is_some_and(|node| contains(root_node, &node)))
    })
}

pub(super) fn editor_owns_selection(root: &HtmlElement, editable: bool) -> bool {
    if editable {
        has_focus(root)
    } else {
        has_selection(root, false)
            && active_element(root).is_some_and(|active| contains(active.as_ref(), root.as_ref()))
    }
}

pub(crate) fn read_selection(
    root: &HtmlElement,
    state: &EditorState,
    doc_view: &DocView,
    origin: SelectionOrigin,
    create_between: Option<&CreateBetween<'_>>,
) -> Option<Selection> {
    let sel = native_selection(root)?;
    let (focus_node, anchor_node) = (sel.focus_node()?, sel.anchor_node()?);
    if !contains(root.as_ref(), &focus_node) || !contains(root.as_ref(), &anchor_node) {
        return None;
    }
    let (focus_offset, anchor_offset) = (sel.focus_offset(), sel.anchor_offset());
    let head = doc_view.pos_at_dom(&focus_node, focus_offset, 1)?;
    let mut resolved_head = state.doc.resolve(head).ok()?;
    let in_widget = doc_view.is_widget(&focus_node);

    let anchor = if equivalent_position(doc_view, &focus_node, focus_offset, &anchor_node, anchor_offset) {
        if let Some(desc) = doc_view.nearest_node_desc(&focus_node) {
            if desc.node.is_atom() && desc.node.is_selectable() && !desc.is_root
                && !(desc.node.is_inline() && on_edge(&focus_node, focus_offset, &desc.dom))
            {
                return Some(Selection::node(desc.pos_before));
            }
            if browser_flags(root).gecko && origin == SelectionOrigin::Key
                && head > state.selection.to(&state.doc)
                && focus_node.node_type() == Node::ELEMENT_NODE
                && focus_offset == node_size(&focus_node)
                && desc.pos_at_start > state.selection.to(&state.doc)
            {
                let mut scan = focus_node.clone();
                let window = root.owner_document()?.default_view()?;
                loop {
                    let style = scan.dyn_ref::<Element>()
                        .and_then(|elt| window.get_computed_style(elt).ok().flatten());
                    if style.and_then(|style| style.get_property_value("overflow").ok())
                        .is_some_and(|overflow| overflow != "visible")
                    {
                        let start = state.doc.resolve(desc.pos_at_start).ok()?;
                        if let Some(selection) = Selection::find_from(&start, 1, false) {
                            return Some(selection);
                        }
                        break;
                    }
                    if same(&scan, &desc.dom) || scan.next_sibling().is_some() {
                        break;
                    }
                    let Some(parent) = scan.parent_node().filter(|node| node.node_type() == Node::ELEMENT_NODE)
                    else { break };
                    scan = parent;
                }
            }
        }
        head
    } else if sel.range_count() > 1 {
        let (mut min, mut max) = (head, head);
        for index in 0..sel.range_count() {
            let range = sel.get_range_at(index).ok()?;
            let from = doc_view.pos_at_dom(&range.start_container().ok()?, range.start_offset().ok()?, 1)?;
            let to = doc_view.pos_at_dom(&range.end_container().ok()?, range.end_offset().ok()?, -1)?;
            min = min.min(from);
            max = max.max(to);
        }
        if max == state.selection.anchor() {
            resolved_head = state.doc.resolve(min).ok()?;
            max
        } else {
            resolved_head = state.doc.resolve(max).ok()?;
            min
        }
    } else {
        doc_view.pos_at_dom(&anchor_node, anchor_offset, 1)?
    };
    let resolved_anchor = state.doc.resolve(anchor).ok()?;
    let bias = if origin == SelectionOrigin::Pointer
        || (state.selection.head(&state.doc) < resolved_head.pos && !in_widget)
    {
        1
    } else {
        -1
    };
    create_between.and_then(|create| create(&resolved_anchor, &resolved_head))
        .or_else(|| Some(Selection::between(&resolved_anchor, &resolved_head, bias)))
}

pub(crate) fn sync_node_selection(
    doc_view: &DocView,
    state: &EditorState,
    previous: &mut Option<HtmlElement>,
) -> Result<(), JsValue> {
    let next = if let Selection::Node { anchor } = state.selection {
        doc_view.node_dom(anchor).and_then(|dom| dom.dyn_into::<HtmlElement>().ok())
    } else {
        None
    };
    if matches!((&*previous, &next), (Some(old), Some(new)) if same(old.as_ref(), new.as_ref())) {
        return Ok(());
    }
    if let Some(old) = previous.take() {
        if old.parent_node().is_some() && !doc_view.set_node_selected(old.as_ref(), false)? {
            old.class_list().remove_1("ProseMirror-selectednode")?;
            if doc_view.nearest_node_desc(old.as_ref())
                .is_some_and(|desc| desc.content_dom.is_some() || !desc.node.node_type().is_some_and(|ty| ty.spec.draggable))
            {
                old.remove_attribute("draggable")?;
            }
        }
    }
    if let Some(next) = next {
        if !doc_view.set_node_selected(next.as_ref(), true)? {
            next.class_list().add_1("ProseMirror-selectednode")?;
            if doc_view.nearest_node_desc(next.as_ref())
                .is_some_and(|desc| desc.content_dom.is_some() || !desc.node.node_type().is_some_and(|ty| ty.spec.draggable))
            {
                next.set_draggable(true);
            }
        }
        *previous = Some(next);
    }
    Ok(())
}

pub(crate) fn write_selection(
    root: &HtmlElement,
    state: &EditorState,
    doc_view: &DocView,
    options: WriteSelectionOptions<'_>,
) -> Result<(), JsValue> {
    if !editor_owns_selection(root, options.editable) {
        return Ok(());
    }
    let Some(native) = native_selection(root) else { return Ok(()) };
    if let Some(wrapper) = options.cursor_wrapper {
        return if wrapper.node_name() == "IMG" {
            native.collapse_with_offset(wrapper.parent_node().as_ref(), index_in_parent(wrapper) + 1)
        } else {
            native.collapse_with_offset(Some(wrapper), 0)
        };
    }

    let state_sel = &state.selection;
    let anchor = state_sel.anchor();
    let head = state_sel.head(&state.doc);
    let flags = browser_flags(root);
    let fix_editable = (flags.safari || flags.old_chrome) && !matches!(state_sel, Selection::Text { .. });
    let mut temporary = Vec::new();
    if fix_editable {
        let from = state_sel.from();
        if state.doc.resolve(from).is_ok_and(|pos| !pos.parent().inline_content()) {
            if let Some(restore) = temporarily_editable(doc_view, from, flags.safari) {
                temporary.push(restore);
            }
            if !state_sel.empty_in(&state.doc) {
                if let Some(restore) = temporarily_editable(doc_view, state_sel.to(&state.doc), flags.safari) {
                    temporary.push(restore);
                }
            }
        }
    }
    let root_node: &Node = root.as_ref();
    if !doc_view.set_custom_selection(anchor, head, &root_node.get_root_node())? {
        set_native_selection(root, doc_view, &native, anchor, head, options.force, flags)?;
    }
    drop(temporary);

    if state_sel.visible() {
        root.class_list().remove_1("ProseMirror-hideselection")?;
    } else {
        root.class_list().add_1("ProseMirror-hideselection")?;
    }
    Ok(())
}

fn set_native_selection(
    root: &HtmlElement,
    doc_view: &DocView,
    native: &DomSelection,
    anchor: usize,
    head: usize,
    mut force: bool,
    flags: BrowserFlags,
) -> Result<(), JsValue> {
    let Some(mut a) = doc_view.dom_at_pos(anchor, if anchor == 0 { 1 } else { -1 }) else { return Ok(()) };
    let Some(mut h) = doc_view.dom_at_pos(head, if head == 0 { 1 } else { -1 }) else { return Ok(()) };

    let mut br_kludge = false;
    if (flags.gecko || flags.safari) && anchor == head {
        if a.0.node_type() == Node::TEXT_NODE {
            let text = a.0.node_value().unwrap_or_default();
            br_kludge = a.1 > 0 && text.encode_utf16().nth((a.1 - 1) as usize) == Some(10);
            if br_kludge && a.1 == node_size(&a.0) {
                let mut scan = a.0.clone();
                loop {
                    if let Some(next) = scan.next_sibling() {
                        if next.node_name() == "BR" {
                            if let Some(parent) = next.parent_node() {
                                a = (parent, index_in_parent(&next) + 1);
                                h = a.clone();
                            }
                        }
                        break;
                    }
                    if block_boundary(doc_view, &scan) { break }
                    let Some(parent) = scan.parent_node() else { break };
                    scan = parent;
                }
            }
        } else if a.1 > 0 {
            br_kludge = a.0.child_nodes().item(a.1 - 1)
                .is_some_and(|prev| prev.node_name() == "BR" || uneditable(&prev));
        }
    }
    if flags.gecko {
        if let Some(focus) = native.focus_node().filter(|node| !same(node, &h.0)) {
            force |= focus.child_nodes().item(native.focus_offset()).is_some_and(|after| uneditable(&after));
        }
    }
    if !(force || br_kludge && flags.safari)
        && native.anchor_node().is_some_and(|node| equivalent_position(doc_view, &a.0, a.1, &node, native.anchor_offset()))
        && native.focus_node().is_some_and(|node| equivalent_position(doc_view, &h.0, h.1, &node, native.focus_offset()))
    {
        return Ok(());
    }

    if !(br_kludge && flags.gecko)
        && native.collapse_with_offset(Some(&a.0), a.1).is_ok()
        && (anchor == head || native.extend_with_offset(&h.0, h.1).is_ok())
    {
        return Ok(());
    }
    if anchor > head {
        std::mem::swap(&mut a, &mut h);
    }
    let document = root.owner_document().ok_or_else(|| JsValue::from_str("Missing owner document"))?;
    let range = document.create_range()?;
    range.set_end(&h.0, h.1)?;
    range.set_start(&a.0, a.1)?;
    native.remove_all_ranges()?;
    native.add_range(&range)
}

struct SavedEditable {
    element: HtmlElement,
    draggable: bool,
}

impl Drop for SavedEditable {
    fn drop(&mut self) {
        self.element.set_content_editable("false");
        if self.draggable {
            self.element.set_draggable(true);
        }
    }
}

fn temporarily_editable(doc_view: &DocView, pos: usize, safari: bool) -> Option<SavedEditable> {
    let (dom, offset) = doc_view.dom_at_pos(pos, 0)?;
    let after = dom.child_nodes().item(offset);
    let before = offset.checked_sub(1).and_then(|offset| dom.child_nodes().item(offset));
    let target = if safari && after.as_ref().is_some_and(uneditable) {
        after
    } else if after.as_ref().is_none_or(uneditable) && before.as_ref().is_none_or(uneditable) {
        after.or(before)
    } else {
        None
    };
    let element: HtmlElement = target?.dyn_into().ok()?;
    let draggable = safari && element.draggable();
    element.set_content_editable("true");
    if draggable {
        element.set_draggable(false);
    }
    Some(SavedEditable { element, draggable })
}

pub(crate) fn same(a: &Node, b: &Node) -> bool {
    a.is_same_node(Some(b))
}

pub(crate) fn node_size(node: &Node) -> u32 {
    if node.node_type() == Node::TEXT_NODE {
        Reflect::get(node.as_ref(), &JsValue::from_str("length")).ok()
            .and_then(|value| value.as_f64()).unwrap_or_default() as u32
    } else {
        node.child_nodes().length()
    }
}

fn index_in_parent(node: &Node) -> u32 {
    std::iter::successors(node.previous_sibling(), Node::previous_sibling).count() as u32
}

pub(crate) fn contains(parent: &Node, child: &Node) -> bool {
    if child.node_type() == Node::ELEMENT_NODE {
        parent.contains(Some(child))
    } else {
        child.parent_node().is_some_and(|parent_node| parent.contains(Some(&parent_node)))
    }
}

pub(crate) fn uneditable(node: &Node) -> bool {
    node.dyn_ref::<HtmlElement>().is_some_and(|elt| elt.content_editable() == "false")
}

fn on_edge(node: &Node, offset: u32, target: &Node) -> bool {
    let (mut start, mut end) = (offset == 0, offset == node_size(node));
    let mut current = node.clone();
    while start || end {
        if same(&current, target) { return true }
        let index = index_in_parent(&current);
        let Some(parent) = current.parent_node() else { return false };
        start &= index == 0;
        end &= index + 1 == node_size(&parent);
        current = parent;
    }
    false
}

fn block_boundary(view: &DocView, node: &Node) -> bool {
    view.nearest_node_desc(node).is_some_and(|desc| {
        desc.node.is_block() && (same(&desc.dom, node)
            || desc.content_dom.as_ref().is_some_and(|dom| same(dom, node)))
    })
}

pub(crate) fn equivalent_position(view: &DocView, a: &Node, off: u32, b: &Node, b_off: u32) -> bool {
    [-1, 1].into_iter().any(|dir| scan_for(view, a.clone(), off, b, b_off, dir))
}

fn scan_for(view: &DocView, mut node: Node, mut offset: u32, target: &Node, target_off: u32, dir: i32) -> bool {
    loop {
        if same(&node, target) && offset == target_off {
            return true;
        }
        if offset == if dir < 0 { 0 } else { node_size(&node) } {
            let Some(parent) = node.parent_node() else { return false };
            if parent.node_type() != Node::ELEMENT_NODE
                || block_boundary(view, &node)
                || matches!(node.node_name().as_str(), "IMG" | "BR" | "INPUT" | "TEXTAREA" | "HR")
                || uneditable(&node)
            {
                return false;
            }
            offset = index_in_parent(&node) + u32::from(dir > 0);
            node = parent;
        } else if node.node_type() == Node::ELEMENT_NODE {
            let index = if dir < 0 { offset.saturating_sub(1) } else { offset };
            let Some(child) = node.child_nodes().item(index) else { return false };
            if uneditable(&child) {
                if view.is_widget(&child) {
                    offset = if dir < 0 { offset - 1 } else { offset + 1 };
                } else {
                    return false;
                }
            } else {
                offset = if dir < 0 { node_size(&child) } else { 0 };
                node = child;
            }
        } else {
            return false;
        }
    }
}
