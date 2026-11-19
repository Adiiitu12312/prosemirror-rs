//! Direct translations of prosemirror-view@1.42.9/test/webtest-view.ts (MIT).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use prosemirror_model::{DomParseRule, DomParser, Schema};
use prosemirror_schema_basic::nodes;
use prosemirror_state::{EditorState, EditorStateConfig, Plugin, Selection};
use prosemirror_test_builder::{doc, hr, p, strong_text, BuilderContext};
use prosemirror_view::browser::{BrowserView, BrowserViewProps, Mount};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::wasm_bindgen_test;
use web_sys::{ClipboardEvent, ClipboardEventInit, DataTransfer, Event, EventInit, HtmlElement, KeyboardEvent, KeyboardEventInit};

use crate::support::{document, mount, mount_state, state};

#[wasm_bindgen_test]
fn can_mount_an_existing_node() {
    let dom = document().create_element("article").unwrap().dyn_into::<HtmlElement>().unwrap();
    let space = document().body().unwrap();
    space.append_child(&dom).unwrap();
    let view = BrowserView::new(Mount::Existing(dom.clone()), state(doc!(p!("hi"))), BrowserViewProps::default()).unwrap();
    assert!(view.dom().is_same_node(Some(&dom)));
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
    view.set_props(|props| {
        props.state = state.apply(&tr);
        props.scroll_threshold = None;
        props.scroll_margin = Some(10.0);
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

#[wasm_bindgen_test]
fn routes_native_clipboard_events() {
    let view = mount(doc!(p!("one <a>two<b>")), BrowserViewProps::default());
    let data = DataTransfer::new().unwrap();
    let init = ClipboardEventInit::new();
    init.set_clipboard_data(Some(&data));
    init.set_bubbles(true);
    init.set_cancelable(true);
    let cut = ClipboardEvent::new_with_event_init_dict("cut", &init).unwrap();
    view.dom().dispatch_event(&cut).unwrap();
    assert!(cut.default_prevented());
    let copied = cut.clipboard_data().unwrap();
    assert_eq!(copied.get_data("text/plain").unwrap(), "two");
    assert_eq!(view.state().doc, doc!(p!("one ")).node);
    let paste_data = DataTransfer::new().unwrap();
    paste_data.set_data("text/plain", &copied.get_data("text/plain").unwrap()).unwrap();
    paste_data.set_data("text/html", &copied.get_data("text/html").unwrap()).unwrap();
    init.set_clipboard_data(Some(&paste_data));
    let paste = ClipboardEvent::new_with_event_init_dict("paste", &init).unwrap();
    if paste.clipboard_data().is_none_or(|data| data.get_data("text/plain").unwrap().is_empty()) {
        // Firefox empties ClipboardEventInit data for synthetic paste events.
        let descriptor = js_sys::Object::new();
        js_sys::Reflect::set(descriptor.as_ref(), &"value".into(), paste_data.as_ref()).unwrap();
        js_sys::Reflect::define_property(paste.as_ref(), &"clipboardData".into(), &descriptor).unwrap();
    }
    view.dom().dispatch_event(&paste).unwrap();
    assert!(paste.default_prevented());
    assert_eq!(view.state().doc, doc!(p!("one two")).node);
}

#[wasm_bindgen_test]
fn uses_browser_tag_and_content_selectors_in_clipboard_and_live_dom() {
    let fixture = doc!(p!("before <a>after"));
    let state = state(fixture);
    let mut rule = DomParseRule::tag("p > span[data-note^='live']:first-of-type");
    rule.mark = Some("strong".into());
    rule.content_element = Some(":scope > em:first-child".into());
    let mut rules = DomParser::schema_rules(&state.schema);
    rules.insert(0, rule);
    let parser = Rc::new(DomParser::new(state.schema.clone(), rules));
    let view = mount_state(state, BrowserViewProps {
        dom_parser: Some(parser.clone()),
        clipboard_parser: Some(parser),
        ..BrowserViewProps::default()
    });
    assert!(view.paste_html("<p><span data-note='lively'><em>like</em><i>omit</i></span></p>", None).unwrap());
    assert_eq!(view.state().doc, doc!(p!("before ", strong_text("like"), "after")).node);
    let paragraph = view.dom().first_element_child().unwrap();
    paragraph.set_inner_html("<span data-note='live-edit'><em>edit</em><i>omit</i></span>");
    view.flush_dom_changes().unwrap();
    assert_eq!(view.state().doc, doc!(p!(strong_text("edit"))).node);
}

#[wasm_bindgen_test]
fn routes_new_dom_event_props_and_cleans_up_listeners_on_destroy() {
    let count = Rc::new(Cell::new(0));
    let calls = count.clone();
    let view = mount(doc!(p!("content")), BrowserViewProps::default());
    view.set_props(|props| {
        props.handle_dom_events.insert("example".into(), Rc::new(move |_, event| {
            calls.set(calls.get() + 1);
            event.prevent_default();
            true
        }));
    }).unwrap();
    let init = EventInit::new();
    init.set_bubbles(true);
    init.set_cancelable(true);
    let event = Event::new_with_event_init_dict("example", &init).unwrap();
    let root = view.dom();
    assert!(!root.dispatch_event(&event).unwrap());
    assert_eq!(count.get(), 1);
    view.destroy();
    let event = Event::new_with_event_init_dict("example", &init).unwrap();
    assert!(root.dispatch_event(&event).unwrap());
    assert_eq!(count.get(), 1);
}

#[wasm_bindgen_test]
fn routes_callbacks_in_direct_then_view_then_state_plugin_order() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let capture = |name, handled| {
        let calls = calls.clone();
        Some(Rc::new(move |_: &BrowserView, _: &KeyboardEvent| {
            calls.borrow_mut().push(name);
            handled
        }) as prosemirror_view::browser::ViewEventHandler<KeyboardEvent>)
    };
    let plugin = Plugin::unkeyed(None);
    let key = plugin.key.clone();
    let mut state = state(doc!(p!("text")));
    state.plugins.push(plugin);
    let view = mount_state(state, BrowserViewProps {
        handle_key_down: capture("direct", false),
        plugins: vec![Rc::new(BrowserViewProps {
            handle_key_down: capture("view", false),
            ..BrowserViewProps::default()
        })],
        state_plugins: [(key, Rc::new(BrowserViewProps {
            handle_key_down: capture("state", true),
            ..BrowserViewProps::default()
        }))].into(),
        ..BrowserViewProps::default()
    });
    let init = KeyboardEventInit::new();
    init.set_key("w");
    init.set_bubbles(true);
    init.set_cancelable(true);
    let event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap();
    assert!(!view.dom().dispatch_event(&event).unwrap());
    assert_eq!(*calls.borrow(), ["direct", "view", "state"]);
}
