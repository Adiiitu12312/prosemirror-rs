use prosemirror_model::{Fragment, Node};

#[test]
fn cuts_and_rejoins_both_halves_of_a_surrogate_pair() {
    let original = Node::new_text("🧑", Vec::new());
    let first = original.cut(0, 1);
    let second = original.cut(1, 2);

    assert_eq!(first.text_utf16().as_deref(), Some(&[0xd83e][..]));
    assert_eq!(second.text_utf16().as_deref(), Some(&[0xddd1][..]));
    assert_eq!(first.node_size(), 1);
    assert_eq!(second.node_size(), 1);
    assert_eq!(Fragment::from_nodes(vec![first, second]).first_child(), Some(&original));
}

#[test]
fn diff_distinguishes_different_unpaired_surrogates() {
    let text = Node::new_text("😀🧑", Vec::new());
    let left = Fragment::from_nodes(vec![text.cut(0, 1)]);
    let right = Fragment::from_nodes(vec![text.cut(2, 3)]);

    assert_ne!(left, right);
    assert_eq!(left.find_diff_start(&right), Some(0));
    let end = left.find_diff_end(&right).unwrap();
    assert_eq!((end.a, end.b), (1, 1));
}
