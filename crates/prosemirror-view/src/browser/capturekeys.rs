//! Keyboard behavior around atoms, block boundaries, and non-editable DOM.

use prosemirror_state::{EditorState, Selection};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Element, KeyboardEvent, Node};

use super::{error, input::is_mac, BrowserView, TextblockDirection};

pub(super) fn capture_key_down(view: &BrowserView, event: &KeyboardEvent) -> Result<bool, JsValue> {
    let key = event.key();
    let code = event.key_code();
    let mac = is_mac(view);
    let control = event.ctrl_key() && !event.alt_key() && !event.meta_key() && !event.shift_key();
    let action = if mac && control {
        match code {
            72 => "Backspace",
            68 => "Delete",
            66 => "Backward",
            70 => "Forward",
            80 => "ArrowUp",
            78 => "ArrowDown",
            _ => &key,
        }
    } else if key.is_empty() {
        match code {
            8 => "Backspace",
            46 => "Delete",
            13 => "Enter",
            27 => "Escape",
            37 => "ArrowLeft",
            39 => "ArrowRight",
            38 => "ArrowUp",
            40 => "ArrowDown",
            _ => &key,
        }
    } else {
        &key
    };
    let direction = match action {
        "Backspace" => -1,
        "Delete" if !event.shift_key() => 1,
        "Enter" | "Escape" => return Ok(true),
        "ArrowLeft" | "ArrowRight" | "Backward" | "Forward" => {
            let direction = match action {
                "Backward" => -1,
                "Forward" => 1,
                _ if (action == "ArrowLeft") != rtl(view) => -1,
                _ => 1,
            };
            if select_horizontally(view, direction, event, mac)? {
                return Ok(true);
            }
            skip_ignored_nodes(view, direction)?;
            return Ok(false);
        }
        "ArrowUp" | "ArrowDown" => {
            let direction = if action == "ArrowUp" { -1 } else { 1 };
            if select_vertically(view, direction, event, mac)? {
                return Ok(true);
            }
            skip_ignored_nodes(view, direction)?;
            return Ok(false);
        }
        _ => {
            return Ok(!event.alt_key() && !event.shift_key()
                && (if mac { event.meta_key() && !event.ctrl_key() } else { control })
                && (matches!(key.to_ascii_lowercase().as_str(), "b" | "i" | "y" | "z")
                    || matches!(code, 66 | 73 | 89 | 90)));
        }
    };
    if stop_native_horizontal_delete(view, direction)? {
        return Ok(true);
    }
    skip_ignored_nodes(view, direction)?;
    Ok(false)
}

fn rtl(view: &BrowserView) -> bool {
    let state = view.state();
    let Ok(resolved) = state.doc.resolve(state.selection.from()) else { return false };
    let node = (1..=resolved.depth).rev().find(|&depth| resolved.node(depth).is_block())
        .and_then(|depth| view.node_dom(resolved.before(depth)))
        .and_then(|node| node.dyn_into::<Element>().ok())
        .unwrap_or_else(|| view.dom().into());
    node.owner_document().and_then(|doc| doc.default_view())
        .and_then(|window| window.get_computed_style(&node).ok().flatten())
        .and_then(|style| style.get_property_value("direction").ok())
        .is_some_and(|direction| direction == "rtl")
}

fn move_selection_block(state: &EditorState, direction: i32) -> Option<Selection> {
    let side = if direction > 0 { state.selection.to(&state.doc) } else { state.selection.from() };
    let mut start = state.doc.resolve(side).ok()?;
    if start.parent().inline_content() {
        if start.depth == 0 {
            return None;
        }
        let boundary = if direction > 0 { start.after(start.depth) } else { start.before(start.depth) };
        start = state.doc.resolve(boundary).ok()?;
    }
    Selection::find_from(&start, direction, false)
}

fn apply(view: &BrowserView, state: &EditorState, selection: Selection) -> Result<bool, JsValue> {
    let mut transaction = state.tr();
    transaction.set_selection(selection).scroll_into_view();
    view.dispatch(transaction)?;
    Ok(true)
}

fn select_horizontally(
    view: &BrowserView, direction: i32, event: &KeyboardEvent, mac: bool,
) -> Result<bool, JsValue> {
    let state = view.state();
    let Selection::Text { anchor, head } = state.selection else {
        if state.selection.selected_node(&state.doc).is_some_and(|node| node.is_inline()) {
            let pos = if direction > 0 { state.selection.to(&state.doc) } else { state.selection.from() };
            return apply(view, &state, Selection::cursor(pos));
        }
        return move_selection_block(&state, direction)
            .map_or(Ok(false), |next| apply(view, &state, next));
    };
    let resolved = state.doc.resolve(head).map_err(error)?;
    let neighbor = (resolved.text_offset() == 0).then(|| {
        if direction < 0 { resolved.node_before() } else { resolved.node_after() }
    }).flatten();
    if event.shift_key() {
        return neighbor.filter(|node| !node.is_text() && node.is_leaf()).map_or(Ok(false), |node| {
            let next = if direction < 0 { head - node.node_size() } else { head + node.node_size() };
            apply(view, &state, Selection::text(anchor, next))
        });
    }
    if anchor != head {
        return Ok(false);
    }
    if view.end_of_textblock(logical_direction(direction), None) {
        return if let Some(next @ Selection::Node { .. }) = move_selection_block(&state, direction) {
            apply(view, &state, next)
        } else {
            Ok(false)
        };
    }
    if mac && event.meta_key() {
        return Ok(false);
    }
    let Some(node) = neighbor.filter(|node| !node.is_text()) else { return Ok(false) };
    let pos = if direction < 0 { head - node.node_size() } else { head };
    let closed = view.node_dom(pos).is_some_and(|dom| uneditable(&dom));
    if !node.is_atom() && !closed {
        return Ok(false);
    }
    let selection = if node.is_selectable() {
        Selection::node(pos)
    } else {
        Selection::cursor(if direction < 0 { pos } else { pos + node.node_size() })
    };
    apply(view, &state, selection)
}

fn select_vertically(
    view: &BrowserView, direction: i32, event: &KeyboardEvent, mac: bool,
) -> Result<bool, JsValue> {
    let state = view.state();
    if event.shift_key() || (mac && event.meta_key())
        || (matches!(state.selection, Selection::Text { .. }) && !state.selection.empty())
    {
        return Ok(false);
    }
    let from = state.doc.resolve(state.selection.from()).map_err(error)?;
    let dir = if direction < 0 { TextblockDirection::Up } else { TextblockDirection::Down };
    if !from.parent().inline_content() || view.end_of_textblock(dir, None) {
        if let Some(next @ Selection::Node { .. }) = move_selection_block(&state, direction) {
            return apply(view, &state, next);
        }
    }
    if from.parent().inline_content() {
        return Ok(false);
    }
    let side = if direction < 0 { from } else { state.doc.resolve(state.selection.to(&state.doc)).map_err(error)? };
    let beyond = if matches!(state.selection, Selection::All) {
        Some(Selection::near(&side, direction))
    } else {
        Selection::find_from(&side, direction, false)
    };
    beyond.map_or(Ok(false), |beyond| apply(view, &state, beyond))
}

fn stop_native_horizontal_delete(view: &BrowserView, direction: i32) -> Result<bool, JsValue> {
    let state = view.state();
    let Selection::Text { anchor, head } = state.selection else { return Ok(true) };
    let head = state.doc.resolve(head).map_err(error)?;
    if !head.same_parent(&state.doc.resolve(anchor).map_err(error)?) {
        return Ok(true);
    }
    if !state.selection.empty() {
        return Ok(false);
    }
    if view.end_of_textblock(logical_direction(direction), None) {
        return Ok(true);
    }
    if head.text_offset() == 0 {
        let next = if direction < 0 { head.node_before() } else { head.node_after() };
        if let Some(next) = next.filter(|next| !next.is_text()) {
            let (from, to) = if direction < 0 { (head.pos - next.node_size(), head.pos) }
                else { (head.pos, head.pos + next.node_size()) };
            let mut transaction = state.tr();
            transaction.delete(from, to);
            view.dispatch(transaction)?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn logical_direction(direction: i32) -> TextblockDirection {
    if direction > 0 { TextblockDirection::Forward } else { TextblockDirection::Backward }
}

fn skip_ignored_nodes(view: &BrowserView, direction: i32) -> Result<(), JsValue> {
    let Some(selection) = view.dom_selection() else { return Ok(()) };
    let Some(mut node) = selection.focus_node() else { return Ok(()) };
    let root = view.dom();
    let mut offset = selection.focus_offset();
    let mut moved = None;
    loop {
        let size = node_size(&node);
        let step = if direction < 0 { offset.checked_sub(1) } else { (offset < size).then_some(offset) };
        if let Some(step) = step {
            let Some(child) = node.child_nodes().item(step) else { break };
            if ignorable(view, &child, direction) {
                offset = if direction < 0 { step } else { step + 1 };
                moved = Some((node.clone(), offset));
            } else if direction < 0 && child.node_type() == Node::TEXT_NODE {
                node = child;
                offset = node_size(&node);
            } else {
                break;
            }
        } else {
            let block = view.with_doc_view(|view| view.nearest_node_desc(&node)
                .is_some_and(|desc| desc.node.is_block() && desc.dom.is_same_node(Some(&node)))).unwrap_or(false);
            if block || node.is_same_node(Some(&root)) { break }
            let Some(parent) = node.parent_node() else { break };
            let mut sibling = if direction < 0 { node.previous_sibling() } else { node.next_sibling() };
            while let Some(skipped) = sibling.filter(|sibling| ignorable(view, sibling, direction)) {
                let index = super::domchange::dom_index(&skipped);
                moved = Some((parent.clone(), if direction < 0 { index } else { index + 1 }));
                sibling = if direction < 0 { skipped.previous_sibling() } else { skipped.next_sibling() };
            }
            if let Some(sibling) = sibling {
                node = sibling;
                offset = if direction < 0 { node_size(&node) } else { 0 };
            } else {
                offset = super::domchange::dom_index(&node) + u32::from(direction > 0);
                node = parent;
            }
        }
    }
    if let Some((node, offset)) = moved {
        if selection.is_collapsed() {
            selection.collapse_with_offset(Some(&node), offset)?;
        } else {
            selection.extend_with_offset(&node, offset)?;
        }
    }
    Ok(())
}

fn ignorable(view: &BrowserView, node: &Node, direction: i32) -> bool {
    view.with_doc_view(|view| view.is_widget(node)).unwrap_or(false)
        || (uneditable(node) && (direction < 0 || node.next_sibling().is_some() || node.node_name() != "BR"))
}

fn uneditable(node: &Node) -> bool {
    node.dyn_ref::<Element>().is_some_and(|element| element.get_attribute("contenteditable").as_deref() == Some("false"))
}

fn node_size(node: &Node) -> u32 {
    node.dyn_ref::<web_sys::Text>().map_or_else(|| node.child_nodes().length(), |text| text.length())
}
