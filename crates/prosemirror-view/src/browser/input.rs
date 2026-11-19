//! Event routing and input handlers from prosemirror-view's input.ts.

use js_sys::Date;
use prosemirror_model::{Node as ModelNode, Slice};
use prosemirror_state::Selection;
use prosemirror_transform::drop_point;
use serde_json::json;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{ClipboardEvent, DragEvent, Event, KeyboardEvent, KeyboardEventInit, MouseEvent, Node};

use super::{capturekeys, clipboard::{clipboard_text, single_node}, domchange, error,
    BrowserView, ClickHandler, ClickOnHandler, PositionAtCoords, SelectionOrigin, ViewportPoint};

pub(super) const EVENTS: &[&str] = &[
    "keydown", "keyup", "keypress", "mousedown", "mouseup", "mousemove", "touchstart",
    "touchmove", "contextmenu", "compositionstart", "compositionupdate", "compositionend",
    "copy", "cut", "paste", "dragstart", "dragend", "dragover", "dragenter", "drop",
    "focus", "blur", "beforeinput", "input",
];

#[derive(Default)]
pub(super) struct InputState {
    shift_key: bool,
    last_key_code: u32,
    last_key_dispatched: bool,
    last_click: Option<Click>,
    mouse_down: Option<Click>,
    triple_selection: Option<Selection>,
    dragging: Option<Dragging>,
}

#[derive(Clone, Copy)]
struct Click {
    time: f64,
    x: i32,
    y: i32,
    button: i16,
    count: u8,
}

#[derive(Clone)]
struct Dragging {
    slice: Slice,
    node: Option<Selection>,
}

pub(super) fn handle_event(view: &BrowserView, event: &Event) -> Result<(), JsValue> {
    if !event_belongs_to_view(view, event) {
        return Ok(());
    }
    dispatch_event(view, event)
}

pub(super) fn dispatch_event(view: &BrowserView, event: &Event) -> Result<(), JsValue> {
    if view.is_destroyed() {
        return Ok(());
    }
    let kind = event.type_();
    if view.props().providers().any(|props| props.handle_dom_events.get(&kind)
        .is_some_and(|handler| handler(view, event) || event.default_prevented()))
    {
        return Ok(());
    }
    if is_edit_event(&kind) && !view.editable() {
        return Ok(());
    }
    match kind.as_str() {
        "keydown" => with_event(event, |event: &KeyboardEvent| key_down(view, event)),
        "keyup" => with_event(event, |event: &KeyboardEvent| {
            if event.key() == "Shift" {
                view.0.borrow_mut().input.shift_key = false;
            }
            Ok(())
        }),
        "keypress" => with_event(event, |event: &KeyboardEvent| key_press(view, event)),
        "compositionstart" => domchange::composition_start(view),
        "compositionupdate" => domchange::composition_update(view),
        "compositionend" => domchange::composition_end(view),
        "mousedown" => with_event(event, |event: &MouseEvent| mouse_down(view, event)),
        "mouseup" => with_event(event, |event: &MouseEvent| mouse_up(view, event)),
        "mousemove" => with_event(event, |event: &MouseEvent| mouse_move(view, event)),
        "touchstart" | "touchmove" | "contextmenu" => {
            view.with_input(|input| input.record_selection(SelectionOrigin::Pointer));
            view.flush_dom()
        }
        "copy" | "cut" => with_event(event, |event: &ClipboardEvent| copy(view, event, kind == "cut")),
        "paste" => with_event(event, |event: &ClipboardEvent| paste(view, event)),
        "dragstart" => with_event(event, |event: &DragEvent| drag_start(view, event)),
        "dragend" => {
            view.0.borrow_mut().input.dragging = None;
            Ok(())
        }
        "dragover" | "dragenter" => {
            event.prevent_default();
            Ok(())
        }
        "drop" => with_event(event, |event: &DragEvent| drop_event(view, event)),
        "focus" | "blur" => {
            let root = view.dom();
            if kind == "focus" {
                root.class_list().add_1("ProseMirror-focused")?;
            } else {
                root.class_list().remove_1("ProseMirror-focused")?;
            }
            Ok(())
        }
        "input" => view.flush_dom(),
        _ => Ok(()),
    }
}

fn with_event<E: JsCast>(event: &Event, handle: impl FnOnce(&E) -> Result<(), JsValue>) -> Result<(), JsValue> {
    event.dyn_ref().map_or(Ok(()), handle)
}

fn is_edit_event(kind: &str) -> bool {
    matches!(kind, "keydown" | "keyup" | "keypress" | "compositionstart" | "compositionupdate"
        | "compositionend" | "cut" | "paste" | "dragover" | "dragenter" | "drop")
}

fn event_belongs_to_view(view: &BrowserView, event: &Event) -> bool {
    if !event.bubbles() {
        return true;
    }
    if event.default_prevented() {
        return false;
    }
    let root = view.dom();
    let Some(target) = event.target().and_then(|target| target.dyn_into::<Node>().ok()) else { return false };
    root.contains(Some(&target)) && view.with_doc_view(|doc| !doc.stop_event(event)).unwrap_or(false)
}

pub(super) fn dispatch_key(view: &BrowserView, key: &str) -> Result<bool, JsValue> {
    let options = KeyboardEventInit::new();
    options.set_key(key);
    options.set_bubbles(true);
    options.set_cancelable(true);
    options.set_key_code(match key {
        "Backspace" => 8,
        "Enter" => 13,
        _ => 0,
    });
    let event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &options)?;
    Ok(run_key_handler(view, &event))
}

fn run_key_handler(view: &BrowserView, event: &KeyboardEvent) -> bool {
    if view.composing() || view.0.borrow().input.last_key_dispatched {
        return false;
    }
    view.0.borrow_mut().input.last_key_dispatched = true;
    view.props().providers().any(|props| props.handle_key_down.as_ref().is_some_and(|handler| handler(view, event)))
}

fn key_down(view: &BrowserView, event: &KeyboardEvent) -> Result<(), JsValue> {
    let key_code = event.key_code();
    let mut inner = view.0.borrow_mut();
    inner.input.shift_key = event.shift_key() || key_code == 16;
    inner.input.last_key_code = key_code;
    inner.input.last_key_dispatched = false;
    drop(inner);
    if event.is_composing() || view.with_input(|input| input.in_or_near_composition()) {
        return Ok(());
    }
    let key = event.key();
    view.with_input(|input| input.record_key(&key));
    if key_code != 229 {
        view.flush_dom()?;
    }
    if run_key_handler(view, event) || capturekeys::capture_key_down(view, event)? {
        event.prevent_default();
    } else {
        view.with_input(|input| input.record_selection(SelectionOrigin::Key));
    }
    Ok(())
}

fn key_press(view: &BrowserView, event: &KeyboardEvent) -> Result<(), JsValue> {
    let code = event.char_code();
    if view.with_input(|input| input.in_or_near_composition()) || code == 0 || (event.ctrl_key() && !event.alt_key())
        || (is_mac(view) && event.meta_key())
    {
        return Ok(());
    }
    let config = view.props();
    if config.providers().any(|props| props.handle_key_press.as_ref().is_some_and(|handler| handler(view, event))) {
        event.prevent_default();
        return Ok(());
    }
    let state = config.state;
    let start = state.doc.resolve(state.selection.from()).map_err(error)?;
    let end = state.doc.resolve(state.selection.to(&state.doc)).map_err(error)?;
    if !matches!(state.selection, Selection::Text { .. }) || !start.same_parent(&end) {
        if let Some(ch) = char::from_u32(code).filter(|ch| !matches!(ch, '\r' | '\n')) {
            let text = ch.to_string();
            let default = || {
                let mut tr = state.tr();
                tr.insert_text_at_selection(&text).scroll_into_view();
                tr
            };
            if !view.handle_text_input(start.pos, end.pos, &text, &default)? {
                view.dispatch(default())?;
            }
        }
        event.prevent_default();
    }
    Ok(())
}

fn event_coords(event: &MouseEvent) -> ViewportPoint {
    ViewportPoint { left: f64::from(event.client_x()), top: f64::from(event.client_y()) }
}

fn is_near(click: Click, event: &MouseEvent) -> bool {
    let dx = f64::from(click.x) - f64::from(event.client_x());
    let dy = f64::from(click.y) - f64::from(event.client_y());
    dx * dx + dy * dy < 100.0
}

pub(super) fn is_mac(view: &BrowserView) -> bool {
    view.dom().owner_document().and_then(|doc| doc.default_view())
        .and_then(|window| window.navigator().platform().ok())
        .is_some_and(|platform| platform.starts_with("Mac"))
}

fn node_modifier(view: &BrowserView, event: &MouseEvent) -> bool {
    if is_mac(view) { event.meta_key() } else { event.ctrl_key() }
}

fn mouse_down(view: &BrowserView, event: &MouseEvent) -> Result<(), JsValue> {
    domchange::end_composition(view)?;
    let now = Date::now();
    let modifier = node_modifier(view, event);
    let mut inner = view.0.borrow_mut();
    let count = inner.input.last_click.filter(|last| {
        last.count < 3 && now - last.time < 500.0 && is_near(*last, event)
            && !modifier && last.button == event.button()
    }).map_or(1, |last| last.count + 1);
    let click = Click { time: now, x: event.client_x(), y: event.client_y(), count, button: event.button() };
    inner.input.shift_key = event.shift_key();
    inner.input.last_click = Some(click);
    inner.input.mouse_down = (count == 1).then_some(click);
    inner.input.triple_selection = None;
    drop(inner);
    view.with_input(|input| input.record_selection(SelectionOrigin::Pointer));
    if let Some(pos) = (count > 1).then(|| view.pos_at_coords(event_coords(event))).flatten() {
        let inside = pos.inside;
        if click_handlers(view, pos, event, count) {
            event.prevent_default();
        } else if let Some(selection) = (count == 3 && event.button() == 0)
            .then(|| triple_selection(&view.state().doc, inside, true)).flatten()
        {
            set_pointer_selection(view, selection.clone())?;
            if matches!(selection, Selection::Text { .. }) {
                view.0.borrow_mut().input.triple_selection = Some(selection);
            }
            event.prevent_default();
        }
    }
    Ok(())
}

fn mouse_up(view: &BrowserView, event: &MouseEvent) -> Result<(), JsValue> {
    let mut inner = view.0.borrow_mut();
    inner.input.triple_selection = None;
    let down = inner.input.mouse_down.take();
    drop(inner);
    let Some(down) = down.filter(|down| is_near(*down, event)) else { return Ok(()) };
    let Some(pos) = view.pos_at_coords(event_coords(event)) else { return Ok(()) };
    let inside = pos.inside;
    if click_handlers(view, pos, event, down.count) {
        event.prevent_default();
    } else if let Some(inside) = inside.filter(|_| !event.shift_key()) {
        let state = view.state();
        let resolved = state.doc.resolve(inside).map_err(error)?;
        let select = if node_modifier(view, event) {
            nearby_nodes(&state.doc, Some(inside)).into_iter().find(|(node, _)| node.is_selectable())
                .map(|(_, pos)| pos)
        } else {
            resolved.node_after().filter(|node| node.is_atom() && node.is_selectable()).map(|_| inside)
        };
        if let Some(pos) = select {
            set_pointer_selection(view, Selection::node(pos))?;
            event.prevent_default();
        }
    }
    Ok(())
}

fn mouse_move(view: &BrowserView, event: &MouseEvent) -> Result<(), JsValue> {
    let mut inner = view.0.borrow_mut();
    if event.buttons() == 0 {
        inner.input.mouse_down = None;
        inner.input.triple_selection = None;
        return Ok(());
    }
    if inner.input.mouse_down.is_some_and(|down| !is_near(down, event)) {
        inner.input.mouse_down = None;
    }
    let triple = inner.input.triple_selection.clone();
    drop(inner);
    let Some(triple) = triple else { return Ok(()) };
    let Some(pos) = view.pos_at_coords(event_coords(event)) else { return Ok(()) };
    let state = view.state();
    if let Some(current) = triple_selection(&state.doc, pos.inside, false) {
        let selection = if current.from() < triple.from() {
            Selection::text(triple.to(&state.doc), current.from())
        } else {
            Selection::text(triple.from(), current.to(&state.doc))
        };
        set_pointer_selection(view, selection)?;
        event.prevent_default();
    }
    Ok(())
}

fn set_pointer_selection(view: &BrowserView, selection: Selection) -> Result<(), JsValue> {
    if !view.has_focus() {
        view.focus()?;
    }
    let state = view.state();
    if state.selection != selection {
        let mut tr = state.tr();
        tr.set_selection(selection).set_meta("pointer", json!(true));
        view.dispatch(tr)?;
    }
    Ok(())
}

fn nearby_nodes(doc: &ModelNode, inside: Option<usize>) -> Vec<(ModelNode, usize)> {
    let Some(pos) = inside.and_then(|pos| doc.resolve(pos).ok()) else { return Vec::new() };
    let mut nodes = Vec::new();
    if let Some(node) = pos.node_after() {
        nodes.push((node, pos.pos));
    }
    for depth in (1..=pos.depth).rev() {
        nodes.push((pos.node(depth).clone(), pos.before(depth)));
    }
    nodes
}

fn click_handlers(view: &BrowserView, pos: PositionAtCoords, event: &MouseEvent, count: u8) -> bool {
    let config = view.props();
    let handler_on = |props: &super::BrowserViewProps| -> Option<ClickOnHandler> {
        match count {
            1 => props.handle_click_on.clone(),
            2 => props.handle_double_click_on.clone(),
            _ => props.handle_triple_click_on.clone(),
        }
    };
    let handler = |props: &super::BrowserViewProps| -> Option<ClickHandler> {
        match count {
            1 => props.handle_click.clone(),
            2 => props.handle_double_click.clone(),
            _ => props.handle_triple_click.clone(),
        }
    };
    for (node, at) in nearby_nodes(&config.state.doc, pos.inside) {
        if config.providers().filter_map(handler_on)
            .any(|on| on(view, pos.pos, &node, at, event, Some(at) == pos.inside))
        {
            return true;
        }
    }
    let mut handlers = config.providers().filter_map(handler);
    handlers.any(|click| click(view, pos.pos, event))
}

fn triple_selection(doc: &ModelNode, inside: Option<usize>, select_nodes: bool) -> Option<Selection> {
    if inside.is_none() && doc.inline_content() {
        return Some(Selection::text(0, doc.content().size()));
    }
    for (node, pos) in nearby_nodes(doc, inside) {
        if node.inline_content() {
            return Some(Selection::text(pos + 1, pos + 1 + node.content().size()));
        }
        if select_nodes && node.is_selectable() {
            return Some(Selection::node(pos));
        }
    }
    None
}

fn copy(view: &BrowserView, event: &ClipboardEvent, cut: bool) -> Result<(), JsValue> {
    let state = view.state();
    if state.selection.empty() {
        return Ok(());
    }
    if let Some(data) = event.clipboard_data() {
        let content = view.serialize_for_clipboard(state.selection.content(&state.doc).map_err(error)?)?;
        event.prevent_default();
        data.clear_data()?;
        data.set_data("text/html", &content.dom.inner_html())?;
        data.set_data("text/plain", &content.text)?;
    }
    if cut {
        let mut tr = state.tr();
        tr.delete_selection().scroll_into_view().set_meta("uiEvent", json!("cut"));
        view.dispatch(tr)?;
    }
    Ok(())
}

fn paste(view: &BrowserView, event: &ClipboardEvent) -> Result<(), JsValue> {
    if view.composing() && !view.dom().owner_document().and_then(|doc| doc.default_view())
        .and_then(|window| window.navigator().user_agent().ok())
        .is_some_and(|ua| ua.contains("Android"))
    {
        return Ok(());
    }
    if let Some(data) = event.clipboard_data() {
        let plain = {
            let inner = view.0.borrow();
            inner.input.shift_key && inner.input.last_key_code != 45
        };
        if view.paste_content(&clipboard_text(&data)?, Some(&data.get_data("text/html")?), plain, Some(event))? {
            event.prevent_default();
        }
    }
    Ok(())
}

fn drag_moves(view: &BrowserView, event: &DragEvent) -> bool {
    let config = view.props();
    let mut copies = config.providers().filter_map(|props| props.drag_copies.as_ref()).peekable();
    if copies.peek().is_some() {
        !copies.any(|copies| copies(event))
    } else if is_mac(view) {
        !event.alt_key()
    } else {
        !event.ctrl_key()
    }
}

fn drag_start(view: &BrowserView, event: &DragEvent) -> Result<(), JsValue> {
    view.0.borrow_mut().input.mouse_down = None;
    let Some(data) = event.data_transfer() else { return Ok(()) };
    let state = view.state();
    let coords = event_coords(event);
    let in_selection = !state.selection.empty() && view.pos_at_coords(coords).is_some_and(|hit| {
        hit.pos >= state.selection.from() && hit.pos <= state.selection.to(&state.doc)
            .saturating_sub(usize::from(matches!(state.selection, Selection::Node { .. })))
    });
    let node = if in_selection {
        None
    } else {
        let target = event.target().and_then(|target| target.dyn_into::<Node>().ok());
        target.and_then(|target| {
            view.with_doc_view(|doc| doc.nearest_node_desc(&target).and_then(|desc| {
                (!desc.is_root && desc.node.node_type().is_some_and(|kind| kind.spec.draggable))
                    .then_some(Selection::node(desc.pos_before))
            })).flatten()
        })
    };
    let slice = node.as_ref().unwrap_or(&state.selection).content(&state.doc).map_err(error)?;
    let content = view.serialize_for_clipboard(slice)?;
    data.clear_data()?;
    data.set_data("text/html", &content.dom.inner_html())?;
    data.set_data("text/plain", &content.text)?;
    data.set_effect_allowed("copyMove");
    view.0.borrow_mut().input.dragging = Some(Dragging { slice: content.slice, node });
    Ok(())
}

fn drop_event(view: &BrowserView, event: &DragEvent) -> Result<(), JsValue> {
    let dragging = view.0.borrow_mut().input.dragging.take();
    let Some(data) = event.data_transfer() else { return Ok(()) };
    let Some(pos) = view.pos_at_coords(event_coords(event)) else { return Ok(()) };
    let config = view.props();
    let state = &config.state;
    let context = state.doc.resolve(pos.pos).map_err(error)?;
    let slice = if let Some(dragging) = &dragging {
        Some(config.providers().filter_map(|props| props.transform_pasted.as_ref())
            .fold(dragging.slice.clone(), |slice, transform| transform(slice, view, false)))
    } else {
        view.parse_clipboard(&clipboard_text(&data)?, Some(&data.get_data("text/html")?), false, &context)?
    };
    let moved = dragging.is_some() && drag_moves(view, event);
    let empty = Slice::empty();
    if config.providers().filter_map(|props| props.handle_drop.as_ref())
        .any(|handler| handler(view, event, slice.as_ref().unwrap_or(&empty), moved))
    {
        event.prevent_default();
        return Ok(());
    }
    let Some(slice) = slice else { return Ok(()) };
    event.prevent_default();
    let insertion = drop_point(&state.doc, context.pos, &slice).unwrap_or(context.pos);
    let mut tr = state.tr();
    if moved {
        if let Some(node) = dragging.and_then(|dragging| dragging.node) {
            tr.set_selection(node);
        }
        tr.delete_selection();
    }
    let at = tr.mapping().map(insertion, 1);
    let before = tr.doc().clone();
    let node = single_node(&slice).cloned();
    if let Some(node) = &node {
        tr.replace_range_with(at, at, node.clone());
    } else {
        tr.replace_range(at, at, slice);
    }
    if tr.doc() == &before {
        return Ok(());
    }
    let from = tr.doc().resolve(at).map_err(error)?;
    let selection = if node.as_ref().is_some_and(|node| node.is_selectable()
        && from.node_after().is_some_and(|after| after.same_markup(node)))
    {
        Selection::node(at)
    } else {
        let mut end = tr.mapping().map(insertion, 1);
        if let Some(map) = tr.mapping().maps.last() {
            map.for_each(|_, _, _, new_end| end = new_end);
        }
        let to = tr.doc().resolve(end).map_err(error)?;
        config.some_prop(|props| props.create_selection_between.as_ref().and_then(|create| create(view, &from, &to)))
            .unwrap_or_else(|| Selection::between(&from, &to, 1))
    };
    tr.set_selection(selection).set_meta("uiEvent", json!("drop"));
    view.focus()?;
    view.dispatch(tr)?;
    Ok(())
}
