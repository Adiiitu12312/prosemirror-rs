#![cfg(target_arch = "wasm32")]

use std::cell::Cell;
use std::rc::Rc;

use prosemirror_dropcursor::{browser_plugin, browser_plugin_with_disabled, DropCursorOptions};
use prosemirror_model::Schema;
use prosemirror_schema_basic::schema;
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_view::browser::{BrowserView, BrowserViewProps, Mount, ViewportPoint};
use serde_json::json;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use web_sys::{DataTransfer, DragEvent, DragEventInit, Element, EventTarget, HtmlElement};

wasm_bindgen_test_configure!(run_in_browser);

struct Editor {
    view: BrowserView,
    mount: Element,
}

impl Editor {
    fn new(schema: &Schema, props: BrowserViewProps) -> Self {
        let document = web_sys::window().unwrap().document().unwrap();
        let mount = document.create_element("div").unwrap();
        mount.set_attribute("style", "position: relative; width: 320px; padding: 12px;").unwrap();
        let style = document.create_element("style").unwrap();
        style.set_text_content(Some(".ProseMirror p { margin: 14px 0; min-height: 26px }"));
        mount.append_child(&style).unwrap();
        document.body().unwrap().append_child(&mount).unwrap();
        let doc = schema.node_from_json(&json!({"type":"doc","content":[
            {"type":"paragraph","content":[{"type":"text","text":"alpha"}]},
            {"type":"paragraph","content":[{"type":"text","text":"beta"}]},
            {"type":"horizontal_rule"},
        ]})).unwrap();
        let state = EditorState::create(EditorStateConfig {
            schema: schema.clone(), doc: Some(doc), selection: None, stored_marks: None, plugins: Vec::new(),
        });
        Self { view: BrowserView::new(Mount::Append(mount.clone()), state, props).unwrap(), mount }
    }

    fn inline_point(&self) -> (i32, i32) {
        let rect = self.view.dom().first_element_child().unwrap().get_bounding_client_rect();
        (rect.left() as i32 + 10, rect.top() as i32 + 7)
    }

    fn block_point(&self) -> (i32, i32) {
        let editor = self.view.dom();
        let first = editor.first_element_child().unwrap();
        let second = first.next_element_sibling().unwrap();
        let prev = first.get_bounding_client_rect();
        let next = second.get_bounding_client_rect();
        (prev.left() as i32 + 10, ((prev.bottom() + next.top()) / 2.0) as i32)
    }

    fn drag(&self, name: &str, (x, y): (i32, i32), related: Option<&EventTarget>) {
        let init = DragEventInit::new();
        init.set_bubbles(true);
        init.set_cancelable(true);
        init.set_client_x(x);
        init.set_client_y(y);
        init.set_related_target(related);
        let event = DragEvent::new_with_event_init_dict(name, &init).unwrap();
        self.view.dom().dispatch_event(&event).unwrap();
    }

    fn overlay(&self, kind: &str) -> Option<HtmlElement> {
        self.mount.query_selector(&format!(".prosemirror-dropcursor-{kind}")).unwrap()
            .map(|element| element.dyn_into().unwrap())
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        self.view.destroy();
        self.mount.remove();
    }
}

#[wasm_bindgen_test]
fn draws_an_inline_overlay_and_respects_the_drag_leave_target() {
    let editor = Editor::new(schema(), browser_plugin(DropCursorOptions {
        width: 3.0, color: None, class: Some("drop-indicator".into()),
    }));
    let point = editor.inline_point();
    editor.drag("dragover", point, None);
    let overlay = editor.overlay("inline").unwrap();
    assert!(overlay.class_list().contains("drop-indicator"));
    assert_eq!(overlay.style().get_property_value("position").unwrap(), "absolute");
    assert_eq!(overlay.style().get_property_value("background-color").unwrap(), "");
    assert!((overlay.get_bounding_client_rect().width() - 3.0).abs() < 0.1);
    assert!(overlay.parent_element().unwrap().is_same_node(Some(&editor.mount)));

    let paragraph = editor.view.dom().first_element_child().unwrap();
    editor.drag("dragleave", point, Some(paragraph.as_ref()));
    assert!(editor.overlay("inline").is_some());
    editor.drag("dragleave", point, None);
    assert!(editor.overlay("inline").is_none());
}

#[wasm_bindgen_test]
fn draws_a_horizontal_cursor_between_adjacent_blocks() {
    let editor = Editor::new(schema(), browser_plugin(DropCursorOptions::default()));
    editor.drag("dragover", editor.block_point(), None);
    let overlay = editor.overlay("block").unwrap().get_bounding_client_rect();
    assert!(overlay.width() > 100.0);
    assert!((overlay.height() - 1.0).abs() < 0.1);
}

#[wasm_bindgen_test]
fn uses_the_drag_slice_to_snap_a_block_drop_to_the_nearest_gap() {
    let editor = Editor::new(schema(), browser_plugin(DropCursorOptions::default()));
    let state = editor.view.state();
    let mut tr = state.tr();
    tr.set_selection(Selection::node(state.doc.child(0).node_size() + state.doc.child(1).node_size()));
    editor.view.dispatch(tr).unwrap();
    let init = DragEventInit::new();
    init.set_data_transfer(Some(&DataTransfer::new().unwrap()));
    let start = DragEvent::new_with_event_init_dict("dragstart", &init).unwrap();
    editor.view.dom().dispatch_event(&start).unwrap();
    assert!(editor.view.dragging_slice().is_some());

    editor.drag("dragover", editor.inline_point(), None);
    assert!(editor.overlay("block").is_some());
}

#[wasm_bindgen_test]
fn respects_schema_disabling_and_the_event_predicate() {
    let disabled_schema = Schema::new(schema().node_types().map(|kind| {
        let mut spec = kind.spec.clone();
        spec.disable_drop_cursor = kind.name == "paragraph";
        (kind.name.clone(), spec)
    }).collect(), schema().mark_types().map(|kind| (kind.name.clone(), kind.spec.clone())).collect());
    let disabled = Editor::new(&disabled_schema, browser_plugin(DropCursorOptions::default()));
    disabled.drag("dragover", disabled.inline_point(), None);
    assert!(disabled.overlay("inline").is_none());
    drop(disabled);

    let called = Rc::new(Cell::new(false));
    let seen = Rc::clone(&called);
    let editor = Editor::new(schema(), browser_plugin_with_disabled(DropCursorOptions::default(), Rc::new(move |view, point, event| {
        let element = view.pos_at_coords(ViewportPoint { left: f64::from(event.client_x()), top: f64::from(event.client_y()) }).unwrap();
        assert_eq!(point.inside, element.inside);
        seen.set(true);
        true
    })));
    editor.drag("dragover", editor.inline_point(), None);
    assert!(called.get());
    assert!(editor.overlay("inline").is_none());
}

#[wasm_bindgen_test]
async fn removes_the_overlay_after_drag_end_and_on_view_destruction() {
    let editor = Editor::new(schema(), browser_plugin(DropCursorOptions::default()));
    editor.drag("dragover", editor.inline_point(), None);
    editor.drag("dragend", editor.inline_point(), None);
    assert!(editor.overlay("inline").is_some());

    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 60).unwrap();
    });
    promise.await.unwrap();
    assert!(editor.overlay("inline").is_none());

    editor.drag("dragover", editor.inline_point(), None);
    assert!(editor.overlay("inline").is_some());
    editor.view.destroy();
    assert!(editor.overlay("inline").is_none());
}
