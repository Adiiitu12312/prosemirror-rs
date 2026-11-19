// Cases from the pinned upstream test/test-commands.ts.

#[path = "../command_support.rs"]
mod support;
fn verify_case(index: usize) {
    support::verify_case(include_str!("cases.json"), index);
}

#[test]
fn wrapinlist_can_wrap_a_paragraph() {
    verify_case(0);
}

#[test]
fn wrapinlist_can_wrap_a_nested_paragraph() {
    verify_case(1);
}

#[test]
fn wrapinlist_can_wrap_multiple_paragraphs() {
    verify_case(2);
}

#[test]
fn wrapinlist_doesnt_wrap_the_first_paragraph_in_a_list_item() {
    verify_case(3);
}

#[test]
fn wrapinlist_doesnt_wrap_the_first_para_in_a_different_type_of_list_item() {
    verify_case(4);
}

#[test]
fn wrapinlist_does_wrap_the_second_paragraph_in_a_list_item() {
    verify_case(5);
}

#[test]
fn wrapinlist_joins_with_the_list_item_above_when_wrapping_its_first_paragraph() {
    verify_case(6);
}

#[test]
fn wrapinlist_only_splits_items_where_valid() {
    verify_case(7);
}

#[test]
fn splitlistitem_has_no_effect_outside_of_a_list() {
    verify_case(8);
}

#[test]
fn splitlistitem_has_no_effect_on_the_top_level() {
    verify_case(9);
}

#[test]
fn splitlistitem_can_split_a_list_item() {
    verify_case(10);
}

#[test]
fn splitlistitem_can_split_a_list_item_at_the_end() {
    verify_case(11);
}

#[test]
fn splitlistitem_deletes_selected_content() {
    verify_case(12);
}

#[test]
fn splitlistitem_splits_when_lifting_from_a_nested_list() {
    verify_case(13);
}

#[test]
fn splitlistitem_can_lift_from_a_continued_nested_list_item() {
    verify_case(14);
}

#[test]
fn splitlistitem_correctly_lifts_an_entirely_empty_sublist() {
    verify_case(15);
}

#[test]
fn liftlistitem_can_lift_from_a_nested_list() {
    verify_case(16);
}

#[test]
fn liftlistitem_can_lift_two_items_from_a_nested_list() {
    verify_case(17);
}

#[test]
fn liftlistitem_can_lift_two_items_from_a_nested_three_item_list() {
    verify_case(18);
}

#[test]
fn liftlistitem_can_lift_an_item_out_of_a_list() {
    verify_case(19);
}

#[test]
fn liftlistitem_can_lift_two_items_out_of_a_list() {
    verify_case(20);
}

#[test]
fn liftlistitem_can_lift_three_items_from_the_middle_of_a_list() {
    verify_case(21);
}

#[test]
fn liftlistitem_can_lift_the_first_item_from_a_list() {
    verify_case(22);
}

#[test]
fn liftlistitem_can_lift_the_last_item_from_a_list() {
    verify_case(23);
}

#[test]
fn liftlistitem_joins_adjacent_lists_when_lifting_an_item_with_subitems() {
    verify_case(24);
}

#[test]
fn sinklistitem_can_wrap_a_simple_item_in_a_list() {
    verify_case(25);
}

#[test]
fn sinklistitem_wont_wrap_the_first_item_in_a_sublist() {
    verify_case(26);
}

#[test]
fn sinklistitem_will_move_an_items_content_into_the_item_above() {
    verify_case(27);
}
