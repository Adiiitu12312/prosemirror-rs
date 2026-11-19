//! Port of prosemirror-changeset 2.4.0 test/test-simplify.ts.

use prosemirror_changeset::{simplify_changes, Span};
use prosemirror_model::Node;
use prosemirror_schema_basic::schema;
use serde_json::json;

use super::{doc, node, paragraph, range, single_paragraph, text};

fn check(changes: &[&[usize]], doc: Node, expected: &[&[usize]]) {
    let input: Vec<_> = changes.iter().enumerate().map(|(i, positions)| {
        let [from_a, to_a] = positions[..2].try_into().unwrap();
        let [from_b, to_b] = if positions.len() > 2 {
            positions[2..].try_into().unwrap()
        } else {
            [from_a, to_a]
        };
        range([from_a, to_a, from_b, to_b], i as u8)
    }).collect();
    let simplified = simplify_changes(&input, &doc);
    assert_eq!(simplified.len(), expected.len());
    for (change, positions) in simplified.iter().zip(expected) {
        let actual = change.ranges();
        assert_eq!(if positions.len() == 2 { &actual[2..] } else { &actual[..] }, *positions);
    }
}

fn inline_atom_document() -> Node {
    let image = schema().node("image", Some(json!({"src": "img.png"})), None, None).unwrap();
    doc(vec![node("paragraph", vec![text("one"), image, text("two")])])
}

macro_rules! simplify_cases {
    ($($name:ident: [$($input:expr),*], $doc:expr, [$($expected:expr),*];)*) => {$(
        #[test]
        fn $name() {
            check(&[$(&$input),*], $doc, &[$(&$expected),*]);
        }
    )*};
}

simplify_cases! {
    preserves_insertions: [[1, 1, 1, 2], [2, 2, 3, 4]], single_paragraph("hello"),
        [[1, 1, 1, 2], [2, 2, 3, 4]];
    preserves_deletions: [[1, 2, 1, 1], [3, 4, 2, 2]], single_paragraph("hello"),
        [[1, 2, 1, 1], [3, 4, 2, 2]];
    preserves_single_letter_replacements: [[1, 2, 1, 2]], single_paragraph("hello"),
        [[1, 2, 1, 2]];
    expands_multiple_letter_replacements: [[2, 4, 2, 4]], single_paragraph("hello"),
        [[1, 6, 1, 6]];
    combines_changes_in_one_word: [[1, 3, 1, 1], [5, 5, 3, 4]], single_paragraph("hello"),
        [[1, 7, 1, 6]];
    expands_to_full_words: [[7, 10]], single_paragraph("one two three four"), [[5, 14]];
    stops_at_non_word_text: [[7, 10]], single_paragraph("one two ----- four"), [[5, 10]];
    treats_leaf_nodes_as_non_words: [[2, 3], [6, 7]], inline_atom_document(), [[2, 3], [6, 7]];
    treats_node_boundaries_as_non_words: [[2, 3], [7, 8]], doc(vec![paragraph("one"), paragraph("two")]),
        [[2, 3], [7, 8]];
    merges_stretches_of_changes: [[2, 3], [4, 6], [8, 10], [15, 16]], single_paragraph("foo bar baz bug ugh"),
        [[1, 12], [15, 16]];
    handles_realistic_word_updates: [[8, 8, 8, 11], [10, 15, 13, 17]], single_paragraph("chonic condition"),
        [[8, 15, 8, 17]];
    works_after_significant_content: [[63, 80, 63, 83]],
        doc(vec![paragraph("one long paragraph -----"), paragraph("two long paragraphs ------"),
            paragraph("a vote against the government")]), [[62, 81, 62, 84]];
    joins_changes_that_expand_to_touch: [[1, 5, 1, 5], [7, 13, 7, 9], [20, 21, 16, 16]],
        single_paragraph("and his co-star"), [[1, 13, 1, 9], [20, 21, 16, 16]];
}

#[test]
fn preserves_metadata_while_expanding_changes() {
    let changes = [range([2, 3, 2, 3], 0), range([4, 6, 4, 6], 1), range([8, 9, 8, 8], 2)];
    let simplified = simplify_changes(&changes, &single_paragraph("1234567890"));
    assert_eq!(simplified.len(), 1);
    assert_eq!(simplified[0].deleted, [Span::new(3, 0), Span::new(4, 1), Span::new(4, 2)]);
    assert_eq!(simplified[0].inserted, [Span::new(3, 0), Span::new(4, 1), Span::new(3, 2)]);
}
