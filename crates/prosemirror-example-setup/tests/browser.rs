#![cfg(all(target_arch = "wasm32", feature = "browser"))]

use std::cell::RefCell;
use std::rc::Rc;

use prosemirror_example_setup::browser::{
    example_setup, prompt::{open_prompt, Field, FieldOptions, SelectField, TextField},
};
use prosemirror_example_setup::ExampleSetupOptions;
use prosemirror_keymap::Platform;
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_test_builder::{doc, p, test_schema};
use prosemirror_view::{BrowserView, Mount};
use serde_json::json;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use web_sys::{
    Document, Event, EventInit, HtmlInputElement, HtmlSelectElement, KeyboardEvent, KeyboardEventInit,
};

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> Document {
    web_sys::window().unwrap().document().unwrap()
}

fn submit(form: &web_sys::Element) {
    let init = EventInit::new();
    init.set_cancelable(true);
    let event = Event::new_with_event_init_dict("submit", &init).unwrap();
    assert!(!form.dispatch_event(&event).unwrap());
}

#[wasm_bindgen_test]
fn runs_the_platform_keymap_and_configures_the_editor() {
    let mut options = ExampleSetupOptions::new(test_schema());
    options.platform = Platform::Other;
    options.menu_bar = false;
    options.history = None;
    let setup = example_setup(options).unwrap();
    let fixture = doc!(p!("a<a>b"));
    let selection = Selection::cursor(fixture.tag("a"));
    let state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(fixture.node),
        selection: Some(selection),
        stored_marks: None,
        plugins: setup.plugins,
    });
    let body = document().body().unwrap();
    let view = BrowserView::new(Mount::Append(body.into()), state, setup.props).unwrap();
    assert!(view.dom().class_list().contains("ProseMirror-example-setup-style"));
    assert!(view.dom().parent_element().unwrap().query_selector(".ProseMirror-menubar").unwrap().is_none());

    let key = KeyboardEventInit::new();
    key.set_key("b");
    key.set_ctrl_key(true);
    key.set_cancelable(true);
    key.set_bubbles(true);
    let event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &key).unwrap();
    assert!(!view.dom().dispatch_event(&event).unwrap());
    assert_eq!(view.state().stored_marks.unwrap()[0].type_name, "strong");
    view.destroy();
}

#[wasm_bindgen_test]
fn validates_cleans_and_reads_prompt_fields() {
    let saved = Rc::new(RefCell::new(None));
    let result = saved.clone();
    let fields: Vec<(String, Box<dyn Field>)> = vec![
        ("href".into(), Box::new(TextField(FieldOptions {
            label: "Link target".into(),
            required: true,
            clean: Some(Rc::new(|value| json!(value.trim()))),
            ..FieldOptions::default()
        }))),
        ("kind".into(), Box::new(SelectField {
            field: FieldOptions { value: "internal".into(), ..FieldOptions::default() },
            options: vec![("external".into(), "External".into()), ("internal".into(), "Internal".into())],
        })),
    ];
    let prompt = open_prompt(&document(), "Create a link", fields, move |attrs| *result.borrow_mut() = Some(attrs)).unwrap();
    let root = prompt.dom();
    let form = root.query_selector("form").unwrap().unwrap();
    submit(&form);
    assert!(saved.borrow().is_none());
    let invalid = root.query_selector(".ProseMirror-invalid").unwrap().unwrap();
    assert_eq!(invalid.text_content().as_deref(), Some("Required field"));
    let input = root.query_selector("input").unwrap().unwrap().dyn_into::<HtmlInputElement>().unwrap();
    assert_eq!(input.placeholder(), "Link target");
    input.set_value(" /guide ");
    let select = root.query_selector("select").unwrap().unwrap().dyn_into::<HtmlSelectElement>().unwrap();
    assert_eq!(select.value(), "internal");
    submit(&form);
    assert_eq!(*saved.borrow(), Some(json!({"href": "/guide", "kind": "internal"})));
    assert!(root.parent_node().is_none());
}

#[wasm_bindgen_test]
fn escape_closes_the_prompt_without_submitting() {
    let prompt = open_prompt(&document(), "", Vec::new(), |_| panic!("cancelled form must not submit")).unwrap();
    let form = prompt.dom().query_selector("form").unwrap().unwrap();
    let init = KeyboardEventInit::new();
    init.set_key("Escape");
    init.set_cancelable(true);
    let key = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap();
    assert!(!form.dispatch_event(&key).unwrap());
    assert!(prompt.dom().parent_node().is_none());
}
