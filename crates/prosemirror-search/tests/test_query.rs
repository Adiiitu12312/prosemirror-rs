// Ported from prosemirror-search 1.1.0, test/test-query.ts.
use prosemirror_search::{SearchQuery, SearchQueryConfig, SearchRange};
use prosemirror_state::{EditorState, EditorStateConfig};
use prosemirror_test_builder::{builders, test_schema, ChildSpec, TaggedDoc};

macro_rules! node {
    ($kind:expr; $($child:expr),* $(,)?) => {
        builders(test_schema(), Default::default())
            .node($kind, None, vec![$(ChildSpec::from($child)),*]).unwrap()
    };
}

fn em(text: &str) -> ChildSpec {
    builders(test_schema(), Default::default()).mark("em", None, vec![text.into()]).unwrap()
}

fn query(search: &str) -> SearchQueryConfig {
    SearchQueryConfig { search: search.into(), ..Default::default() }
}

fn test(config: SearchQueryConfig, doc: TaggedDoc) {
    let mut matches = Vec::new();
    for index in 1.. {
        let start = doc.get_tag(&format!("s{index}"));
        let end = doc.get_tag(&format!("e{index}"));
        let Some((from, to)) = start.zip(end) else { break };
        matches.push(SearchRange::new(from, to));
    }
    let state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc.node),
        selection: None,
        stored_marks: None,
        plugins: vec![],
    });
    let query = SearchQuery::new(config);
    let mut forward = Vec::new();
    let mut pos = 0;
    let size = state.doc.content().size();
    while let Some(next) = query.find_next(&state, pos, size) {
        forward.push(SearchRange::new(next.from, next.to));
        pos = next.to;
    }
    assert_eq!(forward, matches);

    let mut backward = Vec::new();
    let mut pos = size;
    while let Some(next) = query.find_prev(&state, pos, 0) {
        backward.push(SearchRange::new(next.from, next.to));
        pos = next.from;
    }
    matches.reverse();
    assert_eq!(backward, matches);
}

#[test]
fn matches_plain_strings() {
    test(query("abc"), node!("paragraph"; "<s1>abc<e1> flakdj a<s2>abc<e2> aabbcc"));
}

#[test]
fn skips_overlapping_matches() {
    test(query("aba"), node!("paragraph"; "<s1>aba<e1>b<s2>aba<e2>."));
}

#[test]
fn goes_through_multiple_textblocks() {
    test(query("12"), node!("doc";
        node!("paragraph"; "a<s1>12<e1>b"),
        node!("paragraph"; "..."),
        node!("paragraph"; "and <s2>12<e2>")
    ));
}

#[test]
fn matches_across_mark_boundaries() {
    test(query("two"), node!("paragraph"; "ab<s1>t", em("w"), "o<e1>oo"));
}

#[test]
fn matches_case_insensitive_strings() {
    test(query("abC"), node!("paragraph"; "<s1>aBc<e1> flakdj a<s2>ABC<e2>"));
}

#[test]
fn matches_literally() {
    let conf = SearchQueryConfig { literal: true, ..query(r"a\nb") };
    test(conf, node!("paragraph"; "a\nb <s1>a\\nb<e1>"));
}

#[test]
fn matches_by_word() {
    let conf = SearchQueryConfig { whole_word: true, ..query("hello") };
    test(conf, node!("paragraph"; "<s1>hello<e1> hellothere <s2>hello<e2>\nello ahello ohellop"));
}

#[test]
fn matches_non_words_without_requiring_word_characters() {
    let conf = SearchQueryConfig { whole_word: true, ..query("^_^") };
    test(conf, node!("paragraph"; "x<s1>^_^<e1>y <s2>^_^<e2>"));
}

#[test]
fn matches_regular_expressions() {
    let conf = SearchQueryConfig { regexp: true, ..query("a..b") };
    test(conf, node!("paragraph"; "<s1>appb<e1> apb"));
}

#[test]
fn matches_case_insensitive_regular_expressions() {
    let conf = SearchQueryConfig { regexp: true, ..query("a..b") };
    test(conf, node!("paragraph"; "<s1>Appb<e1> Apb"));
}

#[test]
fn matches_regular_expressions_across_textblocks() {
    let conf = SearchQueryConfig { regexp: true, ..query("12") };
    test(conf, node!("doc";
        node!("paragraph"; "a<s1>12<e1>b"),
        node!("paragraph"; "..."),
        node!("paragraph"; "and <s2>12<e2>")
    ));
}

#[test]
fn matches_regular_expressions_by_word() {
    let conf = SearchQueryConfig { regexp: true, whole_word: true, ..query("a..") };
    test(conf, node!("paragraph"; "<s1>aap<e1> baap aapje <s2>a--<e2>w"));
}
