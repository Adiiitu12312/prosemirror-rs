//! Port of prosemirror-transform 1.10.2 test/test-step.ts and test/test-replace_step.ts.

use prosemirror_model::{Fragment, Mark, Node, Slice};
use prosemirror_transform::{Step, StepResult, Transform};

fn children(nodes: Vec<Node>) -> Fragment {
    Fragment::from_nodes(nodes)
}

fn node(name: &str, nodes: Vec<Node>) -> Node {
    Node::new(name, None, children(nodes), Vec::new())
}

fn paragraph(text: &str) -> Node {
    node("paragraph", vec![Node::new_text(text, Vec::new())])
}

fn document(nodes: Vec<Node>) -> Node {
    node("doc", nodes)
}

fn make_step((from, to, value): (usize, usize, Option<&str>)) -> Step {
    let em = Mark::new("em", None);
    match value {
        Some("+em") => Step::add_mark(from, to, em),
        Some("-em") => Step::remove_mark(from, to, em),
        Some(s) => Step::replace(from, to, Slice::new(children(vec![Node::new_text(s, Vec::new())]), 0, 0)),
        None => Step::replace(from, to, Slice::empty()),
    }
}

fn assert_merge(left: (usize, usize, Option<&str>), right: (usize, usize, Option<&str>), expected: bool) {
    let initial = document(vec![paragraph("foobar")]);
    let left = make_step(left);
    let right = make_step(right);
    let merged = left.merge(&right);
    assert_eq!(merged.is_some(), expected);
    if let Some(merged) = merged {
        let StepResult::Ok(intermediate) = left.apply(&initial) else {
            panic!("First step should apply");
        };
        assert_eq!(merged.apply(&initial), right.apply(&intermediate));
    }
}

macro_rules! merge_cases {
    ($($test:ident: $left:expr, $right:expr, $expected:expr;)*) => {$(
        #[test]
        fn $test() {
            assert_merge($left, $right, $expected);
        }
    )*};
}

merge_cases! {
    merges_typing: (2, 2, Some("a")), (3, 3, Some("b")), true;
    merges_inverse_typing: (2, 2, Some("a")), (2, 2, Some("b")), true;
    rejects_separate_typing: (2, 2, Some("a")), (4, 4, Some("b")), false;
    rejects_inverted_separate_typing: (3, 3, Some("a")), (2, 2, Some("b")), false;
    merges_adjacent_backspaces: (3, 4, None), (2, 3, None), true;
    merges_adjacent_deletes: (2, 3, None), (2, 3, None), true;
    rejects_separate_backspaces: (1, 2, None), (2, 3, None), false;
    merges_backspace_and_type: (2, 3, None), (2, 2, Some("x")), true;
    merges_longer_adjacent_inserts: (2, 2, Some("quux")), (6, 6, Some("baz")), true;
    merges_inverted_longer_inserts: (2, 2, Some("quux")), (2, 2, Some("baz")), true;
    merges_longer_deletes: (2, 5, None), (2, 4, None), true;
    merges_inverted_longer_deletes: (4, 6, None), (2, 4, None), true;
    merges_overwrites: (3, 4, Some("x")), (4, 5, Some("y")), true;
    merges_adding_adjacent_styles: (1, 2, Some("+em")), (2, 4, Some("+em")), true;
    merges_adding_overlapping_styles: (1, 3, Some("+em")), (2, 4, Some("+em")), true;
    rejects_adding_separate_styles: (1, 2, Some("+em")), (3, 4, Some("+em")), false;
    merges_removing_adjacent_styles: (1, 2, Some("-em")), (2, 4, Some("-em")), true;
    merges_removing_overlapping_styles: (1, 3, Some("-em")), (2, 4, Some("-em")), true;
    rejects_removing_separate_styles: (1, 2, Some("-em")), (3, 4, Some("-em")), false;
}

#[test]
fn mapped_wrap_step_preserves_insertions() {
    let original = document(vec![paragraph("a")]);
    let position = original.resolve(1).unwrap();
    let range = position.block_range(&position).expect("wrap range");
    let mut wrapping = Transform::new(original.clone());
    wrapping.wrap(&range, &[node("blockquote", Vec::new())]);
    let mut insertion = Transform::new(original);
    insertion.insert(0, children(vec![paragraph("b")]));
    let step = wrapping.steps[0].map(&insertion.mapping).expect("wrap survives insertion");
    insertion.step(step).unwrap();
    assert_eq!(insertion.doc, document(vec![paragraph("b"), node("blockquote", vec![paragraph("a")])]));
}

#[test]
fn mapped_lift_step_preserves_insertions() {
    let original = document(vec![node("blockquote", vec![paragraph("a")])]);
    let position = original.resolve(2).unwrap();
    let range = position.block_range(&position).expect("lift range");
    let mut lifting = Transform::new(original.clone());
    lifting.lift(&range, 0);
    let mut insertion = Transform::new(original);
    insertion.insert(2, children(vec![Node::new_text("x", Vec::new())]));
    let step = lifting.steps[0].map(&insertion.mapping).expect("lift survives insertion");
    insertion.step(step).unwrap();
    assert_eq!(insertion.doc, document(vec![paragraph("xa")]));
}

#[test]
fn a_structural_replace_step_rejects_overwriting_text() {
    let original = document(vec![paragraph("abc")]);
    let step = Step::replace_structure(1, 2, Slice::empty());
    assert_eq!(
        step.apply(&original),
        StepResult::fail("Structure replace would overwrite content"),
    );
}
