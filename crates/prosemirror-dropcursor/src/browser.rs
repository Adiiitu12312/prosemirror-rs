use std::cell::RefCell;
use std::rc::{Rc, Weak};

use prosemirror_state::EditorState;
use prosemirror_view::browser::{BrowserView, BrowserViewProps, PluginView, ViewportPoint};
use prosemirror_view::LayoutRect;
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{DomRect, DragEvent, Element, Event, HtmlElement, Node, Window};

use crate::{DragOver, DropCursor, DropCursorGeometry, DropCursorOptions, DropPosition};

/// Called for the node under the pointer when its schema does not disable the cursor.
pub type DisableDropCursor = Rc<dyn Fn(&BrowserView, DropPosition, &DragEvent) -> bool>;

/// Creates a browser plugin that draws and removes the drop cursor during native drags.
pub fn browser_plugin(options: DropCursorOptions) -> BrowserViewProps {
    plugin_props(options, None)
}

/// Creates a browser plugin with an event-aware predicate for disabling the cursor.
pub fn browser_plugin_with_disabled(options: DropCursorOptions, disable: DisableDropCursor) -> BrowserViewProps {
    plugin_props(options, Some(disable))
}

fn plugin_props(mut options: DropCursorOptions, disable: Option<DisableDropCursor>) -> BrowserViewProps {
    if options.color.as_ref().is_some_and(String::is_empty) {
        options.color = Some("black".into());
    }
    BrowserViewProps {
        plugin_views: vec![Rc::new(move |view| Box::new(DropCursorView::new(view, options.clone(), disable.clone())))],
        ..Default::default()
    }
}

struct DropCursorView {
    root: HtmlElement,
    state: Rc<RefCell<BrowserCursor>>,
    handlers: Vec<(&'static str, Closure<dyn FnMut(Event)>)>,
}

impl DropCursorView {
    fn new(view: &BrowserView, options: DropCursorOptions, disable: Option<DisableDropCursor>) -> Self {
        let state = Rc::new_cyclic(|weak: &Weak<RefCell<BrowserCursor>>| {
            let weak = weak.clone();
            let expire = Closure::new(move || {
                if let Some(state) = weak.upgrade() {
                    let mut state = state.borrow_mut();
                    state.timer = None;
                    state.clear();
                }
            });
            RefCell::new(BrowserCursor { cursor: DropCursor::new(options), element: None, timer: None, expire })
        });
        let root = view.dom();
        let weak_view = view.downgrade();
        let mut handlers = Vec::new();
        for name in ["dragover", "dragend", "drop", "dragleave"] {
            let weak_view = weak_view.clone();
            let cursor = Rc::downgrade(&state);
            let disable = disable.clone();
            let handler = Closure::new(move |event: Event| {
                let (Some(view), Some(cursor)) = (weak_view.upgrade(), cursor.upgrade()) else { return };
                let Some(event) = event.dyn_ref::<DragEvent>() else { return };
                let result = match name {
                    "dragover" => drag_over(&view, &cursor, event, disable.as_ref()),
                    "dragend" | "drop" => cursor.borrow_mut().schedule_removal(&view, 20),
                    "dragleave" => {
                        let related = event.related_target().and_then(|node| node.dyn_into::<Node>().ok());
                        if related.is_none_or(|node| !view.dom().contains(Some(&node))) {
                            cursor.borrow_mut().clear();
                        }
                        Ok(())
                    }
                    _ => unreachable!(),
                };
                if let Err(error) = result {
                    web_sys::console::error_1(&error);
                }
            });
            match root.add_event_listener_with_callback(name, handler.as_ref().unchecked_ref()) {
                Ok(()) => handlers.push((name, handler)),
                Err(error) => web_sys::console::error_1(&error),
            }
        }
        Self { root, state, handlers }
    }

    fn disconnect(&mut self) {
        for (name, handler) in self.handlers.drain(..) {
            let _ = self.root.remove_event_listener_with_callback(name, handler.as_ref().unchecked_ref());
        }
        let mut state = self.state.borrow_mut();
        state.clear_timeout();
        state.clear();
    }
}

impl PluginView for DropCursorView {
    fn update(&mut self, view: &BrowserView, previous_state: &EditorState) {
        let mut state = self.state.borrow_mut();
        let doc = view.state().doc;
        if state.cursor.position().is_some() && !doc.ptr_eq(&previous_state.doc) {
            state.cursor.update(&doc);
            if let Err(error) = state.update_overlay(view) {
                web_sys::console::error_1(&error);
            }
        }
    }

    fn destroy(&mut self) {
        self.disconnect();
    }
}

impl Drop for DropCursorView {
    fn drop(&mut self) {
        self.disconnect();
    }
}

fn drag_over(
    view: &BrowserView, cursor: &RefCell<BrowserCursor>, event: &DragEvent, disable: Option<&DisableDropCursor>,
) -> Result<(), JsValue> {
    if !view.editable() {
        return Ok(());
    }
    let Some(point) = view.pos_at_coords(ViewportPoint {
        left: f64::from(event.client_x()), top: f64::from(event.client_y()),
    }) else { return Ok(()) };
    let target = DropPosition { pos: point.pos, inside: point.inside };
    let doc = view.state().doc;
    if target.inside.and_then(|inside| doc.node_at(inside)).is_some_and(|node| {
        node.node_type().is_some_and(|kind| kind.spec.disable_drop_cursor)
            || disable.is_some_and(|disable| disable(view, target, event))
    }) {
        return Ok(());
    }
    let mut state = cursor.borrow_mut();
    let before = state.cursor.position();
    let dragging = view.dragging_slice();
    state.cursor.drag_over(&doc, DragOver {
        editable: true, target: Some(target), dragging: dragging.as_ref(), time_ms: js_sys::Date::now() as u64,
    }, |_, _| false);
    if state.cursor.position() != before || state.element.is_none() {
        state.update_overlay(view)?;
    }
    state.schedule_removal(view, 5_000)
}

struct BrowserCursor {
    cursor: DropCursor,
    element: Option<HtmlElement>,
    timer: Option<(Window, i32)>,
    expire: Closure<dyn FnMut()>,
}

impl BrowserCursor {
    fn clear(&mut self) {
        self.cursor.clear();
        self.remove_overlay();
    }

    fn remove_overlay(&mut self) {
        if let Some(element) = self.element.take() {
            element.remove();
        }
    }

    fn schedule_removal(&mut self, view: &BrowserView, delay: i32) -> Result<(), JsValue> {
        self.clear_timeout();
        if let Some(window) = view.dom().owner_document().and_then(|document| document.default_view()) {
            let timer = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                self.expire.as_ref().unchecked_ref(), delay,
            )?;
            self.timer = Some((window, timer));
        }
        Ok(())
    }

    fn clear_timeout(&mut self) {
        if let Some((window, timer)) = self.timer.take() {
            window.clear_timeout_with_handle(timer);
        }
    }

    fn update_overlay(&mut self, view: &BrowserView) -> Result<(), JsValue> {
        let editor = view.dom();
        let Some(document) = editor.owner_document() else { return Ok(()) };
        let Some(window) = document.default_view() else { return Ok(()) };
        let scale = element_scale(&editor);
        let geometry = BrowserGeometry { view, scale };
        let state = view.state();
        let Some(overlay) = self.cursor.overlay(&state.doc, &geometry) else {
            self.remove_overlay();
            return Ok(());
        };
        let offset_parent = editor.offset_parent().and_then(|parent| parent.dyn_into::<HtmlElement>().ok());
        let Some(parent) = offset_parent.clone().or_else(|| document.body()) else { return Ok(()) };
        if self.element.is_none() {
            let element = document.create_element("div")?.dyn_into::<HtmlElement>()?;
            if let Some(class) = overlay.class {
                element.set_class_name(class);
            }
            element.style().set_css_text("position: absolute; z-index: 50; pointer-events: none;");
            if let Some(color) = overlay.color {
                element.style().set_property("background-color", color)?;
            }
            parent.append_child(&element)?;
            self.element = Some(element);
        }
        let element = self.element.as_ref().expect("the drop cursor exists");
        let classes = element.class_list();
        classes.toggle_with_force("prosemirror-dropcursor-block", overlay.block)?;
        classes.toggle_with_force("prosemirror-dropcursor-inline", !overlay.block)?;
        let body_static = document.body().is_some_and(|body| parent.is_same_node(Some(&body)))
            && window.get_computed_style(&parent)?.is_some_and(|style| {
                style.get_property_value("position").is_ok_and(|position| position == "static")
            });
        let (left, top) = if offset_parent.is_none() || body_static {
            (-window.page_x_offset()?, -window.page_y_offset()?)
        } else {
            let rect = parent.get_bounding_client_rect();
            let (x, y) = element_scale(&parent);
            (rect.left() - f64::from(parent.scroll_left()) * x, rect.top() - f64::from(parent.scroll_top()) * y)
        };
        let style = element.style();
        let rect = overlay.rect;
        for (name, value) in [
            ("left", (f64::from(rect.x) - left) / scale.0),
            ("top", (f64::from(rect.y) - top) / scale.1),
            ("width", f64::from(rect.width) / scale.0),
            ("height", f64::from(rect.height) / scale.1),
        ] {
            style.set_property(name, &format!("{value}px"))?;
        }
        Ok(())
    }
}

fn element_scale(element: &HtmlElement) -> (f64, f64) {
    let rect = element.get_bounding_client_rect();
    let scale = |size, offset: i32| {
        if size > 0.0 && offset > 0 { size / f64::from(offset) } else { 1.0 }
    };
    (scale(rect.width(), element.offset_width()), scale(rect.height(), element.offset_height()))
}

struct BrowserGeometry<'a> {
    view: &'a BrowserView,
    scale: (f64, f64),
}

impl DropCursorGeometry for BrowserGeometry<'_> {
    fn position_rect(&self, pos: usize) -> Option<LayoutRect> {
        let rect = self.view.coords_at_pos(pos, 1)?;
        Some(LayoutRect { x: rect.left as f32, y: rect.top as f32,
            width: (rect.right - rect.left) as f32, height: (rect.bottom - rect.top) as f32 })
    }

    fn node_rect(&self, pos: usize) -> Option<LayoutRect> {
        let node = self.view.node_dom(pos)?;
        let rect: DomRect = node.dyn_ref::<Element>()?.get_bounding_client_rect();
        Some(LayoutRect { x: rect.left() as f32, y: rect.top() as f32, width: rect.width() as f32, height: rect.height() as f32 })
    }

    fn scale(&self) -> (f32, f32) {
        (self.scale.0 as f32, self.scale.1 as f32)
    }
}
