//! Port of prosemirror-changeset 2.4.0 test/test-diff.ts.

use prosemirror_changeset::{compute_diff_with_encoder, ChangeSet, DefaultEncoder, DefaultToken, TokenEncoder};
use prosemirror_model::{Mark, Node};
use prosemirror_schema_basic::schema;
use serde_json::json;

use super::{doc, node, paragraph, positions, range, single_paragraph, text};

fn paragraphs(texts: &[&str]) -> Node {
    doc(texts.iter().map(|value| paragraph(value)).collect())
}

fn diff(a: Node, b: Node, expected: &[[usize; 4]]) {
    let range = range([0, a.content().size(), 0, b.content().size()], 0);
    assert_eq!(positions(&ChangeSet::compute_diff(a.content(), b.content(), &range)), expected);
}

fn marked(value: &str, mark: &str) -> Node {
    let mark = schema().mark_type(mark).unwrap().create(None);
    schema().text(value, Some(vec![mark]))
}

fn heading(level: usize, value: &str) -> Node {
    node("heading", vec![text(value)]).with_attrs(Some(json!({"level": level})))
}

macro_rules! diff_cases {
    ($($name:ident: $a:expr, $b:expr, $expected:expr;)*) => {$(
        #[test]
        fn $name() {
            diff($a, $b, &$expected);
        }
    )*};
}

diff_cases! {
    identical_documents: paragraphs(&["foo", "bar"]), paragraphs(&["foo", "bar"]), [];
    single_letter: paragraphs(&["foo", "bar"]), paragraphs(&["foa", "bar"]), [[3, 4, 3, 4]];
    simple_structure: paragraphs(&["foo", "bar"]), paragraphs(&["foobar"]), [[4, 6, 4, 4]];
    multiple_changes: paragraphs(&["foo", "---bar"]), paragraphs(&["fgo", "---bur"]), [[2, 4, 2, 4], [10, 11, 10, 11]];
    ignores_one_unchanged_letter: single_paragraph("abcdef"), single_paragraph("axydzf"), [[2, 6, 2, 6]];
    ignores_short_matching_substrings: paragraphs(&["One two three"]),
        paragraphs(&["One", "And another long paragraph that has wo and ee in it"]), [[4, 14, 4, 57]];
    finds_deletions: paragraphs(&["abc", "def"]), paragraphs(&["ac", "d"]), [[2, 3, 2, 2], [7, 9, 6, 6]];
    ignores_marks: single_paragraph("abc"), doc(vec![node("paragraph", vec![marked("a", "em"), marked("bc", "strong")])]), [];
    ignores_marks_and_finds_text_edits: single_paragraph("abcdefghi"),
        doc(vec![node("paragraph", vec![marked("x", "em"), marked("bc", "strong"), text("defgh"), marked("y", "em")])]),
        [[1, 2, 1, 2], [9, 10, 9, 10]];
    ignores_attributes: doc(vec![heading(1, "x")]), doc(vec![heading(2, "x")]), [];
    ambiguous_diffs: single_paragraph("abcbcd"), single_paragraph("abcd"), [[4, 6, 4, 4]];
    distinguishes_closing_tokens: single_paragraph("a"), doc(vec![heading(1, "oo")]), [[0, 3, 0, 4]];
}

#[test]
fn finds_large_insertions() {
    let (xs, bs) = ("x".repeat(200), "b".repeat(20));
    diff(
        single_paragraph(&format!("a{bs}c")),
        single_paragraph(&format!("a{xs}{bs}{xs}c")),
        &[[2, 2, 2, 202], [22, 22, 222, 422]],
    );
}

#[test]
fn finds_large_deletions() {
    let (xs, bs) = ("x".repeat(200), "b".repeat(20));
    diff(
        single_paragraph(&format!("a{xs}{bs}{xs}c")),
        single_paragraph(&format!("a{bs}c")),
        &[[2, 202, 2, 2], [222, 422, 22, 22]],
    );
}

struct MarkEncoder;

impl TokenEncoder for MarkEncoder {
    type Token = (DefaultToken, Vec<Mark>);

    fn encode_character(&self, ch: u16, marks: &[Mark]) -> Self::Token {
        (DefaultEncoder.encode_character(ch, marks), marks.to_vec())
    }

    fn encode_node_start(&self, node: &Node) -> Self::Token {
        (DefaultEncoder.encode_node_start(node), Vec::new())
    }

    fn encode_node_end(&self, node: &Node) -> Self::Token {
        (DefaultEncoder.encode_node_end(node), Vec::new())
    }

    fn compare_tokens(&self, a: &Self::Token, b: &Self::Token) -> bool {
        DefaultEncoder.compare_tokens(&a.0, &b.0) && a.1 == b.1
    }
}

#[test]
fn custom_encoder_detects_marks_and_observes_utf16_positions() {
    let a = single_paragraph("a😀b");
    let b = doc(vec![node("paragraph", vec![text("a"), marked("😀", "em"), text("b")])]);
    let change = range([0, a.content().size(), 0, b.content().size()], 0);
    let actual = compute_diff_with_encoder(a.content(), b.content(), &change, &MarkEncoder);
    assert_eq!(positions(&actual), [[2, 4, 2, 4]]);
}
