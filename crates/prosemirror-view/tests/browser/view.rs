//! Direct translations of prosemirror-view@1.42.9/test/webtest-view.ts (MIT).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use prosemirror_model::Schema;
use prosemirror_schema_basic::nodes;
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_test_builder::{doc, hr, p, strong_text, BuilderContext};
use prosemirror_view::browser::{BrowserView, BrowserViewProps, Mount};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::wasm_bindgen_test;
use web_sys::{HtmlElement, Node};

use crate::support::{document, mount, state};

#[wasm_bindgen_test]
fn can_mount_an_existing_node() {
    let dom = document().create_element("article").unwrap().dyn_into::<HtmlElement>().unwrap();
    let space = document().body().unwrap();
    space.append_child(&dom).unwrap();
    let view = BrowserView::new(Mount::Existing(dom.clone()), state(doc!(p!("hi"))), BrowserViewProps::default()).unwrap();
    assert!((&view.dom() as &Node).is_same_node(Some(&dom)));
    assert_eq!(dom.content_editable(), "true");
    assert_eq!(dom.first_child().unwrap().node_name(), "P");
    view.destroy();
    assert_eq!(dom.content_editable(), "inherit");
    space.remove_child(&dom).unwrap();
}

#[wasm_bindgen_test]
fn reflects_the_current_state_in_props() {
    let view = mount(doc!(p!()), BrowserViewProps::default());
    assert!(view.state().doc.ptr_eq(&view.props().state.doc));
    let mut tr = view.state().tr();
    tr.insert_text_at_selection("x");
    view.dispatch(tr).unwrap();
    assert!(view.state().doc.ptr_eq(&view.props().state.doc));
}

#[wasm_bindgen_test]
fn can_update_props_with_set_props() {
    let props = BrowserViewProps { scroll_threshold: Some(100.0), ..BrowserViewProps::default() };
    let view = mount(doc!(p!()), props);
    let state = view.state();
    let mut tr = state.tr();
    tr.insert_text_at_selection("y");
    view.update(state.apply(&tr), BrowserViewProps {
        scroll_threshold: None, scroll_margin: Some(10.0), ..view.props().props
    }).unwrap();
    assert_eq!(view.state().doc.content().size(), 3);
    assert_eq!(view.props().scroll_threshold, None);
    assert_eq!(view.props().scroll_margin, Some(10.0));
}

#[wasm_bindgen_test]
fn can_update_with_a_state_using_a_different_schema() {
    let schema = Schema::new(nodes(), Vec::new());
    let view = mount(doc!(p!(strong_text("foo"))), BrowserViewProps::default());
    let state = EditorState::create(EditorStateConfig {
        schema, doc: None, selection: None, stored_marks: None, plugins: Vec::new(),
    });
    view.update_state(state).unwrap();
    assert!(view.dom().query_selector("strong").unwrap().is_none());
}

#[wasm_bindgen_test]
fn calls_handle_scroll_to_selection_when_appropriate() {
    let count = Rc::new(Cell::new(0));
    let calls = count.clone();
    let view = mount(doc!(p!("foo")), BrowserViewProps {
        handle_scroll_to_selection: Some(Rc::new(move |_| {
            calls.set(calls.get() + 1);
            false
        })),
        ..BrowserViewProps::default()
    });
    view.focus().unwrap();
    let mut tr = view.state().tr();
    tr.scroll_into_view();
    view.dispatch(tr).unwrap();
    assert_eq!(count.get(), 1);
}

#[wasm_bindgen_test]
fn can_be_queried_for_the_dom_position_at_a_doc_position() {
    let fixture = doc!(|ctx: &mut BuilderContext| ctx.wrap_block("bullet_list", |ctx| {
        vec![ctx.wrap_block("list_item", |ctx| vec![p!(strong_text("foo"))(ctx)])]
    }));
    let view = mount(fixture, BrowserViewProps::default());
    let (text, offset) = view.dom_at_pos(4, 0).unwrap();
    assert_eq!(offset, 1);
    assert_eq!(text.node_value().as_deref(), Some("foo"));
    let (before, offset) = view.dom_at_pos(1, 0).unwrap();
    assert_eq!(offset, 0);
    assert_eq!(before.node_name(), "UL");
    let (after, offset) = view.dom_at_pos(7, 0).unwrap();
    assert_eq!(offset, 1);
    assert_eq!(after.node_name(), "LI");
}

#[wasm_bindgen_test]
fn can_bias_dom_position_queries_to_enter_nodes() {
    let fixture = doc!(p!(
        |ctx: &mut BuilderContext| {
            let em = prosemirror_test_builder::test_schema().mark_type("em").unwrap().create(None);
            let strong = prosemirror_test_builder::test_schema().mark_type("strong").unwrap().create(None);
            ctx.parse_text("a", vec![em, strong])
        },
        prosemirror_test_builder::em_text("b"),
        "c"
    ));
    let view = mount(fixture, BrowserViewProps::default());
    for (pos, side, expected) in [
        (1, 0, "P@0"), (1, -1, "P@0"), (1, 1, "a@0"),
        (2, -1, "a@1"), (2, 0, "EM@1"), (2, 1, "b@0"),
        (3, -1, "b@1"), (3, 0, "P@1"), (3, 1, "c@0"),
        (4, -1, "c@1"), (4, 0, "P@2"), (4, 1, "P@2"),
    ] {
        let (node, offset) = view.dom_at_pos(pos, side).unwrap();
        let name = node.node_value().unwrap_or_else(|| node.node_name());
        assert_eq!(format!("{name}@{offset}"), expected, "position {pos}, side {side}");
    }
}

#[wasm_bindgen_test]
fn can_be_queried_for_a_nodes_dom_representation() {
    let view = mount(doc!(p!("foo"), hr()), BrowserViewProps::default());
    assert_eq!(view.node_dom(0).unwrap().node_name(), "P");
    assert_eq!(view.node_dom(5).unwrap().node_name(), "HR");
    assert!(view.node_dom(3).is_none());
}

#[wasm_bindgen_test]
fn can_map_dom_positions_to_doc_positions() {
    let view = mount(doc!(p!("foo"), hr()), BrowserViewProps::default());
    let dom = view.dom();
    assert_eq!(view.pos_at_dom(&dom.first_child().unwrap().first_child().unwrap(), 2, -1), Some(3));
    assert_eq!(view.pos_at_dom(&dom, 1, -1), Some(5));
    assert_eq!(view.pos_at_dom(&dom, 2, -1), Some(6));
    assert_eq!(view.pos_at_dom(&dom.last_child().unwrap(), 0, -1), Some(5));
    assert_eq!(view.pos_at_dom(&dom.last_child().unwrap(), 0, 1), Some(6));
}

#[wasm_bindgen_test]
fn passes_the_view_to_dispatch_transaction() {
    let dispatch_view = Rc::new(RefCell::new(None));
    let recorded = dispatch_view.clone();
    let view = BrowserView::new(Mount::Detached, state(doc!(p!("hi"))), BrowserViewProps {
        dispatch_transaction: Some(Rc::new(move |view, _| *recorded.borrow_mut() = Some(view.clone()))),
        ..BrowserViewProps::default()
    }).unwrap();
    let mut tr = view.state().tr();
    tr.insert_text_at_selection("x");
    view.dispatch(tr).unwrap();
    assert!(view.same_view(dispatch_view.borrow().as_ref().unwrap()));
    assert_eq!(view.state().doc.text_between(1, 3), "hi");
    dispatch_view.borrow_mut().take();
}

#[wasm_bindgen_test]
fn parses_html_clipboard_and_serializes_the_inserted_slice() {
    let view = mount(doc!(p!("a<a>z")), BrowserViewProps::default());
    assert!(view.paste_html("<strong>b</strong>", None).unwrap());
    assert_eq!(view.state().doc, doc!(p!("a", strong_text("b"), "z")).node);
    let state = view.state();
    let slice = Selection::text(2, 3).content(&state.doc).unwrap();
    let copied = view.serialize_for_clipboard(slice).unwrap();
    assert_eq!(copied.text, "b");
    assert!(copied.dom.inner_html().contains("<strong"));
}
