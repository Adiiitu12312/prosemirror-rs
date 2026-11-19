//! Cases from prosemirror-model 1.24.1 test/test-node.ts.

mod support;

support::cases! {
    "node";
    cut_extracts_a_full_block => 0,
    cut_cuts_text => 1,
    cut_cuts_deeply => 2,
    cut_works_from_the_left => 3,
    cut_works_to_the_right => 4,
    cut_preserves_marks => 5,
    between_iterates_over_text => 6,
    between_descends_multiple_levels => 7,
    between_iterates_over_inline_nodes => 8,
}
