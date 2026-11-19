use std::cell::RefCell;
use std::rc::{Rc, Weak};

use prosemirror_view::browser::{BrowserView, PluginView, PluginViewFactory, ViewEventHandler};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{Event, EventTarget, HtmlElement, MouseEvent, Node};

pub(super) struct Listener {
    target: EventTarget,
    kind: &'static str,
    callback: Closure<dyn FnMut(Event)>,
}

impl Listener {
    pub(super) fn new(
        target: &EventTarget,
        kind: &'static str,
        callback: impl FnMut(Event) + 'static,
    ) -> Result<Self, JsValue> {
        let callback = Closure::wrap(Box::new(callback) as Box<dyn FnMut(Event)>);
        target.add_event_listener_with_callback(kind, callback.as_ref().unchecked_ref())?;
        Ok(Self { target: target.clone(), kind, callback })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self.target.remove_event_listener_with_callback(self.kind, self.callback.as_ref().unchecked_ref());
    }
}

struct DragSession {
    editor: HtmlElement,
    _listeners: Vec<Listener>,
}

#[derive(Default)]
pub(super) struct DragSessions {
    active: RefCell<Vec<DragSession>>,
}

impl DragSessions {
    pub(super) fn start(&self, editor: HtmlElement, listeners: Vec<Listener>) {
        self.stop(&editor);
        self.active.borrow_mut().push(DragSession { editor, _listeners: listeners });
    }

    pub(super) fn stop(&self, editor: &HtmlElement) {
        self.active.borrow_mut().retain(|session| !session.editor.is_same_node(Some(editor.as_ref())));
    }

    pub(super) fn cleanup(self: &Rc<Self>) -> PluginViewFactory {
        let sessions = Rc::downgrade(self);
        Rc::new(move |view| Box::new(DragCleanup { sessions: sessions.clone(), editor: view.dom() }))
    }
}

struct DragCleanup {
    sessions: Weak<DragSessions>,
    editor: HtmlElement,
}

impl PluginView for DragCleanup {
    fn destroy(&mut self) {
        if let Some(sessions) = self.sessions.upgrade() {
            sessions.stop(&self.editor);
        }
    }
}

pub(super) fn mouse_handler(
    handle: impl Fn(&BrowserView, &MouseEvent) -> Result<bool, JsValue> + 'static,
) -> ViewEventHandler<Event> {
    Rc::new(move |view, event| {
        event.dyn_ref::<MouseEvent>().is_some_and(|event| match handle(view, event) {
            Ok(handled) => handled,
            Err(error) => {
                web_sys::console::error_1(&error);
                false
            }
        })
    })
}

pub(super) fn report(result: Result<(), JsValue>) {
    if let Err(error) = result {
        web_sys::console::error_1(&error);
    }
}

pub(super) fn dom_cell(view: &BrowserView, event: &MouseEvent) -> Option<HtmlElement> {
    let editor = view.dom();
    let mut target: Node = event.target()?.dyn_into().ok()?;
    loop {
        if target.is_same_node(Some(editor.as_ref())) {
            return None;
        }
        if matches!(target.node_name().as_str(), "TD" | "TH") {
            return target.dyn_into().ok();
        }
        target = target.parent_node()?;
    }
}
