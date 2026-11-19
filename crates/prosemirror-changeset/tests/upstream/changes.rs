//! Port of prosemirror-changeset 2.4.0 test/test-changes.ts.

use prosemirror_changeset::{Change, ChangeSet, Span};
use prosemirror_model::{Fragment, Node};
use prosemirror_schema_basic::schema;
use prosemirror_transform::Transform;
use serde_json::json;

use super::{doc, node, paragraph, positions, single_paragraph, text};

enum Edit<'a> {
    Text(usize, usize, &'a str),
    Node(usize, usize, Node),
    WrapFirstParagraph,
}

fn insert(pos: usize, value: &str) -> Edit<'_> {
    Edit::Text(pos, pos, value)
}

fn del(from: usize, to: usize) -> Edit<'static> {
    Edit::Text(from, to, "")
}

fn replace(from: usize, to: usize, value: &str) -> Edit<'_> {
    Edit::Text(from, to, value)
}

fn track(initial: Node, edits: &[(&[Edit<'_>], u8)]) -> ChangeSet<u8> {
    let mut cursor = initial.clone();
    let mut set = ChangeSet::create(initial);
    for &(batch, author) in edits {
        let mut transform = Transform::new(cursor);
        for edit in batch {
            match edit {
                Edit::Text(from, to, "") => { transform.delete(*from, *to); }
                Edit::Text(from, to, value) => {
                    transform.replace_with(*from, *to, Fragment::from_nodes(vec![text(value)]));
                }
                Edit::Node(from, to, node) => {
                    transform.replace_with(*from, *to, Fragment::from_nodes(vec![node.clone()]));
                }
                Edit::WrapFirstParagraph => {
                    let pos = transform.doc.resolve(1).unwrap();
                    let range = pos.block_range(&pos).unwrap();
                    let wrapper = schema().node_type("blockquote").unwrap().create(None, None, None);
                    transform.wrap(&range, &[wrapper]);
                }
            }
        }
        set = set.add_steps(&transform.doc, &transform.mapping.maps, author);
        cursor = transform.doc;
    }
    set
}

fn check(initial: Node, edits: &[&[Edit<'_>]], expected: &[[usize; 4]]) {
    let batches: Vec<_> = edits.iter().map(|edits| (*edits, 0)).collect();
    assert_eq!(positions(&track(initial, &batches).changes), expected);
}

fn with_metadata(
    [a, b, c, d]: [usize; 4], deleted: &[(usize, u8)], inserted: &[(usize, u8)],
) -> Change<u8> {
    let make_spans = |ranges: &[(usize, u8)]| ranges.iter()
        .map(|(length, data)| Span::new(*length, *data)).collect();
    Change::new(a, b, c, d, make_spans(deleted), make_spans(inserted))
}

macro_rules! changes {
    ($($name:ident: $initial:expr, [$($edits:expr),*], $expected:expr;)*) => {$(
        #[test]
        fn $name() {
            check(single_paragraph($initial), &[$(&$edits),*], &$expected);
        }
    )*};
}

changes! {
    finds_a_single_insertion: "hello", [[insert(3, "XY")]], [[3, 3, 3, 5]];
    finds_a_single_deletion: "hello", [[del(3, 5)]], [[3, 5, 3, 3]];
    identifies_a_replacement: "hello", [[replace(3, 5, "juj")]], [[3, 5, 3, 6]];
    merges_adjacent_canceling_edits: "hello", [[del(3, 5), insert(3, "ll")]], [];
    handles_edits_after_cancelled_edits: "hello", [[del(2, 3), insert(2, "e"), del(5, 6)]], [[5, 6, 5, 5]];
    stops_handling_a_collapsed_inserted_span: "abcba", [[insert(2, "b"), insert(6, "b"), del(3, 6)]], [[3, 4, 3, 3]];
    partially_merges_insert_at_start: "helLo", [[del(3, 5), insert(3, "l")]], [[4, 5, 4, 4]];
    partially_merges_insert_at_end: "helLo", [[del(3, 5), insert(3, "L")]], [[3, 4, 3, 3]];
    partially_merges_delete_at_start: "abc", [[insert(3, "xyz"), del(3, 4)]], [[3, 3, 3, 5]];
    partially_merges_delete_at_end: "abc", [[insert(3, "xyz"), del(5, 6)]], [[3, 3, 3, 5]];
    finds_multiple_insertions: "abc", [[insert(1, "x"), insert(5, "y")]], [[1, 1, 1, 2], [4, 4, 5, 6]];
    finds_multiple_deletions: "xyz", [[del(1, 2), del(2, 3)]], [[1, 2, 1, 1], [3, 4, 2, 2]];
    identifies_deletion_between_insertions: "zyz", [[insert(2, "A"), insert(4, "B"), del(3, 4)]], [[2, 3, 2, 4]];
    adds_deletion_in_new_add_steps_call: "hello", [[del(1, 2)], [del(2, 3)]], [[1, 2, 1, 1], [3, 4, 2, 2]];
    merges_delete_insert_from_different_calls: "hello", [[del(3, 5)], [insert(3, "ll")]], [];
    reverts_a_deletion_by_reinserting: "bar", [[del(2, 3)], [insert(2, "x")], [insert(2, "a")]], [[3, 3, 3, 4]];
    inserts_a_character_before_a_changed_character: "bar", [[del(2, 3)], [insert(2, "x")], [insert(2, "x")]], [[2, 3, 2, 4]];
    partially_merges_delete_insert_from_different_calls: "heljo", [[del(3, 5)], [insert(3, "ll")]], [[4, 5, 4, 5]];
    merges_insert_delete_from_different_calls: "ok", [[insert(2, "--")], [del(2, 4)]], [];
    partially_merges_insert_delete_from_different_calls: "ok", [[insert(2, "--")], [del(2, 3)]], [[2, 2, 2, 3]];
    maps_deletions_forward: "foobar", [[del(5, 6)], [insert(1, "OKAY")]], [[1, 1, 1, 5], [5, 6, 9, 9]];
    incrementally_undoes_then_redoes: "bar", [[del(2, 3)], [insert(2, "a")], [del(2, 3)]], [[2, 3, 2, 2]];
    maps_through_complicated_changesets: "12345678901234",
        [[del(9, 12), insert(6, "xyz"), replace(2, 3, "uv")], [del(14, 15), insert(13, "90"), del(8, 9)]],
        [[2, 3, 2, 4], [6, 6, 7, 9], [11, 12, 14, 14], [13, 14, 15, 15]];
    readds_content_step_by_step: "one two three",
        [[del(1, 14)], [insert(1, "two")], [insert(4, " ")], [insert(5, "three")]], [[1, 5, 1, 1]];
    handles_multiple_map_entries: "", [[insert(1, "ab")], [Edit::WrapFirstParagraph]], [[0, 0, 0, 1], [1, 1, 2, 4], [2, 2, 5, 6]];
}

#[test]
fn computes_a_proper_diff_of_the_changes() {
    check(
        doc(vec![paragraph("abcd"), paragraph("efgh")]),
        &[&[del(2, 10), insert(2, "cdef")]],
        &[[2, 3, 2, 2], [5, 7, 4, 4], [9, 10, 6, 6]],
    );
}

fn quoted(heading: &str, body: &str) -> Node {
    node("blockquote", vec![node("heading", vec![text(heading)]), paragraph(body)])
}

#[test]
fn tracks_split_deletion_metadata() {
    let result = track(
        doc(vec![quoted("one", "two four")]),
        &[(&[del(7, 11)], 0), (&[Edit::Node(0, 13, quoted("one", "four"))], 1)],
    );
    assert_eq!(&*result.changes, [with_metadata([7, 11, 7, 7], &[(4, 0)], &[])]);
}

#[test]
fn tracks_repeated_split_deletion_metadata() {
    let result = track(
        doc(vec![quoted("one", "two three")]),
        &[
            (&[del(14, 16)], 0),
            (&[del(7, 11)], 1),
            (&[del(3, 5)], 2),
            (&[Edit::Node(0, 10, quoted("o", "thr"))], 3),
        ],
    );
    assert_eq!(&*result.changes, [
        with_metadata([3, 5, 3, 3], &[(2, 2)], &[]),
        with_metadata([8, 12, 6, 6], &[(3, 1), (1, 3)], &[]),
        with_metadata([14, 16, 8, 8], &[(2, 0)], &[]),
    ]);
}

#[test]
fn preserves_order_of_overlapping_changes() {
    let result = track(
        single_paragraph("12345"),
        &[
            (&[del(4, 5)], 0), (&[insert(2, "a")], 0), (&[del(1, 6)], 1), (&[insert(1, "1a235")], 1),
        ],
    );
    assert_eq!(&*result.changes, [
        with_metadata([2, 2, 2, 3], &[], &[(1, 1)]),
        with_metadata([4, 5, 5, 5], &[(1, 0)], &[]),
    ]);
}

#[test]
fn properly_maps_deleted_positions() {
    let result = track(
        single_paragraph("jTKqvPrzApX"),
        &[
            (&[del(8, 11)], 1), (&[insert(1, "MPu")], 2), (&[del(2, 12)], 2), (&[insert(2, "PujTKqvPrX")], 2),
        ],
    );
    assert_eq!(&*result.changes, [
        with_metadata([1, 1, 1, 4], &[], &[(3, 2)]),
        with_metadata([8, 11, 11, 11], &[(3, 1)], &[]),
    ]);
}

#[test]
fn fuzz_issue_1() {
    let result = track(
        single_paragraph("hzwiKqBPzn"),
        &[
            (&[del(3, 7)], 0), (&[insert(5, "LH")], 1), (&[insert(6, "uE")], 0),
            (&[del(1, 6)], 1), (&[del(3, 6)], 0),
        ],
    );
    assert_eq!(&*result.changes, [
        with_metadata([1, 11, 1, 3], &[(2, 1), (4, 0), (2, 1), (2, 0)], &[(2, 0)]),
    ]);
}

#[test]
fn fuzz_issue_2() {
    let result = track(
        single_paragraph("eAMISWgauf"),
        &[
            (&[del(5, 10)], 2), (&[insert(5, "KkM")], 0), (&[insert(3, "UDO")], 0),
            (&[del(1, 12)], 0), (&[insert(1, "eAUDOMIKkMf")], 0),
            (&[del(5, 8)], 0), (&[insert(3, "qX")], 0),
        ],
    );
    assert_eq!(&*result.changes, [
        with_metadata([3, 10, 3, 10], &[(2, 0), (5, 2)], &[(7, 0)]),
    ]);
}

#[test]
fn fuzz_issue_3() {
    let result = track(
        single_paragraph("hfxjahnOuH"),
        &[
            (&[del(1, 5)], 1), (&[insert(3, "X")], 0), (&[del(1, 8)], 1),
            (&[insert(1, "ahXnOuH")], 1), (&[del(2, 4)], 1), (&[insert(2, "tn")], 1),
            (&[del(5, 7)], 1), (&[del(1, 6)], 0), (&[insert(1, "atnnH")], 0), (&[del(2, 6)], 0),
        ],
    );
    assert_eq!(&*result.changes, [
        with_metadata([1, 11, 1, 2], &[(4, 1), (1, 0), (1, 1), (1, 0), (2, 1), (1, 0)], &[(1, 0)]),
    ]);
}

#[test]
fn restores_precomputed_changes_from_upstream_json() {
    let first = with_metadata([1, 2, 1, 3], &[(1, 3)], &[(2, 4)]);
    let expected = json!({
        "fromA": 1, "toA": 2, "fromB": 1, "toB": 3,
        "deleted": [{"length": 1, "data": 3}], "inserted": [{"length": 2, "data": 4}],
    });
    assert_eq!(serde_json::to_value(&first).unwrap(), expected);
    let recovered: Change<u8> = serde_json::from_value(expected).unwrap();
    let doc = single_paragraph("x");
    let set = ChangeSet::with_config(
        doc.clone(), |a, b| (a == b).then_some(*a), prosemirror_changeset::DefaultEncoder, vec![recovered],
    );
    assert_eq!(&*set.changes, [first]);
    assert_eq!(set.start_doc(), &doc);
}
