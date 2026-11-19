// Ported from prosemirror-view@1.42.9 test/webtest-selection.ts.
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use js_sys::{Function, Reflect};
use prosemirror_model::Node as ModelNode;
use prosemirror_state::{EditorState, Selection};
use prosemirror_test_builder::{builders, test_schema, ChildSpec, TaggedDoc};
use prosemirror_view::browser::{ViewportPoint, ViewportRect};
use prosemirror_view::{BrowserView, BrowserViewProps, Decoration, DecorationSet};
use serde_json::json;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::wasm_bindgen_test;
use web_sys::{HtmlElement, KeyboardEvent, KeyboardEventInit, Node};

use crate::support::{document, mount, TestView};

pub(super) fn block(name: &str, children: Vec<ChildSpec>) -> TaggedDoc {
    builders(test_schema(), HashMap::new()).node(name, None, children).unwrap()
}

pub(super) fn paragraph(text: &str) -> ChildSpec {
    block("paragraph", vec![text.into()]).into()
}

pub(super) fn mark(name: &str, children: Vec<ChildSpec>) -> ChildSpec {
    let attrs = (name == "link").then(|| json!({"href": "foo"}));
    builders(test_schema(), HashMap::new()).mark(name, attrs, children).unwrap()
}

fn image() -> ChildSpec {
    const SRC: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAMAAAAoLQ9TAAAAVFBMVEUAAAD/zE3/zE3/zE3/zE3/zE3/zE3/zE3/zE3/zE3BlS5mRQBtSwSFYA+ddhyRaxW0pILg2sz///9zUAa3jCl6VwprSgPxwEb+y0zgsT60iieYcRk7fYG6AAAACnRSTlMAOYbF8wAUjv+pYbspWwAAAHJJREFUGBkFwQlKQDEUBLBM+z64gPc/pyAK1o5JIEm0HeSRBNDWkh2A7FhSa8FaGuuEBBJyVgCArAAAmaCgkIELLpgyXUFvjk7ZZyfau49O+X39AS/fdKa5ecuX9/bq5Mn+SBJt+/nXPJIA2hrak0TbwT8U8zdOEulMLAAAAABJRU5ErkJggg==";
    builders(test_schema(), HashMap::new())
        .node("image", Some(json!({"src": SRC})), Vec::new()).unwrap().into()
}

fn horizontal_rule() -> ChildSpec {
    block("horizontal_rule", Vec::new()).into()
}

pub(super) fn fixture(doc: TaggedDoc) -> TestView {
    fixture_with_props(doc, BrowserViewProps::default())
}

pub(super) fn fixture_with_props(doc: TaggedDoc, props: BrowserViewProps) -> TestView {
    let view = mount(doc, props);
    let workspace = view.dom().parent_element().unwrap();
    workspace.set_attribute("style", "position:relative;width:36em;padding:1px").unwrap();
    let style = document().create_element("style").unwrap();
    style.set_text_content(Some("#workspace img[src] { min-height:10px } #workspace img[src^='data:'] { width:16px; height:16px }"));
    workspace.append_child(&style).unwrap();
    view
}

pub(super) fn set_selection(view: &BrowserView, selection: Selection) {
    let mut transaction = view.state().tr();
    transaction.set_selection(selection);
    assert!(view.dispatch(transaction).unwrap());
}

fn set_pos(view: &BrowserView, pos: usize) {
    set_selection(view, Selection::near(&view.state().doc.resolve(pos).unwrap(), 1));
}

fn text_node(node: &Node, text: &str) -> Option<Node> {
    if node.node_value().as_deref() == Some(text) {
        return Some(node.clone());
    }
    let children = node.child_nodes();
    (0..children.length()).filter_map(|index| children.item(index))
        .find_map(|child| text_node(&child, text))
}

fn native_selection() -> web_sys::Selection {
    document().get_selection().unwrap().unwrap()
}

fn set_dom_selection(node: &Node, offset: u32) {
    let range = document().create_range().unwrap();
    range.set_end(node, offset).unwrap();
    range.set_start(node, offset).unwrap();
    let selection = native_selection();
    selection.remove_all_ranges().unwrap();
    selection.add_range(&range).unwrap();
}

fn read_dom_text_selection() -> (Node, u32) {
    let selection = native_selection();
    let mut focus = selection.focus_node().unwrap();
    let mut offset = selection.focus_offset();
    while focus.node_type() != Node::TEXT_NODE {
        if let Some(after) = focus.child_nodes().item(offset) {
            focus = after;
            offset = 0;
        } else if let Some(before) = offset.checked_sub(1).and_then(|offset| focus.child_nodes().item(offset)) {
            focus = before;
            offset = if let Some(text) = focus.node_value() { text.encode_utf16().count() as u32 } else { focus.child_nodes().length() };
        } else {
            break;
        }
    }
    (focus, offset)
}

fn all_positions(doc: &ModelNode) -> Vec<usize> {
    fn scan(node: &ModelNode, start: usize, result: &mut Vec<usize>) {
        if node.is_textblock() {
            result.extend(start..=start + node.content().size());
        } else {
            let mut offset = start + 1;
            for child in &node.content().children {
                scan(child, offset, result);
                offset += child.node_size();
            }
        }
    }
    let mut result = Vec::new();
    scan(doc, 0, &mut result);
    result
}

fn arrow(view: &BrowserView, key: &str, code: u32) {
    let options = KeyboardEventInit::new();
    options.set_key(key);
    options.set_key_code(code);
    options.set_bubbles(true);
    options.set_cancelable(true);
    let event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &options).unwrap();
    view.dispatch_event(event.as_ref()).unwrap();
}

const LEFT: (&str, u32) = ("ArrowLeft", 37);
const RIGHT: (&str, u32) = ("ArrowRight", 39);
const UP: (&str, u32) = ("ArrowUp", 38);
const DOWN: (&str, u32) = ("ArrowDown", 40);

fn press(view: &BrowserView, direction: (&str, u32)) {
    arrow(view, direction.0, direction.1);
}

#[wasm_bindgen_test]
fn can_read_the_dom_selection() {
    if !document().has_focus().unwrap() { return }
    let view = fixture(block("doc", vec![
        paragraph("one"), horizontal_rule(), block("blockquote", vec![paragraph("two")]).into(),
    ]));
    let root: Node = view.dom().into();
    let one = text_node(&root, "one").unwrap();
    let two = text_node(&root, "two").unwrap();
    for (node, offset, expected) in [
        (one.clone(), 0, 1), (one.clone(), 1, 2), (one.clone(), 3, 4),
        (one.parent_node().unwrap(), 0, 1), (one.parent_node().unwrap(), 1, 4),
        (two.clone(), 0, 8), (two.clone(), 3, 11), (two.parent_node().unwrap(), 1, 11),
        (root.clone(), 1, 4), (root.clone(), 2, 8), (root, 3, 11),
    ] {
        set_dom_selection(&node, offset);
        view.dom().focus().unwrap();
        view.flush_dom_changes().unwrap();
        let state = view.state();
        let pos = match state.selection { Selection::Node { .. } => state.selection.from(), _ => state.selection.head(&state.doc) };
        assert_eq!(pos, expected);
    }
}

#[wasm_bindgen_test]
fn syncs_dom_selection_with_editor_selection() {
    if !document().has_focus().unwrap() { return }
    let view = fixture(block("doc", vec![
        paragraph("one"), horizontal_rule(), block("blockquote", vec![paragraph("two")]).into(),
    ]));
    let one = text_node(view.dom().as_ref(), "one").unwrap();
    let two = text_node(view.dom().as_ref(), "two").unwrap();
    view.focus().unwrap();
    for (pos, node, offset) in [(1, &one, 0), (2, &one, 1), (4, &one, 3), (8, &two, 0), (10, &two, 2)] {
        set_pos(&view, pos);
        let (native, native_offset) = read_dom_text_selection();
        assert!(native.is_same_node(Some(node)));
        assert_eq!(native_offset, offset);
    }
}

#[wasm_bindgen_test]
fn returns_sensible_screen_coordinates() {
    let view = fixture(block("doc", vec![paragraph("one"), paragraph("two")]));
    let [p00, p01, p03, p10, p13] = [1, 2, 4, 6, 9].map(|pos| view.coords_at_pos(pos, 1).unwrap());
    assert!(p00.bottom > p00.top);
    assert!(p13.bottom > p13.top);
    assert_eq!(p00.top, p01.top);
    assert_eq!(p01.top, p03.top);
    assert_eq!(p00.bottom, p03.bottom);
    assert_eq!(p10.top, p13.top);
    assert!(p01.left > p00.left);
    assert!(p03.left > p01.left);
    assert!(p10.top > p00.top);
    assert!(p13.left > p10.left);
}

#[wasm_bindgen_test]
fn returns_proper_coordinates_in_code_blocks() {
    let view = fixture(block("doc", vec![block("code_block", vec!["a\nb\n".into()]).into()]));
    let [p0, p1, p2, p3, p4] = [1, 2, 3, 4, 5].map(|pos| view.coords_at_pos(pos, 1).unwrap());
    assert_eq!(p0.top, p1.top);
    assert!(p0.left < p1.left);
    assert!(p2.top > p1.top);
    assert_eq!(p2.top, p3.top);
    assert!(p2.left < p3.left);
    assert_eq!(p2.left, p0.left);
    assert!(p4.top > p3.top);
    assert_eq!(p4.left.round(), p2.left.round());
}

#[wasm_bindgen_test]
fn produces_sensible_screen_coordinates_in_corner_cases() {
    let view = fixture(block("doc", vec![block("paragraph", vec![
        "one".into(),
        mark("em", vec!["two".into(), mark("strong", vec!["three".into()]), image()]),
        block("hard_break", Vec::new()).into(),
        mark("code", vec!["foo".into()]),
    ]).into(), paragraph("")]));
    for pos in all_positions(&view.state().doc) {
        let coords = view.coords_at_pos(pos, 1).unwrap();
        let found = view.pos_at_coords(ViewportPoint { top: coords.top + 1.0, left: coords.left }).unwrap().pos;
        assert_eq!(found, pos, "{coords:?}");
        set_pos(&view, pos);
    }
}

#[wasm_bindgen_test]
fn returns_nonzero_height_rectangles_after_leaves() {
    let view = fixture(block("doc", vec![block("paragraph", vec![image()]).into()]));
    let coords = view.coords_at_pos(2, 1).unwrap();
    assert!(coords.bottom - coords.top > 5.0);
}

#[wasm_bindgen_test]
fn produces_horizontal_rectangles_for_positions_between_blocks() {
    let view = fixture(block("doc", vec![
        paragraph("ha"), horizontal_rule(), block("blockquote", vec![paragraph("ba")]).into(),
    ]));
    let [a, b, c, d] = [0, 4, 5, 6].map(|pos| view.coords_at_pos(pos, 1).unwrap());
    let first: HtmlElement = view.dom().first_child().unwrap().dyn_into().unwrap();
    assert_eq!(a.top, a.bottom);
    assert_eq!(a.top, first.get_bounding_client_rect().top());
    assert!(a.left < a.right);
    assert_eq!(b.top, b.bottom);
    assert!(b.top > a.top);
    assert!(b.left < b.right);
    assert_eq!(c.top, c.bottom);
    assert!(c.top > b.top);
    assert_eq!(d.top, d.bottom);
    assert!(d.left < d.right);
    assert!(d.top < view.dom().get_bounding_client_rect().bottom());
}

#[wasm_bindgen_test]
fn produces_sensible_screen_coordinates_around_line_breaks() {
    let view = fixture(block("doc", vec![paragraph("one two three four five-six-seven-eight")]));
    view.dom().style().set_property("width", "4em").unwrap();
    let mut previous: Option<(ViewportRect, ViewportRect)> = None;
    let doc = view.state().doc;
    for pos in all_positions(&doc) {
        let coords = view.coords_at_pos(pos, 1).unwrap();
        let before = view.coords_at_pos(pos, -1).unwrap();
        if let Some((prev_after, prev_before)) = previous {
            let after_space = pos > 0 && doc.text_between(pos - 1, pos) == " ";
            assert!(prev_after.top < coords.top || prev_after.top == coords.top && prev_after.left < coords.left);
            assert!(prev_before.top < before.top || prev_before.top == before.top
                && (prev_before.left < before.left || after_space && prev_before.left == before.left));
        }
        let found = view.pos_at_coords(ViewportPoint { top: coords.top + 1.0, left: coords.left }).unwrap().pos;
        assert_eq!(found, pos);
        previous = Some((coords, before));
    }
}

#[wasm_bindgen_test]
fn can_find_coordinates_on_node_boundaries() {
    let view = fixture(block("doc", vec![block("paragraph", vec![
        "one ".into(), mark("em", vec!["two".into()]), " ".into(),
        mark("em", vec![mark("strong", vec!["three".into()])]),
    ]).into()]));
    let coords: Vec<_> = all_positions(&view.state().doc).into_iter()
        .map(|pos| view.coords_at_pos(pos, 1).unwrap()).collect();
    for pair in coords.windows(2) {
        let [prev, coords] = [pair[0], pair[1]];
        assert!(prev.top < coords.top || (prev.top - coords.top).abs() < 4.0 && prev.left < coords.left);
    }
}

#[wasm_bindgen_test]
fn finds_proper_coordinates_in_rtl_text() {
    let view = fixture(block("doc", vec![paragraph("مرآة نثرية")]));
    view.dom().style().set_property("direction", "rtl").unwrap();
    let coords: Vec<_> = all_positions(&view.state().doc).into_iter()
        .map(|pos| view.coords_at_pos(pos, 1).unwrap()).collect();
    for pair in coords.windows(2) {
        let [prev, coords] = [pair[0], pair[1]];
        assert!(prev.top < coords.top || (prev.top - coords.top).abs() < 4.0 && prev.left > coords.left);
    }
}

#[wasm_bindgen_test]
fn round_trips_screen_coordinates_and_document_positions() {
    let view = fixture(block("doc", vec![
        paragraph("one"), block("blockquote", vec![paragraph("two"), paragraph("three")]).into(),
    ]));
    for pos in [1, 2, 4, 7, 14, 15] {
        let coords = view.coords_at_pos(pos, 1).unwrap();
        let found = view.pos_at_coords(ViewportPoint { top: coords.top + 1.0, left: coords.left }).unwrap().pos;
        assert_eq!(found, pos);
    }
}

#[wasm_bindgen_test]
fn returns_correct_screen_coordinates_for_wrapped_lines() {
    let view = fixture(block("doc", vec![paragraph("")]));
    let top = view.coords_at_pos(1, 1).unwrap();
    let mut pos = 1;
    let mut end = top;
    for _ in 0..100 {
        let mut transaction = view.state().tr();
        transaction.insert_text_at_selection("a bc de fg h");
        assert!(view.dispatch(transaction).unwrap());
        pos += 12;
        end = view.coords_at_pos(pos, 1).unwrap();
        if end.bottom > top.bottom + 4.0 { break }
    }
    let coords = ViewportPoint { left: end.left + 50.0, top: end.top + 5.0 };
    assert_eq!(view.pos_at_coords(coords).unwrap().pos, pos);
}

#[wasm_bindgen_test]
fn makes_arrow_motion_go_through_selectable_inline_nodes() {
    let view = fixture(block("doc", vec![
        block("paragraph", vec!["foo<a>".into(), image(), "bar".into()]).into(),
    ]));
    press(&view, RIGHT);
    assert_eq!(view.state().selection.from(), 4);
    press(&view, RIGHT);
    let state = view.state();
    assert_eq!(state.selection.head(&state.doc), 5);
    assert_eq!(state.selection.anchor(), 5);
    press(&view, LEFT);
    assert_eq!(view.state().selection.from(), 4);
    press(&view, LEFT);
    let state = view.state();
    assert_eq!(state.selection.head(&state.doc), 4);
    assert_eq!(state.selection.anchor(), 4);
}

#[wasm_bindgen_test]
fn makes_arrow_motion_go_through_selectable_block_nodes() {
    let view = fixture(block("doc", vec![
        paragraph("hello<a>"), horizontal_rule(),
        block("bullet_list", vec![block("list_item", vec![paragraph("there")]).into()]).into(),
    ]));
    press(&view, DOWN);
    assert_eq!(view.state().selection.from(), 7);
    set_pos(&view, 11);
    press(&view, UP);
    assert_eq!(view.state().selection.from(), 7);
}

#[wasm_bindgen_test]
fn supports_arrow_motion_through_adjacent_blocks() {
    let view = fixture(block("doc", vec![
        block("blockquote", vec![paragraph("hello<a>")]).into(),
        horizontal_rule(), horizontal_rule(), paragraph("there"),
    ]));
    press(&view, DOWN);
    assert_eq!(view.state().selection.from(), 9);
    press(&view, DOWN);
    assert_eq!(view.state().selection.from(), 10);
    set_pos(&view, 14);
    press(&view, UP);
    assert_eq!(view.state().selection.from(), 10);
    press(&view, UP);
    assert_eq!(view.state().selection.from(), 9);
}

#[wasm_bindgen_test]
fn supports_horizontal_motion_through_blocks() {
    let view = fixture(block("doc", vec![paragraph("foo<a>"), horizontal_rule(), horizontal_rule(), paragraph("bar")]));
    press(&view, RIGHT);
    assert_eq!(view.state().selection.from(), 5);
    press(&view, RIGHT);
    assert_eq!(view.state().selection.from(), 6);
    press(&view, RIGHT);
    let state = view.state();
    assert_eq!(state.selection.head(&state.doc), 8);
    press(&view, LEFT);
    assert_eq!(view.state().selection.from(), 6);
    press(&view, LEFT);
    assert_eq!(view.state().selection.from(), 5);
    press(&view, LEFT);
    let state = view.state();
    assert_eq!(state.selection.head(&state.doc), 4);
}

fn inline_block_inline_fixture() -> TestView {
    fixture(block("doc", vec![
        block("paragraph", vec!["foo".into(), image()]).into(), horizontal_rule(),
        block("paragraph", vec![image(), "bar".into()]).into(),
    ]))
}

#[wasm_bindgen_test]
fn allows_moving_directly_from_an_inline_node_to_a_block_node() {
    let view = inline_block_inline_fixture();
    set_selection(&view, Selection::node(4));
    press(&view, DOWN);
    assert_eq!(view.state().selection.from(), 6);
    set_selection(&view, Selection::node(8));
    press(&view, UP);
    assert_eq!(view.state().selection.from(), 6);
}

#[wasm_bindgen_test]
fn updates_selection_even_if_dom_parameters_look_unchanged() {
    if !document().has_focus().unwrap() { return }
    let view = fixture(block("doc", vec![paragraph("foobar<a>")]));
    view.focus().unwrap();
    let decorations = DecorationSet::create(&view.state().doc, vec![
        Decoration::inline(1, 4, BTreeMap::from([("color".to_owned(), "green".to_owned())]), serde_json::Value::Null),
    ]);
    let decoration_provider = Rc::new(move |_: &EditorState| Some(decorations.clone()));
    view.set_props(|props| props.decorations = vec![decoration_provider.clone()]).unwrap();
    view.set_props(|props| props.decorations.clear()).unwrap();
    view.set_props(|props| props.decorations = vec![decoration_provider]).unwrap();
    let selection = native_selection();
    let range = document().create_range().unwrap();
    range.set_end(&selection.anchor_node().unwrap(), selection.anchor_offset()).unwrap();
    range.set_start(view.dom().as_ref(), 0).unwrap();
    let string: Function = Reflect::get(range.as_ref(), &"toString".into()).unwrap().dyn_into().unwrap();
    assert_eq!(string.call0(range.as_ref()).unwrap().as_string().as_deref(), Some("foobar"));
}

struct RestoreExtend {
    prototype: JsValue,
    extend: JsValue,
}

impl Drop for RestoreExtend {
    fn drop(&mut self) {
        Reflect::set(&self.prototype, &"extend".into(), &self.extend).unwrap();
    }
}

fn failing_extend() -> RestoreExtend {
    let ctor = Reflect::get(web_sys::window().unwrap().as_ref(), &"Selection".into()).unwrap();
    let prototype = Reflect::get(&ctor, &"prototype".into()).unwrap();
    let extend = Reflect::get(&prototype, &"extend".into()).unwrap();
    let restore = RestoreExtend { prototype, extend };
    let throw = Function::new_no_args("throw new DOMException('failed')");
    Reflect::set(&restore.prototype, &"extend".into(), throw.as_ref()).unwrap();
    restore
}

#[wasm_bindgen_test]
fn sets_selection_even_if_selection_extend_throws_dom_exception() {
    let _restore = failing_extend();
    let view = inline_block_inline_fixture();
    set_selection(&view, Selection::node(4));
    press(&view, DOWN);
    assert_eq!(view.state().selection.from(), 6);
}

#[wasm_bindgen_test]
fn range_fallback_preserves_content_of_backward_selection() {
    let view = fixture(block("doc", vec![paragraph("abcd")]));
    view.focus().unwrap();
    let _restore = failing_extend();
    set_selection(&view, Selection::text(4, 2));
    assert_eq!(view.state().selection, Selection::text(4, 2));
    let selected = native_selection().get_range_at(0).unwrap();
    let text = text_node(view.dom().as_ref(), "abcd").unwrap();
    assert!(selected.start_container().unwrap().is_same_node(Some(&text)));
    assert!(selected.end_container().unwrap().is_same_node(Some(&text)));
    assert_eq!(selected.start_offset().unwrap(), 1);
    assert_eq!(selected.end_offset().unwrap(), 3);
}

#[wasm_bindgen_test]
fn does_not_put_cursor_after_br_hack_nodes() {
    if !document().has_focus().unwrap() { return }
    let view = fixture(block("doc", vec![paragraph("")]));
    view.focus().unwrap();
    assert_eq!(native_selection().focus_offset(), 0);
}
