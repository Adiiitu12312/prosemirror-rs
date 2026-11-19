// Ported from prosemirror-search 1.1.0, test/test-search.ts.
use std::sync::Arc;

use prosemirror_model::{Fragment, Node, NodeSpec, Schema};
use prosemirror_search::{
    find_next, find_next_no_wrap, find_prev, find_prev_no_wrap, get_match_highlights, get_search_state,
    replace_all, replace_current, replace_next, replace_next_no_wrap, search, set_search_state,
    Dispatch, SearchOptions, SearchQuery, SearchQueryConfig, SearchRange,
};
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_test_builder::{builders, test_schema, ChildSpec, TaggedDoc};
use prosemirror_view::DecorationType;
use serde_json::json;

macro_rules! node {
    ($kind:expr; $($child:expr),* $(,)?) => {
        builders(test_schema(), Default::default())
            .node($kind, None, vec![$(ChildSpec::from($child)),*]).unwrap()
    };
}

macro_rules! p {
    ($($child:expr),* $(,)?) => { node!("paragraph"; $($child),*) };
}

fn em(text: &str) -> ChildSpec {
    builders(test_schema(), Default::default()).mark("em", None, vec![text.into()]).unwrap()
}

fn img() -> Node {
    test_schema().node("image", Some(json!({"src": "img.png"})), None, None).unwrap()
}

fn state(config: SearchQueryConfig, range: Option<SearchRange>, doc: TaggedDoc) -> EditorState {
    let selection = doc.get_tag("a").map(|anchor| {
        Selection::text(anchor, doc.get_tag("b").unwrap_or(anchor))
    });
    EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc.node),
        selection,
        stored_marks: None,
        plugins: vec![search(SearchOptions {
            initial_query: Some(SearchQuery::new(config)),
            initial_range: range,
        })],
    })
}

fn query(search: &str) -> SearchQueryConfig {
    SearchQueryConfig { search: search.into(), ..Default::default() }
}

fn replace(search: &str, replacement: &str) -> SearchQueryConfig {
    SearchQueryConfig { replace: replacement.into(), ..query(search) }
}

fn regexp(search: &str, replacement: &str) -> SearchQueryConfig {
    SearchQueryConfig { regexp: true, ..replace(search, replacement) }
}

type Command = for<'a> fn(&EditorState, Dispatch<'a>) -> bool;

fn test_selection(
    config: SearchQueryConfig,
    range: Option<SearchRange>,
    doc: TaggedDoc,
    command: Command,
) {
    let expected = doc.get_tag("c").map(|anchor| {
        Selection::text(anchor, doc.get_tag("d").unwrap_or(anchor))
    });
    let state = state(config, range, doc);
    let mut applied = None;
    assert_eq!(command(&state, Some(&mut |tr| applied = Some(state.apply(&tr)))), expected.is_some());
    if let Some(expected) = expected {
        assert_eq!(applied.unwrap().selection, expected);
    }
}

fn test_command(
    config: SearchQueryConfig,
    range: Option<SearchRange>,
    start: TaggedDoc,
    expected: Option<TaggedDoc>,
    command: Command,
) {
    let original = state(config.clone(), range, start);
    let mut applied = None;
    assert_eq!(command(&original, Some(&mut |tr| applied = Some(original.apply(&tr)))), expected.is_some());
    if let Some(expected) = expected {
        let actual = applied.unwrap();
        let expected = state(config, range, expected);
        assert_eq!(actual.doc, expected.doc);
        assert_eq!(actual.selection, expected.selection);
    }
}

#[test]
fn finds_next_match() {
    test_selection(query("two"), None, p!("one <c>two<d> two"), find_next);
}

#[test]
fn finds_next_match_from_selection() {
    test_selection(query("two"), None, p!("one <a>two<b> <c>two<d>"), find_next);
}

#[test]
fn next_wraps_at_document_end() {
    test_selection(query("two"), None, p!("one <c>two<d> <a>two<b>"), find_next);
}

#[test]
fn next_skips_wrapping_in_no_wrap_mode() {
    test_selection(query("two"), None, p!("one two <a>two<b>"), find_next_no_wrap);
}

#[test]
fn next_searches_a_limited_range() {
    test_selection(query("two"), Some(SearchRange::new(7, 11)), p!("one two <a>two<b>"), find_next);
}

#[test]
fn next_wraps_within_range() {
    test_selection(query("two"), Some(SearchRange::new(3, 11)), p!("two <c>two<d> <a>two<b>"), find_next);
}

#[test]
fn next_matches_in_nested_structure() {
    test_selection(
        query("one"),
        None,
        node!("doc"; node!("blockquote"; p!("para <a>one<b>"), p!("para two")), p!("and <c>one<d>")),
        find_next,
    );
}

#[test]
fn finds_previous_match() {
    test_selection(query("two"), None, p!("one <c>two<d> <a>two<b>"), find_prev);
}

#[test]
fn prev_wraps_at_start_of_document() {
    test_selection(query("two"), None, p!("one <a>two<b> <c>two<d>"), find_prev);
}

#[test]
fn prev_skips_wrapping_in_no_wrap_mode() {
    test_selection(query("two"), None, p!("one <a>two<b> two"), find_prev_no_wrap);
}

#[test]
fn prev_searches_a_limited_range() {
    test_selection(query("two"), Some(SearchRange::new(7, 11)), p!("one two <a>two<b>"), find_prev);
}

#[test]
fn prev_wraps_within_range() {
    test_selection(query("two"), Some(SearchRange::new(3, 11)), p!("two <a>two<b> <c>two<d>"), find_prev);
}

#[test]
fn prev_matches_in_nested_structure() {
    test_selection(
        query("one"),
        None,
        node!("doc"; node!("blockquote"; p!("para <c>one<d>"), p!("para two")), p!("and <a>one<b>")),
        find_prev,
    );
}

#[test]
fn replace_next_moves_to_match_when_not_already_on_one() {
    test_command(replace("one", "two"), None, p!("one one"), Some(p!("<a>one<b> one")), replace_next);
}

#[test]
fn replace_next_replaces_current_match() {
    test_command(replace("one", "two"), None, p!("<a>one<b> two"), Some(p!("<a>two<b> two")), replace_next);
}

#[test]
fn replace_next_moves_selection_to_next_match() {
    test_command(replace("one", "two"), None, p!("<a>one<b> one"), Some(p!("two <a>one<b>")), replace_next);
}

#[test]
fn replace_next_wraps_at_document_end() {
    test_command(replace("one", "two"), None, p!("one <a>one<b>"), Some(p!("<a>one<b> two")), replace_next);
}

#[test]
fn replace_next_skips_wrapping_in_no_wrap_mode() {
    test_command(replace("one", "two"), None, p!("one <a>one<b>"), Some(p!("one <a>two<b>")), replace_next_no_wrap);
}

#[test]
fn replace_next_replaces_within_limited_range() {
    test_command(
        replace("one", "two"),
        Some(SearchRange::new(0, 7)),
        p!("one <a>one<b> one"),
        Some(p!("<a>one<b> two one")),
        replace_next,
    );
}

#[test]
fn replace_next_reuses_parts_of_match() {
    test_command(
        regexp(r"\((.*?)\)", "[$1]"),
        None,
        p!("<a>(hi)<b> (x)"),
        Some(p!("[hi] <a>(x)<b>")),
        replace_next,
    );
}

#[test]
fn replace_next_reuses_matched_leaf_nodes() {
    test_command(
        regexp(r"\((.*?)\)", "[$1]"),
        None,
        p!("<a>(", img(), ")<b> (x)"),
        Some(p!("[", img(), "] <a>(x)<b>")),
        replace_next,
    );
}

#[test]
fn replace_next_replaces_in_nested_structure() {
    test_command(
        replace("one", "two"),
        None,
        node!("doc"; node!("blockquote"; p!("para <a>one<b>"), p!("para two")), p!("and one")),
        Some(node!("doc"; node!("blockquote"; p!("para two"), p!("para two")), p!("and <a>one<b>"))),
        replace_next,
    );
}

#[test]
fn replace_next_preserves_reused_content_positions() {
    let state = state(regexp(".(eu).", "p$1t"), None, p!("<a>deux<b> trois"));
    let mut transaction = None;
    assert!(replace_next(&state, Some(&mut |tr| transaction = Some(tr))));
    let transaction = transaction.unwrap();
    assert_eq!(transaction.doc(), &p!("peut trois").node);
    assert_eq!(transaction.mapping().map(2, 1), 2);
}

#[test]
fn replace_next_handles_multiple_references_to_groups() {
    test_command(regexp("(ab)-(cd)", "$2$1$2"), None, p!("<a>ab-cd<b>"), Some(p!("<a>cdabcd<b>")), replace_next);
}

#[test]
fn replace_next_replaces_unmatched_groups_with_empty_content() {
    test_command(regexp("(ab)|(cd)", "x$2"), None, p!("<a>ab<b>"), Some(p!("<a>x<b>")), replace_next);
}

#[test]
fn replace_next_supports_match_references_in_string_replacements() {
    test_command(replace("one", "$&$&"), None, p!("<a>one<b>"), Some(p!("<a>oneone<b>")), replace_next);
}

#[test]
fn replace_current_does_nothing_when_selection_is_not_at_match() {
    test_command(replace("one", "two"), None, p!("one"), None, replace_current);
}

#[test]
fn replace_current_selects_replacement() {
    test_command(replace("one", "two"), None, p!("<a>one<b>"), Some(p!("<a>two<b>")), replace_current);
}

#[test]
fn replace_current_replaces_delimiters_with_regexp() {
    test_command(
        regexp("“([^”]+)”", "$1"),
        None,
        p!("This is the <a>“footnote”<b> text"),
        Some(p!("This is the <a>footnote<b> text")),
        replace_current,
    );
}

fn footnote_schema() -> Schema {
    let mut nodes = prosemirror_schema_basic::nodes();
    let image = nodes.iter().position(|(name, _)| name == "image").unwrap();
    nodes.insert(image, ("footnote".into(), NodeSpec {
        group: Some("inline".into()),
        content: Some("text*".into()),
        inline: true,
        atom: true,
        ..Default::default()
    }));
    Schema::new(nodes, prosemirror_schema_basic::marks())
}

fn footnote(schema: &Schema, text: &str) -> TaggedDoc {
    let b = builders(schema, Default::default());
    let footnote = b.node("footnote", None, vec![text.into()]).unwrap();
    b.node("paragraph", None, vec!["text".into(), footnote.into()]).unwrap()
}

#[test]
fn replace_current_replaces_inside_non_leaf_atoms() {
    let schema = footnote_schema();
    test_command(
        replace("footnote", "NOTE"),
        None,
        footnote(&schema, "This is the <a>footnote<b> text"),
        Some(footnote(&schema, "This is the <a>NOTE<b> text")),
        replace_current,
    );
}

#[test]
fn replace_current_replaces_delimiters_inside_non_leaf_atoms() {
    let schema = footnote_schema();
    test_command(
        regexp("“([^”]+)”", "$1"),
        None,
        footnote(&schema, "This is the <a>“footnote”<b> text"),
        Some(footnote(&schema, "This is the <a>footnote<b> text")),
        replace_current,
    );
}

#[test]
fn replace_all_replaces_all_instances() {
    test_command(
        replace("one", "two"),
        None,
        node!("doc"; p!("this one"), p!("that one"), node!("blockquote"; p!("another one"))),
        Some(node!("doc"; p!("this two"), p!("that two"), node!("blockquote"; p!("another two")))),
        replace_all,
    );
}

#[test]
fn replace_all_reuses_parts_of_matches() {
    test_command(regexp(r"(\d+)-(\d+)", "$1:$2"), None, p!("50-20 vs 40-15"), Some(p!("50:20 vs 40:15")), replace_all);
}

#[test]
fn replace_all_works_within_limited_range() {
    test_command(
        replace("one", "two"),
        Some(SearchRange::new(2, 17)),
        p!("one one one one one"),
        Some(p!("one two two two one")),
        replace_all,
    );
}

#[test]
fn filters_replacements_by_emphasis() {
    let config = SearchQueryConfig {
        filter: Some(Arc::new(|state, result| state.doc.range_has_mark(result.from, result.to, "em"))),
        ..replace("one", "two")
    };
    test_command(
        config,
        None,
        node!("doc";
            p!("this one"), p!("that ", em("one")),
            node!("blockquote"; p!("another ", em("one"))),
        ),
        Some(node!("doc";
            p!("this one"), p!("that ", em("two")),
            node!("blockquote"; p!("another ", em("two"))),
        )),
        replace_all,
    );
}

#[test]
fn updates_query_with_typed_filter_and_remaps_active_highlight() {
    let original = state(query(""), None, p!("one <a>one<b>"));
    let mut tr = original.tr();
    let query = SearchQuery::new(SearchQueryConfig {
        filter: Some(Arc::new(|_, result| result.from > 0)),
        ..query("one")
    });
    set_search_state(&mut tr, query, Some(SearchRange::new(0, 7)));
    assert!(!tr.is_generic());
    let searched = original.apply(&tr);
    assert_eq!(get_match_highlights(&searched).find(None, None).len(), 1);
    let mut tr = searched.tr();
    tr.insert(0, Fragment::from_nodes(vec![searched.schema.text("x", None)]));
    let moved = searched.apply(&tr);
    assert_eq!(get_search_state(&moved).unwrap().range, Some(SearchRange::new(1, 8)));
    let decorations = get_match_highlights(&moved).find(None, None);
    assert_eq!((decorations[1].from, decorations[1].to), (5, 8));
    let DecorationType::Inline { attrs, .. } = &decorations[1].kind else { panic!("expected inline decoration") };
    assert_eq!(attrs["class"], "ProseMirror-active-search-match");
}

#[test]
fn replaces_zero_width_matches_without_revisiting_them() {
    let state = state(regexp("(?=o)", "x"), None, p!("one one"));
    let mut updated = None;
    assert!(replace_all(&state, Some(&mut |tr| updated = Some(state.apply(&tr)))));
    assert_eq!(updated.unwrap().doc, p!("xone xone").node);
}
