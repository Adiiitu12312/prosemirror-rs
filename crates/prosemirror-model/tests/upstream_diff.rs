//! Cases from prosemirror-model 1.25.13 test/test-diff.ts.

mod support;

use prosemirror_model::{DiffEnd, Node};
use serde_json::json;

support::cases! {
    "diff";
    finddiffstart_returns_null_for_identical_nodes => 0,
    finddiffstart_notices_when_one_node_is_longer => 1,
    finddiffstart_notices_when_one_node_is_shorter => 2,
    finddiffstart_notices_differing_marks => 3,
    finddiffstart_stops_at_longer_text => 4,
    finddiffstart_stops_at_a_different_character => 5,
    finddiffstart_stops_at_a_different_node_type => 6,
    finddiffstart_works_when_the_difference_is_at_the_start => 7,
    finddiffstart_notices_a_different_attribute => 8,
    finddiffend_returns_null_when_there_is_no_difference => 9,
    finddiffend_notices_when_the_second_doc_is_longer => 10,
    finddiffend_notices_when_the_second_doc_is_shorter => 11,
    finddiffend_notices_different_styles => 12,
    finddiffend_spots_longer_text => 13,
    finddiffend_spots_different_text => 14,
    finddiffend_notices_different_nodes => 15,
    finddiffend_notices_a_difference_at_the_end => 16,
    finddiffend_handles_a_similar_start => 17,
}

fn one_character(text: &str) -> Node {
    support::schema("block+").node_from_json(&json!({
        "type":"doc", "content":[{"type":"paragraph", "content":[{"type":"text", "text":text}]}]
    })).unwrap()
}

#[test]
fn finddiffstart_does_not_start_in_the_middle_of_a_surrogate_pair() {
    let left = one_character("𝛿");
    let right = one_character("𝛾");
    assert_eq!(left.content().find_diff_start(right.content()), Some(1));
}

#[test]
fn finddiffend_does_not_end_in_the_middle_of_a_surrogate_pair() {
    let left = one_character("𝋾");
    let right = one_character("𝛾");
    assert_eq!(left.content().find_diff_end(right.content()), Some(DiffEnd { a: 3, b: 3 }));
}
