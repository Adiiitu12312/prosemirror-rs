//! Browser prompt and form fields from prosemirror-example-setup@1.2.3.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use prosemirror_menu::MenuPrompt;
use serde_json::{Map, Value};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{
    Document, Event, EventTarget, HtmlElement, HtmlInputElement, HtmlOptionElement,
    HtmlSelectElement, KeyboardEvent, Node, Window,
};

const PREFIX: &str = "ProseMirror-prompt";

pub type ValidateField = Rc<dyn Fn(&str) -> Option<String>>;
pub type CleanField = Rc<dyn Fn(String) -> Value>;

#[derive(Clone, Default)]
pub struct FieldOptions {
    pub label: String,
    pub value: String,
    pub required: bool,
    pub validate: Option<ValidateField>,
    pub clean: Option<CleanField>,
}

impl FieldOptions {
    fn validate(&self, value: &str) -> Option<String> {
        if self.required && value.is_empty() {
            Some("Required field".into())
        } else {
            self.validate.as_ref().and_then(|validate| validate(value))
        }
    }

    fn clean(&self, value: String) -> Value {
        if let Some(clean) = self.clean.as_ref() {
            clean(value)
        } else {
            Value::String(value)
        }
    }
}

pub trait Field {
    fn render(&self, document: &Document) -> Result<HtmlElement, JsValue>;

    fn read(&self, element: &HtmlElement) -> String {
        js_sys::Reflect::get(element.as_ref(), &"value".into()).ok()
            .and_then(|value| value.as_string()).unwrap_or_default()
    }

    fn validate(&self, _value: &str) -> Option<String> { None }

    fn clean(&self, value: String) -> Value { Value::String(value) }
}

pub struct TextField(pub FieldOptions);

impl Field for TextField {
    fn render(&self, document: &Document) -> Result<HtmlElement, JsValue> {
        let input: HtmlInputElement = document.create_element("input")?.dyn_into()?;
        input.set_type("text");
        input.set_placeholder(&self.0.label);
        input.set_value(&self.0.value);
        input.set_autocomplete("off");
        Ok(input.into())
    }

    fn validate(&self, value: &str) -> Option<String> { self.0.validate(value) }

    fn clean(&self, value: String) -> Value { self.0.clean(value) }
}

pub struct SelectField {
    pub field: FieldOptions,
    pub options: Vec<(String, String)>,
}

impl Field for SelectField {
    fn render(&self, document: &Document) -> Result<HtmlElement, JsValue> {
        let select: HtmlSelectElement = document.create_element("select")?.dyn_into()?;
        for (value, label) in &self.options {
            let option: HtmlOptionElement = document.create_element("option")?.dyn_into()?;
            option.set_value(value);
            option.set_label(label);
            option.set_selected(value == &self.field.value);
            select.append_child(&option)?;
        }
        Ok(select.into())
    }

    fn validate(&self, value: &str) -> Option<String> { self.field.validate(value) }

    fn clean(&self, value: String) -> Value { self.field.clean(value) }
}

/// A mounted prompt. Dropping it closes the form and unregisters its handlers.
pub struct Prompt(Rc<PromptInner>);

impl Prompt {
    pub fn dom(&self) -> HtmlElement { self.0.wrapper.clone() }

    pub fn close(&self) { self.0.close(); }
}

impl Drop for Prompt {
    fn drop(&mut self) { self.close(); }
}

pub fn open_menu_prompt(
    document: &Document, prompt: MenuPrompt, on_submit: impl FnOnce(Value) + 'static,
) -> Result<Prompt, JsValue> {
    let fields = prompt.fields.into_iter().map(|field| {
        let input = TextField(FieldOptions {
            label: field.label,
            value: field.value,
            required: field.required,
            ..FieldOptions::default()
        });
        (field.name, Box::new(input) as Box<dyn Field>)
    }).collect();
    open_prompt(document, &prompt.title, fields, on_submit)
}

pub fn open_prompt(
    document: &Document, title: &str, fields: Vec<(String, Box<dyn Field>)>,
    on_submit: impl FnOnce(Value) + 'static,
) -> Result<Prompt, JsValue> {
    let window = document.default_view().ok_or_else(|| JsValue::from_str("No browser window"))?;
    let wrapper = element(document, "div")?;
    wrapper.set_class_name(PREFIX);
    let form = element(document, "form")?;
    wrapper.append_child(&form)?;
    if !title.is_empty() {
        let heading = element(document, "h5")?;
        heading.set_text_content(Some(title));
        form.append_child(&heading)?;
    }
    let mut rendered = Vec::with_capacity(fields.len());
    for (name, field) in fields {
        let row = element(document, "div")?;
        let input = field.render(document)?;
        row.append_child(&input)?;
        form.append_child(&row)?;
        rendered.push(RenderedField { name, field, input });
    }

    let buttons = element(document, "div")?;
    buttons.set_class_name(&format!("{PREFIX}-buttons"));
    let submit = button(document, "submit", "OK", "submit")?;
    let cancel = button(document, "button", "Cancel", "cancel")?;
    buttons.append_child(&submit)?;
    buttons.append_child(&document.create_text_node(" "))?;
    buttons.append_child(&cancel)?;
    form.append_child(&buttons)?;
    let body = document.body().ok_or_else(|| JsValue::from_str("No document body"))?;
    body.append_child(&wrapper)?;

    let prompt = Prompt(Rc::new(PromptInner {
        wrapper, window, fields: rendered, closed: Cell::new(false), outside: Cell::new(false),
        on_submit: RefCell::new(Some(Box::new(on_submit))), listeners: RefCell::new(Vec::new()),
    }));
    prompt.0.center()?;
    prompt.0.on(cancel.into(), "click", |prompt, _| prompt.close())?;
    prompt.0.on(form.clone().into(), "submit", |prompt, event| {
        event.prevent_default();
        prompt.submit();
    })?;
    let weak = Rc::downgrade(&prompt.0);
    prompt.0.on(form.into(), "keydown", move |prompt, event| {
        let Some(event) = event.dyn_ref::<KeyboardEvent>() else { return };
        let key = event.key();
        if key == "Escape" || event.key_code() == 27 {
            event.prevent_default();
            prompt.close();
        } else if (key == "Enter" || event.key_code() == 13) && !event.ctrl_key() && !event.meta_key() && !event.shift_key() {
            event.prevent_default();
            prompt.submit();
        } else if key == "Tab" || event.key_code() == 9 {
            let weak = weak.clone();
            let _ = later(&prompt.window, 500, move || {
                if let Some(prompt) = weak.upgrade() {
                    let active = prompt.wrapper.owner_document().and_then(|document| document.active_element());
                    if active.is_none_or(|active| !prompt.wrapper.contains(Some(&active))) {
                        prompt.close();
                    }
                }
            });
        }
    })?;
    prompt.0.on(prompt.0.window.clone().into(), "mousedown", |prompt, event| {
        if prompt.outside.get() {
            let target = event.target().and_then(|target| target.dyn_into::<Node>().ok());
            if !prompt.wrapper.contains(target.as_ref()) {
                prompt.close();
            }
        }
    })?;
    let weak = Rc::downgrade(&prompt.0);
    later(&prompt.0.window, 50, move || {
        if let Some(prompt) = weak.upgrade() {
            prompt.outside.set(true);
        }
    })?;
    prompt.0.fields.first().map_or(submit, |field| field.input.clone()).focus()?;
    Ok(prompt)
}

struct RenderedField {
    name: String,
    field: Box<dyn Field>,
    input: HtmlElement,
}

type Submit = Box<dyn FnOnce(Value)>;

struct PromptInner {
    wrapper: HtmlElement,
    window: Window,
    fields: Vec<RenderedField>,
    closed: Cell<bool>,
    outside: Cell<bool>,
    on_submit: RefCell<Option<Submit>>,
    listeners: RefCell<Vec<Listener>>,
}

impl PromptInner {
    fn center(&self) -> Result<(), JsValue> {
        let rect = self.wrapper.get_bounding_client_rect();
        let height = self.window.inner_height()?.as_f64().unwrap_or_default();
        let width = self.window.inner_width()?.as_f64().unwrap_or_default();
        self.wrapper.style().set_property("top", &format!("{}px", (height - rect.height()) / 2.0))?;
        self.wrapper.style().set_property("left", &format!("{}px", (width - rect.width()) / 2.0))
    }

    fn close(&self) {
        if !self.closed.replace(true) {
            self.listeners.borrow_mut().clear();
            self.on_submit.borrow_mut().take();
            self.wrapper.remove();
        }
    }

    fn submit(&self) {
        if self.closed.get() {
            return;
        }
        let mut result = Map::new();
        for field in &self.fields {
            let value = field.field.read(&field.input);
            if let Some(error) = field.field.validate(&value) {
                if let Err(error) = self.report_invalid(&field.input, &error) {
                    web_sys::console::error_1(&error);
                }
                return;
            }
            result.insert(field.name.clone(), field.field.clean(value));
        }
        let submit = self.on_submit.borrow_mut().take();
        self.close();
        if let Some(submit) = submit {
            submit(Value::Object(result));
        }
    }

    fn report_invalid(&self, input: &HtmlElement, error: &str) -> Result<(), JsValue> {
        let Some(document) = input.owner_document() else { return Ok(()) };
        let Some(parent) = input.parent_node() else { return Ok(()) };
        let message = element(&document, "div")?;
        message.set_class_name("ProseMirror-invalid");
        message.set_text_content(Some(error));
        message.style().set_property("left", &format!("{}px", input.offset_left() + input.offset_width() + 2))?;
        message.style().set_property("top", &format!("{}px", input.offset_top() - 5))?;
        parent.append_child(&message)?;
        later(&self.window, 1500, move || message.remove())
    }

    fn on(self: &Rc<Self>, target: EventTarget, name: &'static str, handle: impl Fn(&Self, Event) + 'static) -> Result<(), JsValue> {
        let weak = Rc::downgrade(self);
        let callback = Closure::new(move |event| {
            if let Some(prompt) = weak.upgrade() {
                handle(&prompt, event);
            }
        });
        target.add_event_listener_with_callback(name, callback.as_ref().unchecked_ref())?;
        self.listeners.borrow_mut().push(Listener { target, name, callback });
        Ok(())
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

fn element(document: &Document, name: &str) -> Result<HtmlElement, JsValue> {
    document.create_element(name)?.dyn_into().map_err(JsValue::from)
}

fn button(document: &Document, kind: &str, label: &str, class: &str) -> Result<HtmlElement, JsValue> {
    let button = element(document, "button")?;
    button.set_attribute("type", kind)?;
    button.set_class_name(&format!("{PREFIX}-{class}"));
    button.set_text_content(Some(label));
    Ok(button)
}

fn later(window: &Window, timeout: i32, run: impl FnOnce() + 'static) -> Result<(), JsValue> {
    let callback = Closure::once_into_js(run);
    window.set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), timeout)?;
    Ok(())
}
