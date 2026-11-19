// Ported from prosemirror-state 1.4.4, test/test-selection.ts and test/state.ts.
use prosemirror_model::Node;
use prosemirror_state::{EditorState, EditorStateConfig, Selection, Transaction};
use prosemirror_test_builder::{blockquote, builders, doc, hr, p, test_schema, ChildSpec, TaggedDoc};
use serde_json::json;

fn node(name: &str) -> Node {
    test_schema().node(name, None, None, None).unwrap()
}

fn img() -> Node {
    test_schema().node("image", Some(json!({"src": "img.png"})), None, None).unwrap()
}

fn list(items: &[&str]) -> Node {
    let builders = builders(test_schema(), Default::default());
    let items = items.iter().map(|text| {
        let paragraph = builders.node("paragraph", None, vec![(*text).into()]).unwrap();
        builders.node("list_item", None, vec![paragraph.into()]).unwrap().into()
    }).collect();
    builders.node("bullet_list", None, items).unwrap().node
}

fn marked_paragraph(parts: Vec<ChildSpec>) -> TaggedDoc {
    let builders = builders(test_schema(), Default::default());
    let paragraph = builders.node("paragraph", None, parts).unwrap();
    builders.node("doc", None, vec![paragraph.into()]).unwrap()
}

fn em(text: &str) -> ChildSpec {
    builders(test_schema(), Default::default())
        .mark("em", None, vec![text.into()]).unwrap()
}

fn link(parts: Vec<ChildSpec>) -> ChildSpec {
    builders(test_schema(), Default::default())
        .mark("link", Some(json!({"href": "foo"})), parts).unwrap()
}

struct TestState(EditorState);

impl TestState {
    fn new(doc: TaggedDoc) -> Self {
        let selection = doc.get_tag("a").map(|anchor| {
            if doc.resolve(anchor).unwrap().parent().inline_content() {
                Selection::text(anchor, doc.get_tag("b").unwrap_or(anchor))
            } else {
                Selection::node(anchor)
            }
        });
        Self(EditorState::create(EditorStateConfig {
            schema: test_schema().clone(),
            doc: Some(doc.node),
            selection,
            stored_marks: None,
            plugins: vec![],
        }))
    }

    fn apply(&mut self, edit: impl FnOnce(&mut Transaction)) {
        let mut tr = self.0.tr();
        edit(&mut tr);
        self.0 = self.0.apply(&tr);
    }

    fn select(&mut self, selection: Selection) {
        self.apply(|tr| { tr.set_selection(selection); });
    }

    fn type_text(&mut self, text: &str) {
        self.apply(|tr| { tr.insert_text_at_selection(text); });
    }

    fn delete_selection(&mut self) {
        self.apply(|tr| { tr.delete_selection(); });
    }

    fn head(&self) -> usize {
        self.0.selection.head(&self.0.doc)
    }
}

#[test]
fn follows_changes() {
    let mut state = TestState::new(doc!(p!("hi")));
    state.apply(|tr| { tr.insert_text("xy", 1, 1); });
    assert_eq!(state.0.selection, Selection::cursor(3));
    state.apply(|tr| { tr.insert_text("zq", 1, 1); });
    assert_eq!(state.0.selection, Selection::cursor(5));
    state.apply(|tr| { tr.insert_text("uv", 7, 7); });
    assert_eq!(state.0.selection, Selection::cursor(5));
}

#[test]
fn explicit_text_insertion_keeps_a_distant_selection() {
    let mut state = TestState::new(doc!(p!("a<a>bc<b>def")));
    state.apply(|tr| { tr.insert_text("X", 6, 6); });
    assert_eq!(state.0.doc, doc!(p!("abcdeXf")).node);
    assert_eq!(state.0.selection, Selection::text(2, 4));

    state.apply(|tr| { tr.insert_text("Y", 1, 1); });
    assert_eq!(state.0.doc, doc!(p!("YabcdeXf")).node);
    assert_eq!(state.0.selection, Selection::text(3, 5));
}

#[test]
fn explicit_text_insertion_collapses_a_selection_ending_at_the_insert() {
    let mut state = TestState::new(doc!(p!("a<a>bc<b>def")));
    state.apply(|tr| { tr.insert_text("😀", 3, 4); });
    assert_eq!(state.0.doc, doc!(p!("ab😀def")).node);
    assert_eq!(state.0.selection, Selection::cursor(5));
}

#[test]
fn moves_after_inserted_content() {
    let mut state = TestState::new(doc!(p!("hi")));
    state.select(Selection::text(2, 3));
    state.type_text("o");
    assert_eq!(state.0.selection, Selection::cursor(3));
}

#[test]
fn moves_after_inserted_leaf_node() {
    let mut state = TestState::new(doc!(p!("foobar")));
    state.select(Selection::cursor(4));
    state.apply(|tr| { tr.replace_selection_with(hr(), true); });
    assert_eq!(state.0.doc, doc!(p!("foo"), hr(), p!("bar")).node);
    assert_eq!(state.head(), 7);
    state.select(Selection::cursor(10));
    state.apply(|tr| { tr.replace_selection_with(hr(), true); });
    assert_eq!(state.0.doc, doc!(p!("foo"), hr(), p!("bar"), hr()).node);
    assert_eq!(state.0.selection.from(), 11);
}

#[test]
fn allows_typing_over_a_leaf_node() {
    let mut state = TestState::new(doc!(p!("a"), "<a>", hr(), p!("b")));
    state.select(Selection::node(3));
    state.apply(|tr| { tr.replace_selection_with(test_schema().text("x", None), true); });
    assert_eq!(state.0.doc, doc!(p!("a"), p!("x"), p!("b")).node);
    assert_eq!(state.0.selection, Selection::cursor(5));
}

#[test]
fn allows_deleting_a_selected_block() {
    let mut state = TestState::new(doc!(p!("foo"), list(&["bar", "baz", "quux"])));
    state.select(Selection::node(0));
    state.delete_selection();
    assert_eq!(state.0.doc, doc!(list(&["bar", "baz", "quux"])).node);
    assert_eq!(state.head(), 3);
    state.select(Selection::node(2));
    state.delete_selection();
    assert_eq!(state.0.doc, doc!(list(&["baz", "quux"])).node);
    assert_eq!(state.head(), 3);
    state.select(Selection::node(9));
    state.delete_selection();
    assert_eq!(state.0.doc, doc!(list(&["baz"])).node);
    assert_eq!(state.head(), 6);
    state.select(Selection::node(0));
    state.delete_selection();
    assert_eq!(state.0.doc, doc!(p!()).node);
}

#[test]
fn preserves_marks_of_deleted_selection() {
    let mut state = TestState::new(marked_paragraph(vec!["foo".into(), em("<a>bar<b>"), "baz".into()]));
    state.delete_selection();
    assert_eq!(state.0.stored_marks.unwrap().len(), 1);
}

#[test]
fn does_not_preserve_non_inclusive_marks_of_deleted_selection() {
    let mut state = TestState::new(marked_paragraph(vec![
        "foo".into(), link(vec![em("<a>bar<b>")]), "baz".into(),
    ]));
    state.delete_selection();
    assert_eq!(state.0.stored_marks.unwrap().len(), 1);
}

#[test]
fn does_not_preserve_marks_when_deleting_across_block_boundary() {
    let builder = builders(test_schema(), Default::default());
    let first = builder.node("paragraph", None, vec!["foo".into(), em("bar<a>")]).unwrap();
    let second = builder.node("paragraph", None, vec!["b<b>az".into()]).unwrap();
    let doc = builder.node("doc", None, vec![first.into(), second.into()]).unwrap();
    let mut state = TestState::new(doc);
    state.delete_selection();
    assert!(state.0.stored_marks.is_none());
}

#[test]
fn drops_non_inclusive_marks_at_deleted_span_end() {
    let mut state = TestState::new(marked_paragraph(vec![
        "foo".into(), link(vec!["ba".into(), em("<a>r<b>")]), "baz".into(),
    ]));
    state.delete_selection();
    let marks = state.0.stored_marks.unwrap();
    assert_eq!(marks.iter().map(|mark| mark.type_name.as_str()).collect::<Vec<_>>(), ["em"]);
}

#[test]
fn keeps_non_inclusive_marks_when_still_inside_them() {
    let mut state = TestState::new(marked_paragraph(vec![
        "foo".into(), link(vec!["b".into(), em("<a>a<b>"), "r".into()]), "baz".into(),
    ]));
    state.delete_selection();
    assert_eq!(state.0.stored_marks.unwrap().len(), 2);
}

#[test]
fn preserves_marks_when_typing_over_marked_text() {
    let mut state = TestState::new(marked_paragraph(vec!["foo ".into(), em("<a>bar<b>"), " baz".into()]));
    state.type_text("quux");
    assert_eq!(state.0.doc, marked_paragraph(vec!["foo ".into(), em("quux"), " baz".into()]).node);
    state.apply(|tr| { tr.insert_text("bar", 5, 9); });
    assert_eq!(state.0.doc, marked_paragraph(vec!["foo ".into(), em("bar"), " baz".into()]).node);
}

#[test]
fn allows_deleting_a_leaf() {
    let mut state = TestState::new(doc!(p!("a"), hr(), hr(), p!("b")));
    state.select(Selection::node(3));
    state.delete_selection();
    assert_eq!(state.0.doc, doc!(p!("a"), hr(), p!("b")).node);
    assert_eq!(state.0.selection.from(), 3);
    state.delete_selection();
    assert_eq!(state.0.doc, doc!(p!("a"), p!("b")).node);
    assert_eq!(state.head(), 4);
}

#[test]
fn deletes_selection() {
    let mut state = TestState::new(doc!(p!("foo", img(), "bar"), blockquote!("hi"), p!("ay")));
    state.select(Selection::node(4));
    state.delete_selection();
    assert_eq!(state.0.doc, doc!(p!("foobar"), blockquote!("hi"), p!("ay")).node);
    assert_eq!(state.head(), 4);
    state.select(Selection::node(9));
    state.delete_selection();
    assert_eq!(state.0.doc, doc!(p!("foobar"), p!("ay")).node);
    assert_eq!(state.0.selection.from(), 9);
    state.select(Selection::node(8));
    state.delete_selection();
    assert_eq!(state.0.doc, doc!(p!("foobar")).node);
    assert_eq!(state.0.selection.from(), 7);
}

#[test]
fn replaces_inline_selections() {
    let mut state = TestState::new(doc!(p!("foo", img(), "bar", img(), "baz")));
    state.select(Selection::node(4));
    state.apply(|tr| { tr.replace_selection_with(node("hard_break"), true); });
    assert_eq!(state.0.doc, doc!(p!("foo", node("hard_break"), "bar", img(), "baz")).node);
    assert_eq!(state.0.selection, Selection::cursor(5));
    state.select(Selection::node(8));
    state.type_text("abc");
    assert_eq!(state.0.doc, doc!(p!("foo", node("hard_break"), "barabcbaz")).node);
    assert_eq!(state.0.selection, Selection::cursor(11));
    state.select(Selection::node(0));
    state.type_text("xyz");
    assert_eq!(state.0.doc, doc!(p!("xyz")).node);
}

#[test]
fn replaces_block_selection() {
    let mut state = TestState::new(doc!(p!("abc"), hr(), hr(), blockquote!(p!("ow"))));
    state.select(Selection::node(5));
    state.apply(|tr| { tr.replace_selection_with(node("code_block"), true); });
    assert_eq!(state.0.doc, doc!(p!("abc"), node("code_block"), hr(), blockquote!(p!("ow"))).node);
    assert_eq!(state.0.selection.from(), 7);
    state.select(Selection::node(8));
    state.apply(|tr| { tr.replace_selection_with(node("paragraph"), true); });
    assert_eq!(state.0.doc, doc!(p!("abc"), node("code_block"), hr(), p!()).node);
    assert_eq!(state.0.selection.from(), 9);
}

#[test]
fn places_cursor_after_inserted_text_from_list_item() {
    let mut state = TestState::new(doc!(p!("<a>abc")));
    let source = doc!(list(&["def"]));
    let slice = source.slice(3, 6, true).unwrap();
    state.apply(|tr| { tr.replace_selection(slice); });
    assert_eq!(state.0.selection.from(), 6);
}

#[test]
fn between_uses_arguments_when_possible() {
    let doc = doc!(p!("f<a>o<b>o"));
    let selection = Selection::between(&doc.resolve(doc.tag("b")).unwrap(), &doc.resolve(doc.tag("a")).unwrap(), 1);
    assert_eq!(selection, Selection::text(doc.tag("b"), doc.tag("a")));
}

#[test]
fn between_adjusts_when_necessary() {
    let doc = doc!("<a>", p!("foo"));
    let pos = doc.resolve(doc.tag("a")).unwrap();
    assert_eq!(Selection::between(&pos, &pos, 1).anchor(), 1);
}

#[test]
fn between_uses_bias_when_adjusting() {
    let doc = doc!(p!("foo"), "<a>", p!("bar"));
    let pos = doc.resolve(doc.tag("a")).unwrap();
    assert_eq!(Selection::between(&pos, &pos, -1).anchor(), 4);
    assert_eq!(Selection::between(&pos, &pos, 1).anchor(), 6);
}

#[test]
fn between_falls_back_to_node_selection() {
    let doc = doc!(hr(), "<a>");
    let pos = doc.resolve(doc.tag("a")).unwrap();
    let selection = Selection::between(&pos, &pos, 1);
    assert_eq!(selection.selected_node(&doc).as_ref(), Some(doc.child(0)));
}

#[test]
fn between_collapses_towards_other_argument() {
    let doc = doc!("<a>", p!("foo"), "<b>");
    let start = doc.resolve(doc.tag("a")).unwrap();
    let end = doc.resolve(doc.tag("b")).unwrap();
    assert_eq!(Selection::between(&start, &end, 1), Selection::text(1, 4));
    assert_eq!(Selection::between(&end, &start, 1), Selection::text(4, 1));
}
