// Cases from the pinned upstream test/test-commands.ts.

#[path = "../command_support.rs"]
mod support;
fn verify_case(index: usize) {
    support::verify_case(include_str!("cases.json"), index);
}

#[test]
fn joinbackward_can_join_paragraphs() {
    verify_case(0);
}

#[test]
fn joinbackward_can_join_out_of_a_nested_node() {
    verify_case(1);
}

#[test]
fn joinbackward_moves_a_block_into_an_adjacent_wrapper() {
    verify_case(2);
}

#[test]
fn joinbackward_moves_a_block_into_an_adjacent_wrapper_from_another_wrapper() {
    verify_case(3);
}

#[test]
fn joinbackward_joins_the_wrapper_to_a_subsequent_one_if_applicable() {
    verify_case(4);
}

#[test]
fn joinbackward_moves_a_block_into_a_list_item() {
    verify_case(5);
}

#[test]
fn joinbackward_joins_lists() {
    verify_case(6);
}

#[test]
fn joinbackward_joins_list_items() {
    verify_case(7);
}

#[test]
fn joinbackward_lifts_out_of_a_list_at_the_start() {
    verify_case(8);
}

#[test]
fn joinbackward_joins_lists_before_and_after() {
    verify_case(9);
}

#[test]
fn joinbackward_deletes_leaf_nodes_before() {
    verify_case(10);
}

#[test]
fn joinbackward_lifts_before_it_deletes() {
    verify_case(11);
}

#[test]
fn joinbackward_does_nothing_at_start_of_doc() {
    verify_case(12);
}

#[test]
fn joinbackward_can_join_single_textblock_child_nodes() {
    verify_case(13);
}

#[test]
fn joinbackward_doesnt_return_true_on_empty_blocks_that_cant_be_deleted() {
    verify_case(14);
}

#[test]
fn joinbackward_doesnt_join_surrounding_nodes_of_different_types() {
    verify_case(15);
}

#[test]
fn jointextblockbackward_can_join_paragraphs() {
    verify_case(16);
}

#[test]
fn jointextblockbackward_can_join_if_second_block_is_wrapped() {
    verify_case(17);
}

#[test]
fn jointextblockbackward_can_join_if_first_block_is_wrapped() {
    verify_case(18);
}

#[test]
fn jointextblockbackward_does_nothing_at_start_of_doc() {
    verify_case(19);
}

#[test]
fn jointextblockbackward_can_join_if_inside_a_nested_block() {
    verify_case(20);
}

#[test]
fn selectnodebackward_selects_the_node_before_the_cut() {
    verify_case(21);
}

#[test]
fn selectnodebackward_does_nothing_when_not_at_the_start_of_the_textblock() {
    verify_case(22);
}

#[test]
fn deleteselection_deletes_part_of_a_text_node() {
    verify_case(23);
}

#[test]
fn deleteselection_can_delete_across_blocks() {
    verify_case(24);
}

#[test]
fn deleteselection_deletes_node_selections() {
    verify_case(25);
}

#[test]
fn deleteselection_moves_selection_after_deleted_node() {
    verify_case(26);
}

#[test]
fn deleteselection_moves_selection_before_deleted_node_at_end() {
    verify_case(27);
}

#[test]
fn joinforward_joins_two_textblocks() {
    verify_case(28);
}

#[test]
fn joinforward_keeps_type_of_second_node_when_first_is_empty() {
    verify_case(29);
}

#[test]
fn joinforward_clears_nodes_from_joined_node_that_wouldnt_be_allowed_in_target_node() {
    verify_case(30);
}

#[test]
fn joinforward_does_nothing_at_the_end_of_the_document() {
    verify_case(31);
}

#[test]
fn joinforward_deletes_a_leaf_node_after_the_current_block() {
    verify_case(32);
}

#[test]
fn joinforward_pulls_the_next_block_into_the_current_list_item() {
    verify_case(33);
}

#[test]
fn joinforward_joins_two_blocks_inside_of_a_list_item() {
    verify_case(34);
}

#[test]
fn joinforward_pulls_the_next_block_into_a_blockquote() {
    verify_case(35);
}

#[test]
fn joinforward_joins_two_blockquotes() {
    verify_case(36);
}

#[test]
fn joinforward_pulls_the_next_block_outside_of_a_wrapping_blockquote() {
    verify_case(37);
}

#[test]
fn joinforward_joins_two_lists() {
    verify_case(38);
}

#[test]
fn joinforward_does_nothing_in_a_nested_node_at_the_end_of_the_document() {
    verify_case(39);
}

#[test]
fn joinforward_deletes_a_leaf_node_at_the_end_of_the_document() {
    verify_case(40);
}

#[test]
fn joinforward_moves_before_it_deletes_a_leaf_node() {
    verify_case(41);
}

#[test]
fn joinforward_does_nothing_when_it_cant_join() {
    verify_case(42);
}

#[test]
fn jointextblockforward_can_join_paragraphs() {
    verify_case(43);
}

#[test]
fn jointextblockforward_can_join_if_second_block_is_wrapped() {
    verify_case(44);
}

#[test]
fn jointextblockforward_can_join_if_first_block_is_wrapped() {
    verify_case(45);
}

#[test]
fn jointextblockforward_does_nothing_at_end_of_doc() {
    verify_case(46);
}

#[test]
fn selectnodeforward_selects_the_next_node() {
    verify_case(47);
}

#[test]
fn selectnodeforward_does_nothing_at_end_of_document() {
    verify_case(48);
}

#[test]
fn joinup_joins_identical_parent_blocks() {
    verify_case(49);
}

#[test]
fn joinup_does_nothing_in_the_first_block() {
    verify_case(50);
}

#[test]
fn joinup_joins_lists() {
    verify_case(51);
}

#[test]
fn joinup_joins_list_items() {
    verify_case(52);
}

#[test]
fn joinup_doesnt_look_at_ancestors_when_a_block_is_selected() {
    verify_case(53);
}

#[test]
fn joinup_can_join_selected_block_nodes() {
    verify_case(54);
}

#[test]
fn joindown_joins_parent_blocks() {
    verify_case(55);
}

#[test]
fn joindown_doesnt_join_with_the_block_before() {
    verify_case(56);
}

#[test]
fn joindown_joins_lists() {
    verify_case(57);
}

#[test]
fn joindown_joins_list_items() {
    verify_case(58);
}

#[test]
fn joindown_doesnt_look_at_parent_nodes_of_a_selected_node() {
    verify_case(59);
}

#[test]
fn joindown_can_join_selected_nodes() {
    verify_case(60);
}

#[test]
fn lift_lifts_out_of_a_parent_block() {
    verify_case(61);
}

#[test]
fn lift_splits_the_parent_block_when_necessary() {
    verify_case(62);
}

#[test]
fn lift_can_lift_out_of_a_list() {
    verify_case(63);
}

#[test]
fn lift_does_nothing_for_a_top_level_block() {
    verify_case(64);
}

#[test]
fn lift_lifts_out_of_the_innermost_parent() {
    verify_case(65);
}

#[test]
fn lift_can_lift_a_node_selection() {
    verify_case(66);
}

#[test]
fn lift_lifts_out_of_a_nested_list() {
    verify_case(67);
}

#[test]
fn wrapin_can_wrap_a_paragraph() {
    verify_case(68);
}

#[test]
fn wrapin_wraps_multiple_pragraphs() {
    verify_case(69);
}

#[test]
fn wrapin_wraps_an_already_wrapped_node() {
    verify_case(70);
}

#[test]
fn wrapin_can_wrap_a_node_selection() {
    verify_case(71);
}

#[test]
fn splitblock_splits_a_paragraph_at_the_end() {
    verify_case(72);
}

#[test]
fn splitblock_split_a_pragraph_in_the_middle() {
    verify_case(73);
}

#[test]
fn splitblock_splits_a_paragraph_from_a_heading() {
    verify_case(74);
}

#[test]
fn splitblock_splits_a_heading_in_two_when_in_the_middle() {
    verify_case(75);
}

#[test]
fn splitblock_deletes_selected_content() {
    verify_case(76);
}

#[test]
fn splitblock_splits_a_parent_block_when_a_node_is_selected() {
    verify_case(77);
}

#[test]
fn splitblock_doesnt_split_the_parent_block_when_at_the_start() {
    verify_case(78);
}

#[test]
fn splitblock_splits_off_a_normal_paragraph_when_splitting_at_the_start_of_a_textblock() {
    verify_case(79);
}

#[test]
fn splitblock_splits_a_paragraph_from_a_heading_when_a_double_heading_isnt_allowed() {
    verify_case(80);
}

#[test]
fn splitblock_wont_try_to_reset_the_type_of_an_empty_leftover_when_the_schema_forbids_it() {
    verify_case(81);
}

#[test]
fn splitblock_can_split_an_inline_node() {
    verify_case(82);
}

#[test]
fn splitblock_prefers_textblocks() {
    verify_case(83);
}

#[test]
fn splitblockas_splits_to_the_appropriate_type() {
    verify_case(84);
}

#[test]
fn splitblockas_passes_an_end_of_block_flag() {
    verify_case(85);
}

#[test]
fn splitblockkeepmarks_keeps_marks_when_used_after_marked_text() {
    verify_case(86);
}

#[test]
fn splitblockkeepmarks_preserves_the_stored_marks() {
    verify_case(87);
}

#[test]
fn liftemptyblock_splits_the_parent_block_when_there_are_sibling_before() {
    verify_case(88);
}

#[test]
fn liftemptyblock_lifts_the_last_child_out_of_its_parent() {
    verify_case(89);
}

#[test]
fn liftemptyblock_lifts_an_only_child() {
    verify_case(90);
}

#[test]
fn liftemptyblock_does_not_violate_schema_constraints() {
    verify_case(91);
}

#[test]
fn liftemptyblock_lifts_out_of_a_list() {
    verify_case(92);
}

#[test]
fn createparagraphnear_creates_a_paragraph_before_a_selected_node_at_the_start_of_the_doc() {
    verify_case(93);
}

#[test]
fn createparagraphnear_creates_a_paragraph_after_a_lone_selected_node() {
    verify_case(94);
}

#[test]
fn createparagraphnear_creates_a_paragraph_after_selected_nodes_not_at_the_start_of_the_doc() {
    verify_case(95);
}

#[test]
fn setblocktype_can_change_the_type_of_a_paragraph() {
    verify_case(96);
}

#[test]
fn setblocktype_can_change_the_type_of_a_code_block() {
    verify_case(97);
}

#[test]
fn setblocktype_can_make_a_heading_into_a_paragraph() {
    verify_case(98);
}

#[test]
fn setblocktype_preserves_marks() {
    verify_case(99);
}

#[test]
fn setblocktype_acts_on_node_selections() {
    verify_case(100);
}

#[test]
fn setblocktype_can_make_a_block_a_code_block() {
    verify_case(101);
}

#[test]
fn setblocktype_clears_marks_when_necessary() {
    verify_case(102);
}

#[test]
fn setblocktype_acts_on_multiple_blocks_when_possible() {
    verify_case(103);
}

#[test]
fn setblocktype_returns_false_when_all_textblocks_in_the_selection_are_already_this_type() {
    verify_case(104);
}

#[test]
fn setblocktype_returns_false_when_the_selected_blocks_cant_be_changed() {
    verify_case(105);
}

#[test]
fn selectparentnode_selects_the_whole_textblock() {
    verify_case(106);
}

#[test]
fn selectparentnode_goes_one_level_up_when_on_a_block() {
    verify_case(107);
}

#[test]
fn selectparentnode_goes_further_up() {
    verify_case(108);
}

#[test]
fn selectparentnode_stops_at_the_top_level() {
    verify_case(109);
}

#[test]
fn autojoin_joins_lists_when_deleting_a_paragraph_between_them() {
    verify_case(110);
}

#[test]
fn autojoin_doesnt_join_lists_when_deleting_an_item_inside_of_them() {
    verify_case(111);
}

#[test]
fn autojoin_joins_lists_when_wrapping_a_paragraph_after_them_in_a_list() {
    verify_case(112);
}

#[test]
fn autojoin_joins_lists_when_wrapping_a_paragraph_between_them_in_a_list() {
    verify_case(113);
}

#[test]
fn autojoin_joins_lists_when_lifting_a_list_between_them() {
    verify_case(114);
}

#[test]
fn togglemark_can_add_a_mark() {
    verify_case(115);
}

#[test]
fn togglemark_can_stack_marks() {
    verify_case(116);
}

#[test]
fn togglemark_can_remove_marks() {
    verify_case(117);
}

#[test]
fn togglemark_can_toggle_pending_marks() {
    verify_case(118);
}

#[test]
fn togglemark_skips_whitespace_at_selection_ends_when_adding_marks() {
    verify_case(119);
}

#[test]
fn togglemark_doesnt_skip_whitespace_only_selections() {
    verify_case(120);
}

#[test]
fn togglemark_can_add_marks_with_remove_when_present_off() {
    verify_case(121);
}

#[test]
fn togglemark_can_remove_marks_with_remove_when_present_off() {
    verify_case(122);
}

#[test]
fn togglemark_can_remove_marks_with_trailing_space_when_remove_when_present_is_off() {
    verify_case(123);
}

#[test]
fn togglemark_enters_inline_atoms_by_default() {
    verify_case(124);
}

#[test]
fn togglemark_doesnt_enter_inline_atoms_to_add_a_mark_when_told_not_to() {
    verify_case(125);
}

#[test]
fn togglemark_can_apply_styles_inside_inline_atoms() {
    verify_case(126);
}

#[test]
fn togglemark_can_add_a_mark_even_if_already_active_inside_an_inline_atom() {
    verify_case(127);
}

#[test]
fn togglemark_doesnt_enter_inline_atoms_to_remove_a_mark_when_told_not_to() {
    verify_case(128);
}

#[test]
fn selecttextblockstart_and_selecttextblockend_can_move_the_cursor_when_the_selection_is_empty() {
    verify_case(129);
}

#[test]
fn selecttextblockstart_and_selecttextblockend_can_move_the_cursor_when_the_selection_is_not_empty() {
    verify_case(130);
}

#[test]
fn selecttextblockstart_and_selecttextblockend_can_move_the_cursor_when_the_selection_crosses_multiple_text_blocks() {
    verify_case(131);
}
