//! Browser toolbar, dropdowns, and floating layout from prosemirror-menu 1.3.0.

mod dom;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use prosemirror_state::{EditorState, Transaction};
use prosemirror_view::browser::{BrowserView, PluginView, PluginViewFactory};
use serde_json::{Map, Value};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{
    Document, DomRect, Element, Event, EventTarget, FocusEvent, HtmlElement, KeyboardEvent,
    MouseEvent, Node, Selection, ShadowRoot, Window,
};

use crate::{
    FloatingGeometry, MenuBar, MenuBarOptions, MenuElement, MenuEvent, MenuEventSource, MenuFocus,
    MenuHost, MenuItem, MenuKey, MenuPath, MenuPosition, MenuPrompt, Rect,
};
use crate::menubar::lookup;
use dom::{control_path, controls_element, element, show, MenuDom};

/// Upstream styles for the browser toolbar, dropdowns, and icons.
pub const CSS: &str = include_str!("../style/menu.css");

pub type MenuTranslator = Rc<dyn Fn(&str) -> String>;
pub type BrowserMenuPrompt = Rc<dyn Fn(&BrowserView, &MenuPrompt) -> Option<Value>>;
pub type BrowserMenuActivation = Rc<dyn Fn(&BrowserView, &[usize], &MenuItem, &MenuEvent) -> bool>;

#[derive(Clone, Default)]
pub struct BrowserMenuOptions {
    pub position: MenuPosition,
    pub floating: bool,
    pub translate: Option<MenuTranslator>,
    /// Provides a synchronous prompt for native menu actions.
    pub prompt: Option<BrowserMenuPrompt>,
    /// Intercepts enabled actions to support browser-specific dialogs and controls.
    pub activate: Option<BrowserMenuActivation>,
}

/// An editor plugin view that inserts the toolbar and wraps the editable DOM.
pub fn menu_bar(content: Vec<Vec<MenuElement>>, options: BrowserMenuOptions) -> PluginViewFactory {
    Rc::new(move |view| {
        let menu = BrowserMenuView::mount(view, content.clone(), options.clone())
            .unwrap_or_else(|error| wasm_bindgen::throw_val(error));
        Box::new(menu)
    })
}

struct BrowserMenuView {
    inner: Rc<MenuView>,
    listeners: Vec<Listener>,
}

struct MenuView {
    view: BrowserView,
    document: Document,
    window: Window,
    wrapper: HtmlElement,
    toolbar: HtmlElement,
    dom: RefCell<MenuDom>,
    dom_root: RefCell<Node>,
    model: RefCell<MenuBar>,
    options: BrowserMenuOptions,
    original_parent: bool,
    spacer: RefCell<Option<HtmlElement>>,
    focus_generation: Cell<u32>,
    destroyed: Cell<bool>,
}

impl BrowserMenuView {
    fn mount(view: &BrowserView, content: Vec<Vec<MenuElement>>, options: BrowserMenuOptions) -> Result<Self, JsValue> {
        let editor = view.dom();
        let document = editor.owner_document().ok_or_else(|| JsValue::from_str("Menu needs an owner document"))?;
        let window = document.default_view().ok_or_else(|| JsValue::from_str("Menu needs a browser window"))?;
        let wrapper = element(&document, "div", "ProseMirror-menubar-wrapper")?;
        let toolbar = element(&document, "div", "ProseMirror-menubar")?;
        toolbar.set_attribute("role", "toolbar")?;
        controls_element(&toolbar, Some(&editor));
        let model = MenuBar::new(content, MenuBarOptions { position: options.position, floating: options.floating });
        let projections = model.snapshot(&view.state(), &UnattachedHost(&options)).map_err(menu_error)?;
        let root = view.root();
        let dom = MenuDom::new(&document, &root, &toolbar, &projections.groups)?;
        let old_parent = editor.parent_node();
        if let Some(parent) = &old_parent {
            parent.replace_child(&wrapper, &editor)?;
        }
        for child in match options.position {
            MenuPosition::Before => [&toolbar, &editor],
            MenuPosition::After => [&editor, &toolbar],
        } {
            wrapper.append_child(child)?;
        }
        let inner = Rc::new(MenuView {
            view: view.clone(), document, window, wrapper, toolbar,
            dom: RefCell::new(dom), dom_root: RefCell::new(root), model: RefCell::new(model),
            options, original_parent: old_parent.is_some(), spacer: RefCell::new(None),
            focus_generation: Cell::new(0), destroyed: Cell::new(false),
        });
        let mut result = Self { inner, listeners: Vec::new() };
        result.listen()?;
        result.inner.refresh(None)?;
        Ok(result)
    }

    fn listen(&mut self) -> Result<(), JsValue> {
        let bar: EventTarget = self.inner.toolbar.clone().into();
        let window: EventTarget = self.inner.window.clone().into();
        self.add(&bar, "mousedown", |_, event| {
            if event_element(event).is_some_and(|element| control_path(&element, true).is_some()) {
                event.prevent_default();
            }
            Ok(())
        })?;
        self.add(&bar, "click", |inner, event| {
            let Some(path) = event_element(event).and_then(|element| control_path(&element, true)) else {
                return Ok(())
            };
            let Some(mouse) = event.dyn_ref::<MouseEvent>() else { return Ok(()) };
            inner.activate(&path, MenuEvent {
                source: if mouse.detail() == 0 { MenuEventSource::Keyboard } else { MenuEventSource::Pointer },
                shift: mouse.shift_key(), ctrl: mouse.ctrl_key(), alt: mouse.alt_key(), meta: mouse.meta_key(),
                ..MenuEvent::default()
            })?;
            Ok(())
        })?;
        self.add(&bar, "keydown", |inner, event| {
            let Some(event) = event.dyn_ref::<KeyboardEvent>() else { return Ok(()) };
            let Some(key) = menu_key(&event.key()) else { return Ok(()) };
            let Some(path) = event_element(event).and_then(|element| control_path(&element, true)) else {
                return Ok(())
            };
            let handled = if matches!(key, MenuKey::Enter | MenuKey::Space) {
                inner.activate(&path, MenuEvent {
                    source: MenuEventSource::Keyboard, key: Some(key),
                    shift: event.shift_key(), ctrl: event.ctrl_key(), alt: event.alt_key(), meta: event.meta_key(),
                })?
            } else {
                let state = inner.view.state();
                let mut host = inner.host();
                let handled = inner.model.borrow_mut().handle_key(&path, key, &state, &mut host);
                if handled {
                    inner.refresh(None)?;
                    host.apply()?;
                }
                handled
            };
            if handled {
                event.prevent_default();
                event.stop_propagation();
            }
            Ok(())
        })?;
        self.add(&bar, "focusin", |inner, event| {
            let Some(path) = event_element(event).and_then(|element| control_path(&element, true)) else {
                return Ok(())
            };
            inner.cancel_focusout();
            inner.model.borrow_mut().focus_changed(MenuFocus::Control(path));
            inner.refresh(None)
        })?;
        self.add(&bar, "focusout", |inner, event| {
            if let Some(focus) = event.dyn_ref::<FocusEvent>() {
                if focus.related_target().and_then(|target| target.dyn_into::<Node>().ok())
                    .is_some_and(|target| inner.toolbar.contains(Some(&target)))
                {
                    return Ok(());
                }
            }
            inner.close_after_focusout()
        })?;
        self.add(&bar, "pointerover", |inner, event| {
            let path = event_element(event).and_then(|element| control_path(&element, false));
            inner.model.borrow_mut().hover_changed(path);
            inner.refresh(None)
        })?;
        self.add(&bar, "pointerout", |inner, event| {
            let next = event.dyn_ref::<MouseEvent>().and_then(MouseEvent::related_target)
                .and_then(|target| target.dyn_into::<Element>().ok());
            let path = next.filter(|next| inner.toolbar.contains(Some(next))).and_then(|next| control_path(&next, false));
            inner.model.borrow_mut().hover_changed(path);
            inner.refresh(None)
        })?;
        self.add(&window, "click", |inner, event| {
            let path = event_element(event).filter(|element| inner.toolbar.contains(Some(element)))
                .and_then(|element| control_path(&element, false));
            inner.model.borrow_mut().outside_click(path.as_ref());
            inner.refresh(None)
        })?;
        if self.inner.options.floating && !self.inner.ios() {
            let mut ancestors = vec![window];
            let mut current = self.inner.wrapper.parent_node();
            while let Some(parent) = current {
                current = parent.parent_node();
                ancestors.push(parent.into());
            }
            for ancestor in ancestors {
                self.add(&ancestor, "scroll", |inner, event| {
                    let top = event_element(event).map(|element| element.get_bounding_client_rect().top());
                    inner.update_layout(top)
                })?;
            }
        }
        Ok(())
    }

    fn add(
        &mut self, target: &EventTarget, name: &'static str,
        f: impl Fn(&Rc<MenuView>, &Event) -> Result<(), JsValue> + 'static,
    ) -> Result<(), JsValue> {
        let weak = Rc::downgrade(&self.inner);
        let callback = Closure::<dyn FnMut(Event)>::new(move |event| {
            if let Some(inner) = weak.upgrade().filter(|inner| !inner.destroyed.get()) {
                report(f(&inner, &event));
            }
        });
        target.add_event_listener_with_callback(name, callback.as_ref().unchecked_ref())?;
        self.listeners.push(Listener { target: target.clone(), name, callback });
        Ok(())
    }
}

impl PluginView for BrowserMenuView {
    fn update(&mut self, _: &BrowserView, _: &EditorState) {
        report(self.inner.update());
    }

    fn destroy(&mut self) {
        self.inner.destroy();
        self.listeners.clear();
    }
}

impl Drop for BrowserMenuView {
    fn drop(&mut self) {
        self.destroy();
    }
}

impl MenuView {
    fn host(&self) -> Host<'_> {
        Host { menu: self, dispatch: Vec::new(), focus: None }
    }

    fn activate(&self, path: &MenuPath, event: MenuEvent) -> Result<bool, JsValue> {
        let state = self.view.state();
        let mut host = self.host();
        if let Some(activate) = &self.options.activate {
            let item = {
                let model = self.model.borrow();
                lookup(model.content(), path).and_then(|item| match item {
                    MenuElement::Item(item) if model.available(path, &state, &host.focus()) && item.state(&state).enabled =>
                        Some(item.clone()),
                    _ => None,
                })
            };
            if let Some(item) = item {
                let focused = host.focus();
                if activate(&self.view, path, &item, &event) {
                    if event.source == MenuEventSource::Pointer {
                        self.model.borrow_mut().outside_click(None);
                    }
                    let control = MenuFocus::Control(path.clone());
                    if (focused == MenuFocus::Editor || focused == control) && host.focus() == control {
                        host.focus_editor();
                    }
                    self.refresh(None)?;
                    host.apply()?;
                    return Ok(true);
                }
            }
        }
        let handled = self.model.borrow_mut().activate(path, &state, &mut host, event);
        self.refresh(None)?;
        host.apply()?;
        Ok(handled)
    }

    fn update(&self) -> Result<(), JsValue> {
        self.refresh(Some(&self.view.state()))?;
        self.scroll_selection()
    }

    fn refresh(&self, state: Option<&EditorState>) -> Result<(), JsValue> {
        if self.destroyed.get() || self.view.is_destroyed() {
            return Ok(());
        }
        let mut host = self.host();
        let snapshot = if let Some(state) = state {
            self.model.borrow_mut().update(state, &mut host)
        } else {
            self.model.borrow().snapshot(&self.view.state(), &host)
        }.map_err(menu_error)?;
        let root = self.view.root();
        if !root.is_same_node(Some(&self.dom_root.borrow())) {
            self.toolbar.set_text_content(None);
            *self.dom.borrow_mut() = MenuDom::new(&self.document, &root, &self.toolbar, &snapshot.groups)?;
            *self.dom_root.borrow_mut() = root;
        }
        self.dom.borrow().update(&snapshot.groups)?;
        host.apply()?;
        if self.model.borrow().layout.floating.is_none() {
            self.update_layout(None)?;
        }
        Ok(())
    }

    fn focused(&self) -> MenuFocus {
        // The document reports the shadow host, so query the shadow root first.
        let active = self.view.root().dyn_into::<ShadowRoot>().ok().and_then(|root| root.active_element())
            .or_else(|| self.document.active_element());
        if let Some(active) = active {
            if self.toolbar.contains(Some(&active)) {
                return control_path(&active, true).map_or(MenuFocus::Other, MenuFocus::Control);
            }
            if self.view.dom().contains(Some(&active)) {
                return MenuFocus::Editor;
            }
        }
        MenuFocus::Other
    }

    fn cancel_focusout(&self) -> u32 {
        let generation = self.focus_generation.get().wrapping_add(1);
        self.focus_generation.set(generation);
        generation
    }

    fn close_after_focusout(self: &Rc<Self>) -> Result<(), JsValue> {
        let generation = self.cancel_focusout();
        let weak = Rc::downgrade(self);
        let callback = Closure::once_into_js(move || {
            if let Some(inner) = weak.upgrade().filter(|inner| !inner.destroyed.get() && inner.focus_generation.get() == generation) {
                let focus = inner.focused();
                inner.model.borrow_mut().focus_changed(focus);
                report(inner.refresh(None));
            }
        });
        self.window.set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), 20)?;
        Ok(())
    }

    fn update_layout(&self, scroll_ancestor_top: Option<f64>) -> Result<(), JsValue> {
        let geometry = FloatingGeometry {
            wrapper: rect(&self.wrapper.get_bounding_client_rect()),
            toolbar: rect(&self.toolbar.get_bounding_client_rect()),
            wrapper_offset_width: self.wrapper.offset_width().into(),
            wrapper_client_width: self.wrapper.client_width().into(),
            toolbar_offset_height: self.toolbar.offset_height().into(),
            viewport_height: self.window.inner_height()?.as_f64().unwrap_or_default(),
            scroll_ancestor_top,
            ios: self.ios(),
        };
        let (floating, max_height) = {
            let mut model = self.model.borrow_mut();
            model.update_layout(geometry);
            (model.layout.floating, model.layout.max_height)
        };
        let style = self.toolbar.style();
        if let Some(floating) = floating {
            style.set_property("position", "fixed")?;
            style.set_property("left", &pixels(floating.left))?;
            style.set_property("top", &floating.top.map_or_else(String::new, pixels))?;
            style.set_property("width", &pixels(floating.width))?;
            show(&self.toolbar, floating.visible)?;
            if self.spacer.borrow().is_none() {
                let spacer = element(&self.document, "div", "ProseMirror-menubar-spacer")?;
                spacer.style().set_property("height", &pixels(floating.spacer_height))?;
                self.wrapper.insert_before(&spacer, Some(&self.toolbar))?;
                self.spacer.replace(Some(spacer));
            }
        } else {
            if let Some(spacer) = self.spacer.take() {
                spacer.remove();
                for property in ["position", "left", "top", "width", "display"] {
                    style.remove_property(property)?;
                }
            }
            if max_height > 0.0 {
                style.set_property("min-height", &pixels(max_height))?;
            }
        }
        Ok(())
    }

    fn scroll_selection(&self) -> Result<(), JsValue> {
        if self.model.borrow().layout.floating.is_none() {
            return Ok(());
        }
        let selection = self.document.get_selection()?;
        let Some(selection) = selection.filter(|selection| selection.range_count() > 0) else { return Ok(()) };
        let Some(focus) = selection.focus_node().filter(|node| self.view.dom().contains(Some(node))) else { return Ok(()) };
        let Some(rects) = selection.get_range_at(0)?.get_client_rects() else { return Ok(()) };
        let rects: Vec<_> = (0..rects.length()).filter_map(|index| rects.item(index)).map(|value| rect(&value)).collect();
        let toolbar = rect(&self.toolbar.get_bounding_client_rect());
        let inverted = selection_inverted(&selection, &focus);
        let delta = self.model.borrow().layout.selection_scroll_delta(toolbar, &rects, inverted);
        if let Some(delta) = delta {
            let mut current = self.wrapper.parent_node();
            while let Some(node) = current {
                if let Some(element) = node.dyn_ref::<Element>()
                    .filter(|element| element.scroll_height() > element.client_height())
                {
                    element.set_scroll_top(element.scroll_top() + delta.round() as i32);
                    break;
                }
                current = node.parent_node();
            }
        }
        Ok(())
    }

    fn ios(&self) -> bool {
        self.window.navigator().user_agent().is_ok_and(|agent| {
            !agent.contains("Edge/") && agent.contains("AppleWebKit") && agent.contains("Mobile/")
        })
    }

    fn destroy(&self) {
        if self.destroyed.replace(true) {
            return;
        }
        self.cancel_focusout();
        let editor = self.view.dom();
        if let Some(parent) = self.wrapper.parent_node() {
            if editor.parent_node().is_some_and(|parent| self.wrapper.is_same_node(Some(&parent))) {
                report(parent.replace_child(&editor, &self.wrapper).map(|_| ()));
            } else {
                self.wrapper.remove();
            }
        } else if !self.original_parent && editor.parent_node().is_some_and(|parent| self.wrapper.is_same_node(Some(&parent))) {
            report(self.wrapper.remove_child(&editor).map(|_| ()));
        }
    }
}

struct UnattachedHost<'a>(&'a BrowserMenuOptions);

impl MenuHost for UnattachedHost<'_> {
    fn dispatch(&mut self, _: Transaction) {}

    fn translate(&self, text: &str) -> String {
        self.0.translate.as_ref().map_or_else(|| text.to_owned(), |map| map(text))
    }
}

struct Host<'a> {
    menu: &'a MenuView,
    dispatch: Vec<Transaction>,
    focus: Option<MenuFocus>,
}

impl Host<'_> {
    fn apply(self) -> Result<(), JsValue> {
        // Dispatch and focus can synchronously call back into the toolbar.
        // Release the model and DOM borrows before running their effects.
        for transaction in self.dispatch {
            self.menu.view.dispatch(transaction)?;
        }
        match self.focus {
            Some(MenuFocus::Editor) => self.menu.view.focus()?,
            Some(MenuFocus::Control(path)) => {
                let button = self.menu.dom.borrow().button(&path).cloned();
                if let Some(button) = button {
                    button.focus()?;
                }
            }
            _ => (),
        }
        Ok(())
    }
}

impl MenuHost for Host<'_> {
    fn dispatch(&mut self, transaction: Transaction) {
        self.dispatch.push(transaction);
    }

    fn focus(&self) -> MenuFocus {
        self.focus.clone().unwrap_or_else(|| self.menu.focused())
    }

    fn focus_editor(&mut self) {
        self.focus = Some(MenuFocus::Editor);
    }

    fn focus_control(&mut self, path: &MenuPath) {
        self.focus = Some(MenuFocus::Control(path.clone()));
    }

    fn translate(&self, text: &str) -> String {
        UnattachedHost(&self.menu.options).translate(text)
    }

    fn prompt(&mut self, prompt: &MenuPrompt) -> Option<Value> {
        if let Some(callback) = &self.menu.options.prompt {
            return callback(&self.menu.view, prompt);
        }
        let mut attrs = Map::new();
        for field in &prompt.fields {
            let message = format!("{}\n{}", self.translate(&prompt.title), self.translate(&field.label));
            let value = self.menu.window.prompt_with_message_and_default(&message, &field.value).ok().flatten()?;
            if field.required && value.trim().is_empty() {
                return None;
            }
            attrs.insert(field.name.clone(), Value::String(value));
        }
        Some(Value::Object(attrs))
    }
}

struct Listener {
    target: EventTarget,
    name: &'static str,
    callback: Closure<dyn FnMut(Event)>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self.target.remove_event_listener_with_callback(self.name, self.callback.as_ref().unchecked_ref());
    }
}

fn rect(rect: &DomRect) -> Rect {
    Rect { left: rect.left(), top: rect.top(), right: rect.right(), bottom: rect.bottom() }
}

fn selection_inverted(selection: &Selection, focus: &Node) -> bool {
    selection.anchor_node().is_some_and(|anchor| {
        if anchor.is_same_node(Some(focus)) {
            selection.anchor_offset() > selection.focus_offset()
        } else {
            anchor.compare_document_position(focus) == Node::DOCUMENT_POSITION_FOLLOWING
        }
    })
}

fn pixels(length: f64) -> String { format!("{length}px") }

fn event_element(event: &Event) -> Option<Element> {
    event.target()?.dyn_into().ok()
}

fn menu_key(key: &str) -> Option<MenuKey> {
    Some(match key {
        "ArrowUp" => MenuKey::ArrowUp,
        "ArrowDown" => MenuKey::ArrowDown,
        "ArrowLeft" => MenuKey::ArrowLeft,
        "ArrowRight" => MenuKey::ArrowRight,
        "Home" => MenuKey::Home,
        "End" => MenuKey::End,
        "Escape" => MenuKey::Escape,
        "Enter" => MenuKey::Enter,
        " " | "Spacebar" => MenuKey::Space,
        _ => return None,
    })
}

fn menu_error(error: crate::MenuError) -> JsValue {
    js_sys::Error::new(&error.to_string()).into()
}

fn report(result: Result<(), JsValue>) {
    if let Err(error) = result {
        web_sys::console::error_1(&error);
    }
}
