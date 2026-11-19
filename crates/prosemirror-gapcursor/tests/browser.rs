#![cfg(target_arch = "wasm32")]

use prosemirror_gapcursor::{browser_plugin, CSS};
use prosemirror_keymap::Platform;
use prosemirror_schema_basic::schema;
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_view::browser::{BrowserView, Mount};
use serde_json::json;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use web_sys::{Element, InputEvent, InputEventInit, KeyboardEvent, KeyboardEventInit, MouseEvent, MouseEventInit};

wasm_bindgen_test_configure!(run_in_browser);

struct Editor {
    view: BrowserView,
    mount: Element,
}

impl Editor {
    fn new(selection: Selection) -> Self {
        let document = web_sys::window().unwrap().document().unwrap();
        let mount = document.create_element("div").unwrap();
        let style = document.create_element("style").unwrap();
        style.set_text_content(Some(CSS));
        mount.append_child(&style).unwrap();
        document.body().unwrap().append_child(&mount).unwrap();
        let doc = schema().node_from_json(&json!({"type":"doc","content":[
            {"type":"horizontal_rule"}, {"type":"horizontal_rule"},
        ]})).unwrap();
        let state = EditorState::create(EditorStateConfig {
            schema: schema().clone(), doc: Some(doc), selection: Some(selection),
            stored_marks: None, plugins: Vec::new(),
        });
        let view = BrowserView::new(Mount::Append(mount.clone()), state, browser_plugin(Platform::Other, 1)).unwrap();
        Self { view, mount }
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        self.view.destroy();
        self.mount.remove();
    }
}

#[wasm_bindgen_test]
fn renders_a_visible_gap_cursor_widget_for_the_focused_editor() {
    let editor = Editor::new(Selection::gap_cursor(1));
    let widget = editor.view.dom().query_selector("hr + div.ProseMirror-gapcursor + hr").unwrap();
    assert!(widget.is_some());
    let widget = editor.view.dom().query_selector(".ProseMirror-gapcursor").unwrap().unwrap();
    assert_eq!(widget.get_attribute("contenteditable").as_deref(), Some("false"));

    editor.view.focus().unwrap();
    let display = web_sys::window().unwrap().get_computed_style(&widget).unwrap().unwrap()
        .get_property_value("display").unwrap();
    assert_eq!(display, "block");
}

#[wasm_bindgen_test]
fn handles_native_arrow_key_from_a_node_selection() {
    let editor = Editor::new(Selection::node(1));
    let init = KeyboardEventInit::new();
    init.set_key("ArrowLeft");
    init.set_cancelable(true);
    init.set_bubbles(true);
    let event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap();
    editor.view.dom().dispatch_event(&event).unwrap();
    assert!(event.default_prevented());
    assert_eq!(editor.view.state().selection, Selection::gap_cursor(1));
}

#[wasm_bindgen_test]
fn handles_a_native_click_between_selectable_blocks() {
    let editor = Editor::new(Selection::node(0));
    let first = editor.view.dom().first_element_child().unwrap();
    let second = first.next_element_sibling().unwrap();
    let before = first.get_bounding_client_rect();
    let after = second.get_bounding_client_rect();
    let init = MouseEventInit::new();
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_client_x((before.left() + 10.0) as i32);
    init.set_client_y(((before.bottom() + after.top()) / 2.0) as i32);
    let down = MouseEvent::new_with_mouse_event_init_dict("mousedown", &init).unwrap();
    let up = MouseEvent::new_with_mouse_event_init_dict("mouseup", &init).unwrap();
    editor.view.dom().dispatch_event(&down).unwrap();
    editor.view.dom().dispatch_event(&up).unwrap();
    assert!(up.default_prevented());
    assert_eq!(editor.view.state().selection, Selection::gap_cursor(1));
}

#[wasm_bindgen_test]
fn creates_an_inline_context_for_native_composition() {
    let editor = Editor::new(Selection::gap_cursor(1));
    let init = InputEventInit::new();
    init.set_input_type("insertCompositionText");
    init.set_cancelable(true);
    init.set_bubbles(true);
    let event = InputEvent::new_with_event_init_dict("beforeinput", &init).unwrap();
    editor.view.dom().dispatch_event(&event).unwrap();
    assert!(!event.default_prevented());
    assert_eq!(editor.view.state().selection, Selection::cursor(2));
    assert_eq!(editor.view.state().doc.child(1).type_name(), "paragraph");
    assert!(editor.view.dom().query_selector(".ProseMirror-gapcursor").unwrap().is_none());
}
