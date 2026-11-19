#![cfg(all(target_arch = "wasm32", feature = "browser"))]

use std::collections::HashMap;
use std::ops::Deref;

use prosemirror_model::{Node, Schema};
use prosemirror_schema_basic::{marks, nodes};
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_tables::browser::{column_resizing_props, table_editing_props, BrowserTableView};
use prosemirror_tables::{column_resizing, table_editing, table_nodes, ColumnResizingOptions, ResizeState, TableEditingOptions, TableNodesOptions, TABLE_EDITING_KEY};
use prosemirror_view::browser::{BrowserView, BrowserViewProps, Mount};
use serde_json::{json, Value};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use web_sys::{ClipboardEvent, ClipboardEventInit, DataTransfer, Document, Element, EventTarget, HtmlElement, KeyboardEvent, KeyboardEventInit, MouseEvent, MouseEventInit};

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> Document {
    web_sys::window().unwrap().document().unwrap()
}

fn schema() -> Schema {
    let mut kinds = nodes();
    kinds.extend(table_nodes(TableNodesOptions::default()));
    Schema::new(kinds, marks())
}

fn cell(text: &str, width: Option<u32>) -> Value {
    json!({"type": "table_cell", "attrs": {"colwidth": width.map(|width| [width])},
        "content": [{"type": "paragraph", "content": [{"type": "text", "text": text}]}]})
}

fn table(schema: &Schema, rows: &[&[Value]]) -> Node {
    let rows: Vec<_> = rows.iter().map(|cells| json!({"type": "table_row", "content": cells})).collect();
    schema.node_from_json(&json!({"type": "table", "content": rows})).unwrap()
}

struct Fixture {
    view: BrowserView,
    host: Element,
}

impl Deref for Fixture {
    type Target = BrowserView;

    fn deref(&self) -> &BrowserView {
        &self.view
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.view.destroy();
        self.host.remove();
    }
}

fn mount(options: Option<ColumnResizingOptions>, selection: Option<Selection>) -> Fixture {
    let schema = schema();
    let table = table(&schema, &[
        &[cell("a", Some(90)), cell("b", Some(110))],
        &[cell("c", Some(90)), cell("d", Some(110))],
    ]);
    let doc = schema.node_from_json(&json!({"type": "doc", "content": [table]})).unwrap();
    let editing = table_editing(TableEditingOptions::default());
    let mut state_plugins = HashMap::from([(editing.key.clone(), table_editing_props())]);
    let mut plugins = Vec::new();
    if let Some(options) = options {
        let resizing = column_resizing();
        state_plugins.insert(resizing.key.clone(), column_resizing_props(&schema, options));
        plugins.push(resizing);
    }
    plugins.push(editing);
    let state = EditorState::create(EditorStateConfig {
        schema, doc: Some(doc), selection, stored_marks: None, plugins,
    });
    let document = document();
    let host = document.create_element("section").unwrap();
    host.set_attribute("style", "padding:10px;width:260px").unwrap();
    let style = document.create_element("style").unwrap();
    style.set_text_content(Some(&format!("{}\n{}\n.ProseMirror td {{ height:28px; border:1px solid black }}\n.ProseMirror td p {{ margin:0 }}",
        prosemirror_view::browser::CSS, prosemirror_tables::browser::CSS)));
    host.append_child(&style).unwrap();
    document.body().unwrap().append_child(&host).unwrap();
    let view = BrowserView::new(Mount::Append(host.clone()), state, BrowserViewProps {
        state_plugins, ..BrowserViewProps::default()
    }).unwrap();
    Fixture { view, host }
}

fn html_node(view: &BrowserView, pos: usize) -> HtmlElement {
    view.node_dom(pos).unwrap().dyn_into().unwrap()
}

fn style(element: &HtmlElement, name: &str) -> String {
    element.style().get_property_value(name).unwrap()
}

fn click_point(view: &BrowserView, pos: usize) -> (i32, i32) {
    let position = view.coords_at_pos(pos, 1).unwrap();
    ((position.left + 1.0).round() as i32, ((position.top + position.bottom) / 2.0).round() as i32)
}

fn mouse(target: &EventTarget, kind: &str, point: (i32, i32), button: i16, buttons: u16) -> bool {
    let init = MouseEventInit::new();
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_client_x(point.0);
    init.set_client_y(point.1);
    init.set_button(button);
    init.set_buttons(buttons);
    let event = MouseEvent::new_with_mouse_event_init_dict(kind, &init).unwrap();
    target.dispatch_event(&event).unwrap()
}

fn key(view: &BrowserView, name: &str, code: u32, shift: bool) -> bool {
    let init = KeyboardEventInit::new();
    init.set_key(name);
    init.set_key_code(code);
    init.set_shift_key(shift);
    init.set_cancelable(true);
    let event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap();
    view.dom().dispatch_event(&event).unwrap()
}

#[wasm_bindgen_test]
fn updates_live_table_columns_and_reuses_the_existing_colgroup() {
    let schema = schema();
    let initial = table(&schema, &[&[cell("a", Some(40)), cell("b", Some(60))]]);
    let mut view = BrowserTableView::new(&document(), initial, 100.0).unwrap();
    assert_eq!(view.dom.class_name(), "tableWrapper");
    assert_eq!(view.content_dom.tag_name(), "TBODY");
    assert_eq!(style(&view.table, "--default-cell-min-width"), "100px");
    assert_eq!(style(&view.table, "width"), "100px");
    let first = view.colgroup.first_child().unwrap();

    let updated = table(&schema, &[&[cell("a", None)]]);
    assert!(view.update(&updated).unwrap());
    assert!(view.colgroup.first_child().unwrap().is_same_node(Some(&first)));
    assert_eq!(view.colgroup.child_nodes().length(), 1);
    assert_eq!(style(&view.table, "width"), "");
    assert_eq!(style(&view.table, "min-width"), "100px");
    assert_eq!(style(&first.dyn_into::<HtmlElement>().unwrap(), "width"), "");
}

#[wasm_bindgen_test]
fn dragging_a_column_previews_dom_width_then_updates_every_row() {
    let view = mount(Some(ColumnResizingOptions::default()), None);
    let first = html_node(&view, 2);
    let table: HtmlElement = view.dom().query_selector("table").unwrap().unwrap().dyn_into().unwrap();
    assert_eq!(style(&table, "width"), "200px");
    assert!(view.dom().query_selector(".column-resize-handle").unwrap().is_none());
    let point = ((first.get_bounding_client_rect().right() - 2.0).round() as i32, click_point(&view, 4).1);
    assert!(mouse(first.as_ref(), "mousemove", point, 0, 0));
    assert_eq!(ResizeState::get(&view.state()).unwrap().active_handle, Some(2));
    assert!(view.dom().class_name().contains("resize-cursor"));
    assert!(view.dom().query_selector("td .column-resize-handle").unwrap().is_some());
    assert!(!mouse(first.as_ref(), "mousedown", point, 0, 1));
    assert!(ResizeState::get(&view.state()).unwrap().dragging.is_some());
    assert!(first.class_name().contains("column-resize-dragging"));

    let window = web_sys::window().unwrap();
    let end = (point.0 + 35, point.1);
    mouse(window.as_ref(), "mousemove", end, 0, 1);
    assert_eq!(style(&table, "width"), "235px");
    assert_eq!(view.state().doc.child(0).child(0).child(0).attrs().unwrap()["colwidth"], json!([90]));
    mouse(window.as_ref(), "mouseup", end, 0, 0);
    let state = view.state();
    assert!(ResizeState::get(&state).unwrap().dragging.is_none());
    for row in &state.doc.child(0).content().children {
        assert_eq!(row.child(0).attrs().unwrap()["colwidth"][0].as_f64(), Some(125.0));
        assert_eq!(row.child(1).attrs().unwrap()["colwidth"][0].as_f64(), Some(110.0));
    }
    assert_eq!(style(&table, "width"), "235px");
    assert_eq!(html_node(&view, 2).get_attribute("data-colwidth").as_deref(), Some("125"));
    mouse(view.dom().as_ref(), "mouseleave", end, 0, 0);
    assert!(ResizeState::get(&view.state()).unwrap().active_handle.is_none());
    assert!(view.dom().query_selector(".column-resize-handle").unwrap().is_none());
}

#[wasm_bindgen_test]
fn right_click_leaves_selection_unmanaged_and_left_drag_selects_cells() {
    let view = mount(None, None);
    let first = html_node(&view, 2);
    let last = html_node(&view, 19);
    let start = click_point(&view, 4);
    let end = click_point(&view, 21);
    mouse(first.as_ref(), "mousedown", start, 2, 2);
    mouse(last.as_ref(), "mousemove", end, 0, 2);
    assert!(!matches!(view.state().selection, Selection::Cell(_)));

    mouse(first.as_ref(), "mousedown", start, 0, 1);
    mouse(last.as_ref(), "mousemove", end, 0, 1);
    let selection = view.state().selection;
    let Selection::Cell(selection) = selection else { panic!("expected a cell selection") };
    assert_eq!((selection.anchor, selection.head), (2, 19));
    assert_eq!(view.state().get_plugin_state(TABLE_EDITING_KEY), Some(&json!(2)));
    assert_eq!(view.dom().query_selector_all(".selectedCell").unwrap().length(), 4);

    mouse(last.as_ref(), "mouseup", end, 0, 0);
    assert_eq!(view.state().get_plugin_state(TABLE_EDITING_KEY), Some(&Value::Null));
    assert_eq!(view.state().selection, Selection::Cell(selection));
}

#[wasm_bindgen_test]
fn arrow_extension_and_delete_use_the_table_keymap() {
    let view = mount(None, Some(Selection::cursor(5)));
    view.focus().unwrap();
    assert!(!key(&view, "ArrowRight", 39, true));
    let state = view.state();
    assert_eq!(state.selection, Selection::cell(&state.doc, 2, 7).unwrap());
    assert!(!key(&view, "Delete", 46, false));
    let state = view.state();
    assert!(state.doc.child(0).child(0).text_content().is_empty());
    assert_eq!(state.doc.child(0).child(1).text_content(), "cd");
}

#[wasm_bindgen_test]
fn flushing_keeps_cell_selection_until_the_native_range_changes() {
    let view = mount(None, None);
    view.focus().unwrap();
    let state = view.state();
    let cells = Selection::cell(&state.doc, 2, 7).unwrap();
    let mut transaction = state.tr();
    transaction.set_selection(cells.clone());
    assert!(view.dispatch(transaction).unwrap());

    view.flush_dom_changes().unwrap();
    view.flush_dom_changes().unwrap();
    assert_eq!(view.state().selection, cells);

    let (node, offset) = view.dom_at_pos(21, 1).unwrap();
    let native = document().get_selection().unwrap().unwrap();
    native.collapse_with_offset(Some(&node), offset).unwrap();
    view.flush_dom_changes().unwrap();
    assert_eq!(view.state().selection, Selection::cursor(21));
}

#[wasm_bindgen_test]
fn applies_plain_text_clipboard_content_to_each_selected_cell() {
    let view = mount(None, None);
    let state = view.state();
    let mut transaction = state.tr();
    transaction.set_selection(Selection::cell(&state.doc, 2, 7).unwrap());
    assert!(view.dispatch(transaction).unwrap());
    view.focus().unwrap();
    let data = DataTransfer::new().unwrap();
    data.set_data("text/plain", "X").unwrap();
    let init = ClipboardEventInit::new();
    init.set_clipboard_data(Some(&data));
    init.set_cancelable(true);
    let paste = ClipboardEvent::new_with_event_init_dict("paste", &init).unwrap();
    // Firefox can strip the payload when constructing a synthetic clipboard event.
    if paste.clipboard_data().and_then(|data| data.get_data("text/plain").ok()).as_deref() == Some("X") {
        assert!(!view.dom().dispatch_event(&paste).unwrap());
    } else {
        assert!(view.paste_text("X", None).unwrap());
    }
    let state = view.state();
    assert_eq!(state.doc.child(0).child(0).text_content(), "XX");
    assert_eq!(state.doc.child(0).child(1).text_content(), "cd");
}
