//! Cases from prosemirror-model 1.24.1 test/test-slice.ts.

mod support;

support::cases! {
    "slice";
    slice_can_cut_half_a_paragraph => 0,
    slice_can_cut_to_the_end_of_a_pragraph => 1,
    slice_leaves_off_extra_content => 2,
    slice_preserves_styles => 3,
    slice_can_cut_multiple_blocks => 4,
    slice_can_cut_to_a_top_level_position => 5,
    slice_can_cut_to_a_deep_position => 6,
    slice_can_cut_everything_after_a_position => 7,
    slice_can_cut_from_the_start_of_a_textblock => 8,
    slice_leaves_off_extra_content_before => 9,
    slice_preserves_styles_after_cut => 10,
    slice_preserves_styles_started_after_cut => 11,
    slice_can_cut_from_a_top_level_position => 12,
    slice_can_cut_from_a_deep_position => 13,
    slice_can_cut_part_of_a_text_node => 14,
    slice_can_cut_across_paragraphs => 15,
    slice_can_cut_part_of_marked_text => 16,
    slice_can_cut_across_different_depths => 17,
    slice_can_cut_between_deeply_nested_nodes => 18,
    slice_can_include_parents => 19,
}
