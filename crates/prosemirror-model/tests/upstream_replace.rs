//! Cases from prosemirror-model 1.24.1 test/test-replace.ts.

mod support;

support::cases! {
    "replace";
    replace_joins_on_delete => 0,
    replace_merges_matching_blocks => 1,
    replace_merges_when_adding_text => 2,
    replace_can_insert_text => 3,
    replace_doesn_t_merge_non_matching_blocks => 4,
    replace_can_merge_a_nested_node => 5,
    replace_can_replace_within_a_block => 6,
    replace_can_insert_a_lopsided_slice => 7,
    replace_can_insert_a_deep_lopsided_slice => 8,
    replace_can_merge_multiple_levels => 9,
    replace_can_merge_multiple_levels_while_inserting => 10,
    replace_can_insert_a_split => 11,
    replace_can_insert_a_deep_split => 12,
    replace_can_add_a_split_one_level_up => 13,
    replace_keeps_the_node_type_of_the_left_node => 14,
    replace_keeps_the_node_type_even_when_empty => 15,
    replace_doesn_t_allow_the_left_side_to_be_too_deep => 16,
    replace_doesn_t_allow_a_depth_mismatch => 17,
    replace_rejects_a_bad_fit => 18,
    replace_rejects_unjoinable_content => 19,
    replace_rejects_an_unjoinable_delete => 20,
    replace_check_content_validity => 21,
}
