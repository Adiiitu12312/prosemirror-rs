//! Port of prosemirror-view 1.42.9, test/webtest-composition.ts.

use std::cell::RefCell;
use std::rc::Rc;

use prosemirror_state::EditorState;
use prosemirror_test_builder::{test_schema, ChildSpec};
use prosemirror_view::browser::{BrowserView, BrowserViewProps, WidgetView};
use prosemirror_view::{Decoration, DecorationSet};
use serde_json::{json, Value};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::wasm_bindgen_test;
use web_sys::{CompositionEvent, Node, Text};

use super::domchange::{find_text, make_node, mark, set_selection};
use super::support::{document, mount};

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

fn event(view: &BrowserView, kind: &str) {
    view.dom().dispatch_event(&CompositionEvent::new(kind).unwrap()).unwrap();
}

fn text_len(node: &Node) -> u32 {
    node.unchecked_ref::<Text>().length()
}

fn edit(node: &Node, text: &str, from: u32, to: u32) {
    node.unchecked_ref::<Text>().replace_data(from, to - from, text).unwrap();
    set_selection(node, from + text.encode_utf16().count() as u32);
}

fn append(node: &Node, text: &str) {
    let end = text_len(node);
    edit(node, text, end, end);
}

fn focus() -> (Node, u32) {
    let selection = document().get_selection().unwrap().unwrap();
    (selection.focus_node().expect("composition focus"), selection.focus_offset())
}

#[derive(Default)]
struct Options<'a> {
    has_node: bool,
    cancel: bool,
    end: Option<&'a dyn Fn(&Node)>,
}

fn compose(
    view: &BrowserView,
    start: impl FnOnce() -> Node,
    updates: &[&dyn Fn(&Node)],
    options: Options<'_>,
) {
    view.focus().unwrap();
    event(view, "compositionstart");
    assert!(view.composing());
    let node = start();
    for index in 0..=updates.len() {
        if let Some(update) = index.checked_sub(1).and_then(|index| updates.get(index)) {
            update(&node);
        }
        let (before, offset) = focus();
        view.flush_dom_changes().unwrap();
        if options.cancel && index == updates.len() {
            assert!(view.composition_node().is_none());
        } else {
            assert!(node.parent_node().is_some_and(|parent| view.dom().contains(Some(&parent))),
                "the composing text node was removed");
            let (current, position) = focus();
            assert!(current.is_same_node(Some(&before)), "composition focus changed");
            assert_eq!(position, offset);
            if options.has_node {
                assert!(view.composition_node().is_some());
            }
        }
    }
    event(view, "compositionend");
    if let Some(end) = options.end {
        end(&node);
        view.flush_dom_changes().unwrap();
    }
    view.end_composition().unwrap();
    assert!(!view.composing());
    assert!(view.composition_node().is_none());
}

fn child_text(view: &BrowserView, text: &str) -> Node {
    find_text(view.dom().as_ref(), text)
}

fn append_to_paragraph(view: &BrowserView, text: &str) -> Node {
    let child: Node = document().create_text_node(text).into();
    view.dom().first_child().unwrap().append_child(&child).unwrap();
    append(&child, "");
    child
}

#[wasm_bindgen_test]
fn composes_in_an_empty_block() {
    let view = mount(doc!(p!("<a>")), BrowserViewProps::default());
    compose(&view, || append_to_paragraph(&view, "a"), &[&|n| append(n, "b"), &|n| append(n, "c")],
        Options { has_node: true, ..Options::default() });
    assert_eq!(view.state().doc, doc!(p!("abc")).node);
}

#[wasm_bindgen_test]
fn composes_at_end_of_block() {
    let view = mount(doc!(p!("foo")), BrowserViewProps::default());
    compose(&view, || {
        let text = child_text(&view, "foo");
        append(&text, "");
        text
    }, &[&|n| append(n, "!"), &|n| append(n, "?")], Options::default());
    assert_eq!(view.state().doc, doc!(p!("foo!?")).node);
}

#[wasm_bindgen_test]
fn composes_at_end_of_block_in_a_new_node() {
    let view = mount(doc!(p!("foo")), BrowserViewProps::default());
    compose(&view, || append_to_paragraph(&view, "!"), &[&|n| append(n, "?")],
        Options { has_node: true, ..Options::default() });
    assert_eq!(view.state().doc, doc!(p!("foo!?")).node);
}

#[wasm_bindgen_test]
fn composes_at_start_of_block_in_a_new_node() {
    let view = mount(doc!(p!("foo")), BrowserViewProps::default());
    compose(&view, || {
        let parent = view.dom().first_child().unwrap();
        let node: Node = document().create_text_node("!").into();
        parent.insert_before(&node, parent.first_child().as_ref()).unwrap();
        append(&node, "");
        node
    }, &[&|n| append(n, "?")], Options { has_node: true, ..Options::default() });
    assert_eq!(view.state().doc, doc!(p!("!?foo")).node);
}

#[wasm_bindgen_test]
fn composes_inside_existing_text() {
    let view = mount(doc!(p!("foo")), BrowserViewProps::default());
    compose(&view, || {
        let text = child_text(&view, "foo");
        append(&text, "");
        text
    }, &[&|n| edit(n, "x", 1, 1), &|n| edit(n, "y", 2, 2), &|n| edit(n, "z", 3, 3)], Options::default());
    assert_eq!(view.state().doc, doc!(p!("fxyzoo")).node);
}

#[wasm_bindgen_test]
fn handles_android_style_newline_after_composition() {
    let view = mount(doc!(p!("abcdef")), BrowserViewProps::default());
    compose(&view, || {
        let text = child_text(&view, "abcdef");
        append(&text, "");
        text
    }, &[&|n| edit(n, "x", 3, 3), &|n| edit(n, "y", 4, 4)], Options {
        end: Some(&|n| {
            let line = document().create_element("div").unwrap();
            view.dom().append_child(&line).unwrap();
            line.set_text_content(Some("def"));
            n.set_node_value(Some("abcxy"));
            set_selection(&line, 0);
        }),
        ..Options::default()
    });
    assert_eq!(view.state().doc, doc!(p!("abcxy"), p!("def")).node);
}

#[wasm_bindgen_test]
fn handles_composition_replacing_existing_words() {
    let view = mount(doc!(p!("one two three")), BrowserViewProps::default());
    compose(&view, || {
        let text = child_text(&view, "one two three");
        edit(&text, "five", 4, 7);
        text
    }, &[&|n| edit(n, "seven", 4, 8), &|n| edit(n, "zero", 4, 9)], Options::default());
    assert_eq!(view.state().doc, doc!(p!("one zero three")).node);
}

#[wasm_bindgen_test]
fn composes_inside_marks() {
    let view = mount(doc!(p!("one ", mark("em", vec!["two".into()]))), BrowserViewProps::default());
    compose(&view, || {
        let text = child_text(&view, "two");
        append(&text, "o");
        text
    }, &[&|n| append(n, "o"), &|n| append(n, "w")], Options::default());
    assert_eq!(view.state().doc, doc!(p!("one ", mark("em", vec!["twooow".into()]))).node);
}

#[wasm_bindgen_test]
fn composes_in_a_mark_with_multiple_children() {
    let three = || mark("strong", vec![" three".into()]);
    let view = mount(doc!(p!("one ", mark("em", vec!["two".into(), three()]))), BrowserViewProps::default());
    compose(&view, || {
        let text = child_text(&view, "two");
        append(&text, "o");
        text
    }, &[&|n| append(n, "o"), &|n| append(n, "w")], Options::default());
    assert_eq!(view.state().doc, doc!(p!("one ", mark("em", vec!["twooow".into(), three()]))).node);
}

fn stored_mark(view: &BrowserView, mark: &str) {
    let mut tr = view.state().tr();
    tr.add_stored_mark(test_schema().mark_type(mark).unwrap().create(None));
    view.dispatch(tr).unwrap();
}

#[wasm_bindgen_test]
fn composes_in_a_cursor_wrapper() {
    let view = mount(doc!(p!("<a>")), BrowserViewProps::default());
    stored_mark(&view, "em");
    compose(&view, || {
        let node = append_to_paragraph(&view, "");
        append(&node, "a");
        node
    }, &[&|n| append(n, "b"), &|n| append(n, "c")], Options { has_node: true, ..Options::default() });
    assert_eq!(view.state().doc, doc!(p!(mark("em", vec!["abc".into()]))).node);
}

#[wasm_bindgen_test]
fn composes_in_a_multi_child_mark_with_a_cursor_wrapper() {
    let three = || mark("strong", vec![" three".into()]);
    let view = mount(doc!(p!("one ", mark("em", vec!["two<a>".into(), three()]))), BrowserViewProps::default());
    stored_mark(&view, "code");
    let em = view.dom().query_selector("em").unwrap().unwrap();
    compose(&view, || {
        let text: Node = document().create_text_node("").into();
        let strong = em.query_selector("strong").unwrap().unwrap();
        em.insert_before(&text, Some(&strong)).unwrap();
        append(&text, "o");
        text
    }, &[&|n| append(n, "o"), &|n| append(n, "w")], Options { has_node: true, ..Options::default() });
    assert_eq!(view.state().doc, doc!(p!("one ", mark("em", vec!["two".into(), mark("code", vec!["oow".into()]), three()]))).node);
}

fn word_props() -> BrowserViewProps {
    BrowserViewProps {
        decorations: vec![Rc::new(|state: &EditorState| {
            let mut decorations = Vec::new();
            state.doc.descendants(|node, pos, _, _| {
                if let Some(text) = node.text() {
                    let mut start = None;
                    for (index, unit) in text.encode_utf16().chain([0]).enumerate() {
                        let word = u8::try_from(unit).is_ok_and(|char| char.is_ascii_alphanumeric() || char == b'_');
                        if word {
                            start.get_or_insert(index);
                        } else if let Some(from) = start.take() {
                            decorations.push(Decoration::inline(pos + from, pos + index,
                                [("class".into(), "word".into())].into(), Value::Null));
                        }
                    }
                }
                true
            });
            Some(DecorationSet::create(&state.doc, decorations))
        })],
        ..Default::default()
    }
}

#[wasm_bindgen_test]
fn composition_survives_decoration_changes() {
    let view = mount(doc!(p!("foo ...")), word_props());
    compose(&view, || {
        let text = child_text(&view, " ...");
        append(&text, "");
        text
    }, &[&|n| edit(n, "hi", 1, 4)], Options::default());
    assert_eq!(view.state().doc, doc!(p!("foo hi")).node);
}

#[wasm_bindgen_test]
fn composes_inside_highlighted_text() {
    let view = mount(doc!(p!("one two")), word_props());
    compose(&view, || {
        let text = child_text(&view, "one");
        append(&text, "x");
        text
    }, &[&|n| append(n, "y"), &|n| append(n, ".")], Options::default());
    assert_eq!(view.state().doc, doc!(p!("onexy. two")).node);
}

#[wasm_bindgen_test]
fn handles_compositions_spanning_multiple_nodes() {
    let view = mount(doc!(p!("one two")), word_props());
    compose(&view, || {
        let text = child_text(&view, "two");
        append(&text, "a");
        text
    }, &[&|n| append(n, "b"), &|n| append(n, "c")], Options {
        end: Some(&|n| {
            let parent = n.parent_node().unwrap();
            for _ in 0..2 {
                let previous = parent.previous_sibling().unwrap();
                previous.parent_node().unwrap().remove_child(&previous).unwrap();
            }
            edit(n, "xyzone ", 0, 0);
        }),
        ..Options::default()
    });
    assert_eq!(view.state().doc, doc!(p!("xyzone twoabc")).node);
}

fn widgets() -> BrowserViewProps {
    let initial = doc!(p!());
    let decorations = Rc::new(RefCell::new(DecorationSet::create(&initial.node, vec![
        Decoration::widget(1, 1, json!({"side": -1})),
        Decoration::widget(1, 2, json!({"side": 1})),
    ])));
    let active = Rc::clone(&decorations);
    let factory: prosemirror_view::browser::WidgetViewFactory = Rc::new(|document, _, _, _| {
        let widget = document.create_element("var")?;
        widget.set_text_content(Some("×"));
        Ok(WidgetView::new(widget))
    });
    BrowserViewProps {
        decorations: vec![Rc::new(move |_| Some(decorations.borrow().clone()))],
        widgets: [(1, Rc::clone(&factory)), (2, factory)].into(),
        dispatch_transaction: Some(Rc::new(move |view, tr| {
            let mapped = active.borrow().map(tr.mapping(), tr.doc());
            *active.borrow_mut() = mapped;
            view.update_state(view.state().apply(&tr)).unwrap();
        })),
        ..Default::default()
    }
}

#[wasm_bindgen_test]
fn preserves_widgets_next_to_the_composition() {
    let view = mount(doc!(p!("")), widgets());
    compose(&view, || {
        let paragraph = view.dom().first_child().unwrap();
        let node: Node = document().create_text_node("a").into();
        paragraph.insert_before(&node, paragraph.last_child().as_ref()).unwrap();
        append(&node, "");
        node
    }, &[&|n| edit(n, "b", 0, 1)], Options {
        end: Some(&|_| assert_eq!(view.dom().query_selector_all("var").unwrap().length(), 2)),
        ..Options::default()
    });
    assert_eq!(view.state().doc, doc!(p!("b")).node);
}

fn replace(view: &BrowserView, text: &str, from: usize, to: usize) {
    let mut tr = view.state().tr();
    tr.insert_text(text, from, to);
    view.dispatch(tr).unwrap();
}

#[wasm_bindgen_test]
fn cancels_composition_when_an_edit_fully_overlaps_it() {
    let view = mount(doc!(p!("one"), p!("two"), p!("three")), BrowserViewProps::default());
    compose(&view, || {
        let text = child_text(&view, "two");
        append(&text, "x");
        text
    }, &[&|_| replace(&view, "---", 3, 13)], Options { cancel: true, ..Options::default() });
    assert_eq!(view.state().doc, doc!(p!("on---hree")).node);
}

#[wasm_bindgen_test]
fn cancels_composition_when_an_edit_partially_overlaps_it() {
    let view = mount(doc!(p!("one"), p!("two"), p!("three")), BrowserViewProps::default());
    compose(&view, || {
        let text = child_text(&view, "two");
        edit(&text, "x", 0, 0);
        text
    }, &[&|_| replace(&view, "---", 7, 15)], Options { cancel: true, ..Options::default() });
    assert_eq!(view.state().doc, doc!(p!("one"), p!("x---ee")).node);
}

#[wasm_bindgen_test]
fn cancels_composition_when_an_edit_happens_inside_it() {
    let view = mount(doc!(p!("one"), p!("two"), p!("three")), BrowserViewProps::default());
    compose(&view, || {
        let text = child_text(&view, "two");
        edit(&text, "x", 0, 0);
        text
    }, &[&|_| replace(&view, "!", 7, 8)], Options { cancel: true, ..Options::default() });
    assert_eq!(view.state().doc, doc!(p!("one"), p!("x!wo"), p!("three")).node);
}

#[wasm_bindgen_test]
fn keeps_composition_when_an_edit_happens_elsewhere() {
    let view = mount(doc!(p!("one"), p!("two"), p!("three")), BrowserViewProps::default());
    compose(&view, || {
        let text = child_text(&view, "two");
        edit(&text, "x", 0, 0);
        text
    }, &[&|n| edit(n, "y", 1, 1), &|_| replace(&view, "!", 2, 3), &|n| edit(n, "z", 2, 2)], Options::default());
    assert_eq!(view.state().doc, doc!(p!("o!e"), p!("xyztwo"), p!("three")).node);
}

#[wasm_bindgen_test]
fn handles_rapid_compositions() {
    let view = mount(doc!(p!("one"), p!("two")), BrowserViewProps::default());
    view.focus().unwrap();
    event(&view, "compositionstart");
    let one = child_text(&view, "one");
    append(&one, "!");
    view.flush_dom_changes().unwrap();
    event(&view, "compositionend");
    one.set_node_value(Some("one!!"));
    let last = view.dom().last_child().unwrap();
    event(&view, "compositionstart");
    let two = child_text(&view, "two");
    assert!(view.dom().last_child().unwrap().is_same_node(Some(&last)));
    append(&two, ".");
    view.flush_dom_changes().unwrap();
    let (focused, offset) = focus();
    assert!(focused.is_same_node(Some(&two)));
    assert_eq!(offset, 4);
    assert!(view.composing());
    event(&view, "compositionend");
    view.flush_dom_changes().unwrap();
    assert_eq!(view.state().doc, doc!(p!("one!!"), p!("two.")).node);
}

#[wasm_bindgen_test]
fn composes_across_paragraphs() {
    let view = mount(doc!(p!("one <a>two"), p!("three"), p!("four<b> five")), BrowserViewProps::default());
    compose(&view, || {
        let target = view.dom().first_child().unwrap().first_child().unwrap();
        target.set_node_value(Some("one A five"));
        set_selection(&target, 5);
        target
    }, &[&|n| edit(n, "B", 4, 5), &|n| edit(n, "C", 4, 5)], Options::default());
    assert_eq!(view.state().doc, doc!(p!("one C five")).node);
}
