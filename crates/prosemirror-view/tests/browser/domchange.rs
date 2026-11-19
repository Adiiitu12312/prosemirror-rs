//! Port of prosemirror-view 1.42.9, test/webtest-domchange.ts.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use prosemirror_model::Node as ModelNode;
use prosemirror_state::Selection;
use prosemirror_test_builder::{builders, test_schema, ChildSpec, TaggedDoc};
use prosemirror_transform::Step;
use prosemirror_view::browser::{BrowserView, BrowserViewProps};
use serde_json::json;
use wasm_bindgen::{closure::Closure, JsCast};
use wasm_bindgen_test::wasm_bindgen_test;
use web_sys::{Element, Event, HtmlElement, KeyboardEvent, KeyboardEventInit, MutationObserver, MutationObserverInit, Node};

use super::support::{document, mount, state};

pub(super) fn make_node(name: &str, children: Vec<ChildSpec>) -> TaggedDoc {
    builders(test_schema(), Default::default()).node(name, None, children).unwrap()
}

pub(super) fn mark(name: &str, children: Vec<ChildSpec>) -> ChildSpec {
    let attrs = (name == "link").then(|| json!({"href": "foo"}));
    builders(test_schema(), Default::default()).mark(name, attrs, children).unwrap()
}

macro_rules! node {
    ($kind:expr; $($child:expr),* $(,)?) => {
        make_node($kind, vec![$(ChildSpec::from($child)),*])
    };
}

macro_rules! p {
    ($($child:expr),* $(,)?) => { node!("paragraph"; $($child),*) };
}

macro_rules! doc {
    ($($child:expr),* $(,)?) => { node!("doc"; $($child),*) };
}

const IMAGE_SOURCE: &str = "data:image/gif;base64,R0lGODlhAQABAAAAACH5BAEKAAEALAAAAAABAAEAAAICTAEAOw==";

fn img() -> ModelNode {
    test_schema().node("image", Some(json!({"src": IMAGE_SOURCE})), None, None).unwrap()
}

fn br() -> ModelNode {
    test_schema().node("hard_break", None, None, None).unwrap()
}

pub(super) fn find_text(root: &Node, text: &str) -> Node {
    fn search(root: &Node, text: &str) -> Option<Node> {
        if root.node_type() == Node::TEXT_NODE && root.node_value().as_deref() == Some(text) {
            return Some(root.clone());
        }
        let children = root.child_nodes();
        (0..children.length()).filter_map(|index| children.item(index))
            .find_map(|child| search(&child, text))
    }
    search(root, text).unwrap_or_else(|| panic!("text node {text:?} not found"))
}

fn maybe_text(root: &Node, text: &str) -> bool {
    if root.node_type() == Node::TEXT_NODE {
        return root.node_value().as_deref() == Some(text);
    }
    let children = root.child_nodes();
    (0..children.length()).filter_map(|index| children.item(index))
        .any(|child| maybe_text(&child, text))
}

pub(super) fn set_selection(anchor: &Node, offset: u32) {
    document().get_selection().unwrap().unwrap()
        .collapse_with_offset(Some(anchor), offset).unwrap();
}

fn replace_text(view: &BrowserView, text: &str, replacement: &str) {
    find_text(view.dom().as_ref(), text).set_node_value(Some(replacement));
}

fn element(view: &BrowserView, selector: &str) -> Element {
    view.dom().query_selector(selector).unwrap().unwrap()
}

fn flushed(view: &BrowserView, expected: TaggedDoc) {
    view.flush_dom_changes().unwrap();
    assert_eq!(view.state().doc, expected.node);
}

macro_rules! simple_text_edit {
    ($name:ident, $before:expr, $after:expr) => {
        #[wasm_bindgen_test]
        fn $name() {
            let view = mount(doc!(p!($before)), BrowserViewProps::default());
            replace_text(&view, $before, $after);
            flushed(&view, doc!(p!($after)));
        }
    };
}

simple_text_edit!(notices_when_text_is_added, "hello", "heLllo");
simple_text_edit!(notices_when_text_is_removed, "hello", "heo");
simple_text_edit!(handles_ambiguous_changes, "hello", "helo");
simple_text_edit!(supports_inserting_repeated_text, "hello", "helhello");
simple_text_edit!(reads_simple_composition, "hello", "hellox");

#[wasm_bindgen_test]
fn respects_stored_marks() {
    let view = mount(doc!(p!("hello")), BrowserViewProps::default());
    let mut tr = view.state().tr();
    tr.add_stored_mark(test_schema().mark_type("em").unwrap().create(None));
    view.dispatch(tr).unwrap();
    replace_text(&view, "hello", "helloo");
    flushed(&view, doc!(p!("hello", mark("em", vec!["o".into()]))));
}

#[wasm_bindgen_test]
fn adds_a_node() {
    let view = mount(doc!(p!("hello")), BrowserViewProps::default());
    let text = find_text(view.dom().as_ref(), "hello");
    text.parent_node().unwrap().append_child(&document().create_text_node("!")).unwrap();
    flushed(&view, doc!(p!("hello!")));
}

#[wasm_bindgen_test]
fn removes_a_text_node() {
    let view = mount(doc!(p!("hello")), BrowserViewProps::default());
    let text = find_text(view.dom().as_ref(), "hello");
    text.parent_node().unwrap().remove_child(&text).unwrap();
    flushed(&view, doc!(p!()));
}

fn add_paragraph_before(view: &BrowserView, text: &str) {
    let para = document().create_element("p").unwrap();
    view.dom().insert_before(&para, view.dom().first_child().as_ref()).unwrap();
    para.append_child(&document().create_text_node(text)).unwrap();
}

#[wasm_bindgen_test]
fn adds_a_paragraph() {
    let view = mount(doc!(p!("hello")), BrowserViewProps::default());
    add_paragraph_before(&view, "hey");
    flushed(&view, doc!(p!("hey"), p!("hello")));
}

#[wasm_bindgen_test]
fn supports_duplicating_a_paragraph() {
    let view = mount(doc!(p!("hello")), BrowserViewProps::default());
    add_paragraph_before(&view, "hello");
    flushed(&view, doc!(p!("hello"), p!("hello")));
}

fn key_props(key: &str) -> (BrowserViewProps, Rc<Cell<bool>>) {
    let pressed = Rc::new(Cell::new(false));
    let handled = Rc::clone(&pressed);
    let key = key.to_owned();
    (BrowserViewProps {
        handle_key_down: Some(Rc::new(move |_, event| {
            let matched = event.key() == key;
            if matched { handled.set(true); }
            matched
        })),
        ..Default::default()
    }, pressed)
}

#[wasm_bindgen_test]
fn detects_an_enter_press() {
    let (props, pressed) = key_props("Enter");
    let view = mount(doc!(node!("blockquote"; p!("foo"), p!("<a>"))), props);
    element(&view, "blockquote").append_child(&document().create_element("p").unwrap()).unwrap();
    view.flush_dom_changes().unwrap();
    assert!(pressed.get());
}

#[wasm_bindgen_test]
fn detects_enter_creating_a_different_block() {
    let (props, pressed) = key_props("Enter");
    let view = mount(doc!(node!("heading"; "<a>")), props);
    let h2 = document().create_element("h2").unwrap();
    view.dom().append_child(&h2).unwrap();
    set_selection(&h2, 0);
    view.flush_dom_changes().unwrap();
    assert!(pressed.get());
}

fn join_paragraphs(view: &BrowserView) {
    view.dom().remove_child(&view.dom().last_child().unwrap()).unwrap();
    view.dom().first_child().unwrap().set_text_content(Some("foobar"));
    view.flush_dom_changes().unwrap();
}

#[wasm_bindgen_test]
fn detects_a_simple_backspace_press() {
    let (props, pressed) = key_props("Backspace");
    let view = mount(doc!(p!("foo"), p!("<a>bar")), props);
    join_paragraphs(&view);
    assert!(pressed.get());
}

#[wasm_bindgen_test]
fn detects_a_complex_backspace_press() {
    let (props, pressed) = key_props("Backspace");
    let view = mount(doc!(node!("blockquote"; node!("blockquote"; p!("foo")), p!("<a>", br(), "bar"))), props);
    let outer = view.dom().first_child().unwrap();
    outer.remove_child(&outer.last_child().unwrap()).unwrap();
    let para: Element = outer.first_child().unwrap().first_child().unwrap().dyn_into().unwrap();
    para.set_inner_html("foo<br>bar");
    view.flush_dom_changes().unwrap();
    assert!(pressed.get());
}

#[wasm_bindgen_test]
fn does_not_route_delete_as_backspace() {
    let (props, pressed) = key_props("Backspace");
    let view = mount(doc!(p!("foo<a>"), p!("bar")), props);
    join_paragraphs(&view);
    assert!(!pressed.get());
}

#[wasm_bindgen_test]
fn adjusts_selection() {
    let view = mount(doc!(p!("abc<a>")), BrowserViewProps::default());
    let text = find_text(view.dom().as_ref(), "abc");
    text.set_node_value(Some("abcd"));
    set_selection(&text, 3);
    flushed(&view, doc!(p!("abcd")));
    assert_eq!(view.state().selection, Selection::cursor(4));
}

#[wasm_bindgen_test]
fn handles_splitting_of_a_textblock() {
    let view = mount(doc!(node!("heading"; "abc"), p!("defg<a>")), BrowserViewProps::default());
    let para = element(&view, "p");
    let split: Element = para.clone_node().unwrap().dyn_into().unwrap();
    para.parent_node().unwrap().append_child(&split).unwrap();
    split.set_inner_html("fg");
    find_text(&para, "defg").set_node_value(Some("dexy"));
    set_selection(&split.first_child().unwrap(), 1);
    flushed(&view, doc!(node!("heading"; "abc"), p!("dexy"), p!("fg")));
    assert_eq!(view.state().selection.anchor(), 13);
}

#[wasm_bindgen_test]
fn handles_a_deep_split_of_nodes() {
    let view = mount(doc!(node!("blockquote"; p!("ab<a>cd"))), BrowserViewProps::default());
    let quote = element(&view, "blockquote");
    let quote2 = quote.clone_node_with_deep(true).unwrap();
    view.dom().append_child(&quote2).unwrap();
    find_text(&quote, "abcd").set_node_value(Some("abx"));
    let second = find_text(&quote2, "abcd");
    second.set_node_value(Some("cd"));
    set_selection(&second.parent_node().unwrap(), 0);
    flushed(&view, doc!(node!("blockquote"; p!("abx")), node!("blockquote"; p!("cd"))));
    assert_eq!(view.state().selection.anchor(), 9);
}

#[wasm_bindgen_test]
fn deletes_the_third_instance_of_a_character() {
    let view = mount(doc!(p!("foo xxx<a> bar")), BrowserViewProps::default());
    replace_text(&view, "foo xxx bar", "foo xx bar");
    flushed(&view, doc!(p!("foo xx bar")));
}

fn marked_document(text: &str) -> TaggedDoc {
    doc!(p!("a", mark("em", vec!["b".into(), img().into(), mark("strong", vec![text.into()])]), "e"))
}

#[wasm_bindgen_test]
fn deletes_text_inside_markup() {
    let view = mount(marked_document("cd<a>"), BrowserViewProps::default());
    replace_text(&view, "cd", "c");
    flushed(&view, marked_document("c"));
}

#[wasm_bindgen_test]
fn recognizes_typing_inside_markup() {
    let view = mount(marked_document("cd<a>"), BrowserViewProps::default());
    replace_text(&view, "cd", "cdxy");
    flushed(&view, marked_document("cdxy"));
}

#[wasm_bindgen_test]
fn resolves_ambiguous_text_input() {
    let view = mount(doc!(p!("fo<a>o")), BrowserViewProps::default());
    let mut tr = view.state().tr();
    tr.add_stored_mark(test_schema().mark_type("strong").unwrap().create(None));
    view.dispatch(tr).unwrap();
    replace_text(&view, "foo", "fooo");
    flushed(&view, doc!(p!("fo", mark("strong", vec!["o".into()]), "o")));
}

#[wasm_bindgen_test]
fn keeps_the_text_node_typed_into() {
    let view = mount(doc!(p!("fo<a>o")), BrowserViewProps::default());
    let original = find_text(view.dom().as_ref(), "foo");
    original.set_node_value(Some("fojo"));
    let mutated = Rc::new(Cell::new(false));
    let callback = {
        let mutated = Rc::clone(&mutated);
        Closure::<dyn FnMut(js_sys::Array, MutationObserver)>::new(move |_, _| mutated.set(true))
    };
    let observer = MutationObserver::new(callback.as_ref().unchecked_ref()).unwrap();
    let options = MutationObserverInit::new();
    options.set_subtree(true);
    options.set_character_data(true);
    options.set_child_list(true);
    observer.observe_with_options(view.dom().as_ref(), &options).unwrap();
    flushed(&view, doc!(p!("fojo")));
    assert!(!mutated.get());
    observer.disconnect();
    assert!(original.is_same_node(Some(&find_text(view.dom().as_ref(), "fojo"))));
}

#[wasm_bindgen_test]
fn reads_text_typed_into_an_empty_paragraph() {
    let view = mount(doc!(p!("<a>")), BrowserViewProps::default());
    element(&view, "p").set_text_content(Some("i"));
    flushed(&view, doc!(p!("i")));
}

#[wasm_bindgen_test]
fn ignores_a_placeholder_break() {
    let view = mount(doc!(p!("i<a>")), BrowserViewProps::default());
    element(&view, "p").set_inner_html("<br>");
    flushed(&view, doc!(p!()));
}

fn reject_changes() -> BrowserViewProps {
    BrowserViewProps { dispatch_transaction: Some(Rc::new(|_, _| {})), ..Default::default() }
}

#[wasm_bindgen_test]
fn restores_text_when_input_is_ignored() {
    let view = mount(doc!(p!("foo")), reject_changes());
    replace_text(&view, "foo", "food");
    view.flush_dom_changes().unwrap();
    assert_eq!(view.dom().text_content().as_deref(), Some("foo"));
}

#[wasm_bindgen_test]
fn restores_structure_when_input_is_ignored() {
    let view = mount(doc!(p!("foo", br(), "bar")), reject_changes());
    let para = element(&view, "p");
    para.replace_child(&document().create_element("img").unwrap(), &para.last_child().unwrap()).unwrap();
    view.flush_dom_changes().unwrap();
    assert_eq!(view.dom().text_content().as_deref(), Some("foobar"));
}

#[wasm_bindgen_test]
fn aborts_dom_change_after_setting_an_incompatible_state() {
    let view = mount(doc!(p!("abcde")), BrowserViewProps::default());
    replace_text(&view, "abcde", "xabcde");
    view.dispatch_event(&Event::new("input").unwrap()).unwrap();
    view.update_state(state(doc!(p!("uvw")))).unwrap();
    flushed(&view, doc!(p!("uvw")));
}

fn track_steps() -> (BrowserViewProps, Rc<RefCell<Vec<Step>>>) {
    let steps = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&steps);
    (BrowserViewProps {
        dispatch_transaction: Some(Rc::new(move |view, tr| {
            *recorded.borrow_mut() = tr.steps().to_vec();
            view.update_state(view.state().apply(&tr)).unwrap();
        })),
        ..Default::default()
    }, steps)
}

#[wasm_bindgen_test]
fn recognizes_a_mark_change() {
    let (props, steps) = track_steps();
    let view = mount(doc!(p!("one")), props);
    element(&view, "p").set_inner_html("<b>one</b>");
    flushed(&view, doc!(p!(mark("strong", vec!["one".into()]))));
    assert!(matches!(steps.borrow().as_slice(), [Step::AddMark { .. }]));
}

#[wasm_bindgen_test]
fn preserves_marks_on_deletion() {
    let view = mount(doc!(p!("one", mark("em", vec!["x<a>".into()]))), BrowserViewProps::default());
    let em: HtmlElement = element(&view, "em").dyn_into().unwrap();
    em.set_inner_text("");
    view.dispatch_event(&Event::new("input").unwrap()).unwrap();
    view.flush_dom_changes().unwrap();
    let mut tr = view.state().tr();
    tr.insert_text_at_selection("y");
    view.dispatch(tr).unwrap();
    assert_eq!(view.state().doc, doc!(p!("one", mark("em", vec!["y".into()]))).node);
}

#[wasm_bindgen_test]
fn handles_deleted_content_dom() {
    let view = mount(doc!(p!("one"), node!("code_block"; "two<a>")), BrowserViewProps::default());
    let code: HtmlElement = element(&view, "pre").dyn_into().unwrap();
    code.set_inner_text("");
    view.dispatch_event(&Event::new("input").unwrap()).unwrap();
    flushed(&view, doc!(p!("one"), node!("code_block";)));
    let state = view.state();
    assert_eq!(state.selection.head(&state.doc), 6);
}

#[wasm_bindgen_test]
fn handles_replaced_content_dom() {
    let view = mount(doc!(p!("one"), node!("code_block"; "two<a>")), BrowserViewProps::default());
    element(&view, "pre").set_inner_html("<font>q</font>");
    view.dispatch_event(&Event::new("input").unwrap()).unwrap();
    flushed(&view, doc!(p!("one"), node!("code_block"; "q")));
}

fn inside_marks_edit(original: &str, replacement: &str, expected: TaggedDoc) {
    let view = mount(doc!(p!("foo", mark("em", vec!["bar".into()]), mark("strong", vec!["baz".into()]))), BrowserViewProps::default());
    let bar = find_text(view.dom().as_ref(), "bar");
    let preceding_text = find_text(view.dom().as_ref(), "foo");
    replace_text(&view, original, replacement);
    flushed(&view, expected);
    assert!(bar.parent_node().is_some_and(|node| view.dom().contains(Some(&node))));
    assert!(preceding_text.parent_node().is_some_and(|node| view.dom().contains(Some(&node))));
}

#[wasm_bindgen_test]
fn retains_marked_content_when_typing_in_front() {
    inside_marks_edit("foo", "froo", doc!(p!("froo", mark("em", vec!["bar".into()]), mark("strong", vec!["baz".into()]))));
}

#[wasm_bindgen_test]
fn retains_other_content_when_typing_inside_a_mark() {
    inside_marks_edit("bar", "baar", doc!(p!("foo", mark("em", vec!["baar".into()]), mark("strong", vec!["baz".into()]))));
}

#[wasm_bindgen_test]
fn maps_coordinates_after_pending_input() {
    let view = mount(doc!(p!("foo")), BrowserViewProps::default());
    view.dispatch_event(&Event::new("input").unwrap()).unwrap();
    let mut tr = view.state().tr();
    tr.insert_text_at_selection("more text");
    view.dispatch(tr).unwrap();
    assert!(view.coords_at_pos(13, 1).is_some());
}

#[wasm_bindgen_test]
fn reads_text_added_at_the_start_of_a_mark_cursor_wrapper() {
    let view = mount(doc!(p!(mark("strong", vec![mark("link", vec!["foo<a>".into()]), "bar".into()]))), BrowserViewProps::default());
    replace_text(&view, "foo", "fooxy");
    flushed(&view, doc!(p!(mark("strong", vec![mark("link", vec!["foo".into()]), "xybar".into()]))));
}

#[wasm_bindgen_test]
fn removes_text_of_a_cursor_wrapper_when_it_remains_valid() {
    let view = mount(doc!(p!(mark("link", vec![mark("strong", vec!["foo<a>".into()]), "bar".into()]))), BrowserViewProps::default());
    replace_text(&view, "foo", "fooq");
    flushed(&view, doc!(p!(mark("link", vec![mark("strong", vec!["fooq".into()]), "bar".into()]))));
    assert!(!maybe_text(view.dom().as_ref(), "\u{feff}q"));
}

fn backspace(view: &BrowserView) {
    let options = KeyboardEventInit::new();
    options.set_key("Backspace");
    options.set_key_code(8);
    let event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &options).unwrap();
    view.dispatch_event(&event).unwrap();
}

fn assert_replace(steps: &[Step], start: usize, end: usize, text: Option<&str>) {
    let [Step::Replace { from, to, slice, .. }] = steps else { panic!("expected one replace step, got {steps:?}") };
    assert_eq!((*from, *to), (start, end));
    if let Some(text) = text {
        assert_eq!(slice.content.first_child().and_then(ModelNode::text), Some(text));
    }
}

macro_rules! deletion_step {
    ($name:ident, $source:literal, $before:literal, $after:literal, $backspace:literal, $from:literal, $to:literal) => {
        #[wasm_bindgen_test]
        fn $name() {
            let (props, steps) = track_steps();
            let view = mount(doc!(p!($source)), props);
            if $backspace { backspace(&view); }
            replace_text(&view, $before, $after);
            view.flush_dom_changes().unwrap();
            assert_replace(&steps.borrow(), $from, $to, None);
        }
    };
}

deletion_step!(distinguishes_backspace_from_delete, "a<a>a", "aa", "a", true, 1, 2);
deletion_step!(disambiguates_multi_character_backspace, "foo<a>foo", "foofoo", "foo", true, 1, 4);
deletion_step!(distinguishes_delete_from_backspace, "a<a>a", "aa", "a", false, 2, 3);
deletion_step!(distinguishes_multi_character_delete, "one foo<a>foo three", "one foofoo three", "one foo three", false, 8, 11);
deletion_step!(creates_right_step_for_ambiguous_selection_deletion, "la<a>la<b>la", "lalala", "lala", true, 3, 5);

#[wasm_bindgen_test]
fn covers_entire_selection_for_a_partially_matching_replacement() {
    let (props, steps) = track_steps();
    let view = mount(doc!(p!("one <a>two<b> three")), props);
    replace_text(&view, "one two three", "one t three");
    view.flush_dom_changes().unwrap();
    assert_replace(&steps.borrow(), 5, 8, Some("t"));
    let mut tr = view.state().tr();
    tr.set_selection(Selection::text(7, 12));
    view.dispatch(tr).unwrap();
    replace_text(&view, "one t three", "one t e");
    view.flush_dom_changes().unwrap();
    assert_replace(&steps.borrow(), 7, 12, Some("e"));
}

#[wasm_bindgen_test]
fn does_not_split_surrogate_pairs_when_inserting() {
    let (props, steps) = track_steps();
    let view = mount(doc!(p!("👽<a>")), props);
    replace_text(&view, "👽", "💎👽");
    view.flush_dom_changes().unwrap();
    assert_replace(&steps.borrow(), 1, 1, Some("💎"));
}

#[wasm_bindgen_test]
fn does_not_split_surrogate_pairs_when_overwriting() {
    let (props, steps) = track_steps();
    let view = mount(doc!(p!("𝛾<a>")), props);
    replace_text(&view, "𝛾", "𝛽");
    view.flush_dom_changes().unwrap();
    assert_replace(&steps.borrow(), 1, 3, Some("𝛽"));
}
