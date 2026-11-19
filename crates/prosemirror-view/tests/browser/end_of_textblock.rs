// Ported from prosemirror-view@1.42.9 test/webtest-endOfTextblock.ts.
use std::collections::HashMap;
use std::rc::Rc;

use js_sys::Reflect;
use prosemirror_state::{EditorState, Selection};
use prosemirror_test_builder::test_schema;
use prosemirror_view::browser::{TextblockDirection, WidgetView};
use prosemirror_view::{BrowserView, BrowserViewProps, Decoration, DecorationSet};
use serde_json::Value;
use wasm_bindgen_test::wasm_bindgen_test;

use crate::selection::{block, fixture, fixture_with_props, mark, paragraph, set_selection};
use crate::support::document;

use TextblockDirection::{Backward, Down, Forward, Left, Right, Up};

fn assert_horizontal(view: &BrowserView, expected: [bool; 4]) {
    assert_eq!([Left, Backward, Right, Forward].map(|dir| view.end_of_textblock(dir, None)), expected);
}

fn assert_vertical(view: &BrowserView, up: bool, down: bool) {
    assert_eq!([Up, Down].map(|dir| view.end_of_textblock(dir, None)), [up, down]);
}

fn supports_selection_modify() -> bool {
    let selection = document().get_selection().unwrap().unwrap();
    Reflect::has(selection.as_ref(), &"modify".into()).unwrap()
}

#[wasm_bindgen_test]
fn works_at_the_left_side_of_a_textblock() {
    let view = fixture(block("doc", vec![paragraph("a"), paragraph("<a>bcd")]));
    assert_horizontal(&view, [true, true, false, false]);
}

#[wasm_bindgen_test]
fn works_at_the_right_side_of_a_textblock() {
    let view = fixture(block("doc", vec![paragraph("abc<a>"), paragraph("d")]));
    assert_horizontal(&view, [false, false, true, true]);
}

#[wasm_bindgen_test]
fn works_in_the_middle_of_a_textblock() {
    let view = fixture(block("doc", vec![paragraph("a<a>b")]));
    assert_horizontal(&view, [false, false, false, false]);
}

#[wasm_bindgen_test]
fn works_at_the_start_of_the_document() {
    let view = fixture(block("doc", vec![paragraph("<a>bcd")]));
    assert_horizontal(&view, [true, true, false, false]);
}

#[wasm_bindgen_test]
fn works_at_the_end_of_the_document() {
    let view = fixture(block("doc", vec![paragraph("abc<a>")]));
    assert_horizontal(&view, [false, false, true, true]);
}

#[wasm_bindgen_test]
fn works_for_vertical_motion_in_a_one_line_block() {
    let view = fixture(block("doc", vec![paragraph("abc<a>")]));
    assert_vertical(&view, true, true);
}

#[wasm_bindgen_test]
fn works_for_vertical_motion_at_the_end_of_a_wrapped_block() {
    let view = fixture(block("doc", vec![paragraph(&format!("{}<a>foo", "foo ".repeat(99)))]));
    assert_vertical(&view, false, true);
}

#[wasm_bindgen_test]
fn works_for_vertical_motion_at_the_start_of_a_wrapped_block() {
    let view = fixture(block("doc", vec![paragraph(&format!("foo <a>{}", "foo ".repeat(99)))]));
    assert_vertical(&view, true, false);
}

#[wasm_bindgen_test]
fn works_for_virtual_motion_when_in_a_mark() {
    let repeated = "foo ".repeat(49);
    let doc = block("doc", vec![block("paragraph", vec![
        mark("link", vec!["fo<a>o ".into()]), repeated.as_str().into(),
        mark("link", vec!["fo<b>o ".into()]), repeated.as_str().into(),
        mark("link", vec!["fo<c>o".into()]),
    ]).into()]);
    let b = doc.tag("b");
    let c = doc.tag("c");
    let view = fixture(doc);
    assert_vertical(&view, true, false);
    set_selection(&view, Selection::cursor(b));
    assert_vertical(&view, false, false);
    set_selection(&view, Selection::cursor(c));
    assert_vertical(&view, false, true);
}

#[wasm_bindgen_test]
fn works_at_the_start_of_an_rtl_block() {
    if !supports_selection_modify() { return }
    let view = fixture(block("doc", vec![paragraph("<a>مرآة")]));
    view.dom().style().set_property("direction", "rtl").unwrap();
    assert_horizontal(&view, [false, true, true, false]);
}

#[wasm_bindgen_test]
fn works_at_the_end_of_an_rtl_block() {
    if !supports_selection_modify() { return }
    let view = fixture(block("doc", vec![paragraph("مرآة<a>")]));
    view.dom().style().set_property("direction", "rtl").unwrap();
    assert_horizontal(&view, [true, false, false, true]);
}

#[wasm_bindgen_test]
fn works_inside_an_rtl_block() {
    if !supports_selection_modify() { return }
    let view = fixture(block("doc", vec![paragraph("مر<a>آة")]));
    view.dom().style().set_property("direction", "rtl").unwrap();
    assert_horizontal(&view, [false, false, false, false]);
}

#[wasm_bindgen_test]
fn works_in_a_bidirectional_block() {
    if !supports_selection_modify() { return }
    let view = fixture(block("doc", vec![paragraph("proseمرآة<a>")]));
    assert!(!view.end_of_textblock(Left, None));
    assert!(!view.end_of_textblock(Backward, None));
    assert!(view.end_of_textblock(Forward, None));
}

#[wasm_bindgen_test]
fn works_in_a_cursor_wrapper() {
    if !supports_selection_modify() { return }
    let view = fixture(block("doc", vec![paragraph("foo<a>")]));
    let mut transaction = view.state().tr();
    transaction.set_stored_marks(Some(vec![test_schema().mark_type("em").unwrap().create(None)]));
    assert!(view.dispatch(transaction).unwrap());
    assert!(!view.end_of_textblock(Backward, None));
}

#[wasm_bindgen_test]
fn works_after_a_widget() {
    if !supports_selection_modify() { return }
    let doc = block("doc", vec![paragraph("fo<a>o")]);
    let widget = document().create_element("span").unwrap();
    widget.set_text_content(Some("!"));
    let decorations = DecorationSet::create(&doc.node, vec![Decoration::widget(3, 1, Value::Null)]);
    let props = BrowserViewProps {
        decorations: vec![Rc::new(move |_: &EditorState| Some(decorations.clone()))],
        widgets: HashMap::from([(1, Rc::new(move |_: &_, _: &_, _: &_, _| Ok(WidgetView::new(widget.clone()))) as _)]),
        ..BrowserViewProps::default()
    };
    let view = fixture_with_props(doc, props);
    assert!(!view.end_of_textblock(Backward, None));
}
