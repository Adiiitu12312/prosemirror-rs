//! Port of prosemirror-view@1.42.9/test/webtest-decoration.ts.

#[path = "support/decoration.rs"]
mod support;
use support::verify_case;

#[test]
fn builds_up_a_matching_tree() {
    verify_case(0);
}

#[test]
fn does_not_build_nodes_when_there_are_no_decorations() {
    verify_case(1);
}

#[test]
fn puts_decorations_between_children_in_local() {
    verify_case(2);
}

#[test]
fn puts_decorations_spanning_children_in_local() {
    verify_case(3);
}

#[test]
fn puts_node_decorations_in_the_parent_node() {
    verify_case(4);
}

#[test]
fn drops_empty_inline_decorations() {
    verify_case(5);
}

#[test]
fn decorationset_find_finds_all_when_no_arguments_are_given() {
    verify_case(6);
}

#[test]
fn decorationset_find_finds_only_those_within_the_given_range() {
    verify_case(7);
}

#[test]
fn decorationset_find_finds_decorations_at_the_edge_of_the_range() {
    verify_case(8);
}

#[test]
fn decorationset_find_returns_the_correct_offset_for_deeply_nested_decorations() {
    verify_case(9);
}

#[test]
fn decorationset_find_can_filter_by_predicate() {
    verify_case(10);
}

#[test]
fn decorationset_map_supports_basic_mapping() {
    verify_case(11);
}

#[test]
fn decorationset_map_drops_deleted_decorations() {
    verify_case(12);
}

#[test]
fn decorationset_map_can_map_node_decorations() {
    verify_case(13);
}

#[test]
fn decorationset_map_can_map_inside_node_decorations() {
    verify_case(14);
}

#[test]
fn decorationset_map_removes_partially_overwritten_node_decorations() {
    verify_case(15);
}

#[test]
fn decorationset_map_removes_exactly_overwritten_node_decorations() {
    verify_case(16);
}

#[test]
fn decorationset_map_isnt_inclusive_by_default() {
    verify_case(17);
}

#[test]
fn decorationset_map_understands_unclusiveleft() {
    verify_case(18);
}

#[test]
fn decorationset_map_understands_unclusiveright() {
    verify_case(19);
}

#[test]
fn decorationset_map_preserves_subtrees_not_touched_by_mapping() {
    verify_case(20);
}

#[test]
fn decorationset_map_rebuilds_when_a_node_is_joined() {
    verify_case(21);
}

#[test]
fn decorationset_map_rebuilds_when_a_node_is_split() {
    verify_case(22);
}

#[test]
fn decorationset_map_correctly_rebuilds_a_deep_structure() {
    verify_case(23);
}

#[test]
fn decorationset_map_calls_onremove_when_dropping_decorations() {
    verify_case(24);
}

#[test]
fn decorationset_map_respects_the_side_option_on_widgets() {
    verify_case(25);
}

#[test]
fn decorationset_map_doesnt_doubly_map_decorations_nested_in_multiple_nodes() {
    verify_case(26);
}

#[test]
fn decorationset_map_rebuilds_subtrees_correctly_at_an_offset() {
    verify_case(27);
}

#[test]
fn decorationset_map_properly_maps_decorations_after_deleted_siblings() {
    verify_case(28);
}

#[test]
fn decorationset_map_can_map_the_content_of_nodes_that_moved_in_the_same_transaction() {
    verify_case(29);
}

#[test]
fn decorationset_map_can_handle_nodes_moving_up_multiple_levels() {
    verify_case(30);
}

#[test]
fn decorationset_map_maps_inline_decorations_through_ranges_with_3_elements() {
    verify_case(31);
}

#[test]
fn decorationset_map_correctly_offsets_a_deep_structure() {
    verify_case(32);
}

#[test]
fn decorationset_map_doesnt_get_confused_by_composite_changes() {
    verify_case(33);
}

#[test]
fn decorationset_add_can_add_a_local_decoration() {
    verify_case(34);
}

#[test]
fn decorationset_add_can_add_a_decoration_in_a_new_child() {
    verify_case(35);
}

#[test]
fn decorationset_add_can_add_a_decoration_to_an_existing_child() {
    verify_case(36);
}

#[test]
fn decorationset_add_can_add_a_decoration_beyond_an_existing_child() {
    verify_case(37);
}

#[test]
fn decorationset_add_can_add_multiple_decorations() {
    verify_case(38);
}

#[test]
fn decorationset_remove_can_delete_a_decoration() {
    verify_case(39);
}

#[test]
fn decorationset_remove_can_delete_multiple_decorations() {
    verify_case(40);
}

#[test]
fn decorationset_remove_ignores_decorations_that_dont_exist() {
    verify_case(41);
}

#[test]
fn decorationset_remove_compares_by_both_position_and_type_when_removing() {
    verify_case(42);
}

#[test]
fn removeoverlap_returns_the_original_array_when_there_is_no_overlap() {
    verify_case(43);
}

#[test]
fn removeoverlap_splits_a_partially_overlapping_decoration() {
    verify_case(44);
}

#[test]
fn removeoverlap_splits_a_decoration_that_spans_multiple_widgets() {
    verify_case(45);
}

#[test]
fn removeoverlap_correctly_splits_overlapping_inline_decorations() {
    verify_case(46);
}
