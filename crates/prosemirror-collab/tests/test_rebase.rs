// Ported from prosemirror-collab 1.3.1, test/test-rebase.ts.
use std::sync::Arc;

use prosemirror_collab::{rebase_steps, Rebaseable};
use prosemirror_model::{Fragment, Mark};
use prosemirror_test_builder::{builders, test_schema, ChildSpec, TaggedDoc};
use prosemirror_transform::Transform;

macro_rules! node {
    ($kind:expr; $($child:expr),* $(,)?) => {
        builders(test_schema(), Default::default())
            .node($kind, None, vec![$(ChildSpec::from($child)),*]).unwrap()
    };
}

fn em(value: &str) -> ChildSpec {
    builders(test_schema(), Default::default())
        .mark("em", None, vec![value.into()]).unwrap()
}

fn mark() -> Mark {
    test_schema().mark_type("em").unwrap().create(None)
}

fn type_text(tr: &mut Transform, pos: usize, text: &str) {
    tr.replace_with(pos, pos, Fragment::from_nodes(vec![test_schema().text(text, None)]));
}

fn wrap(tr: &mut Transform, pos: usize, name: &str) {
    let pos = tr.doc.resolve(pos).unwrap();
    let range = pos.block_range(&pos).unwrap();
    let wrapper = test_schema().node_type(name).unwrap().create(None, None, None);
    tr.wrap(&range, &[wrapper]);
}

type Edit<'a> = dyn Fn(&mut Transform) + 'a;

fn rebase(start: &TaggedDoc, clients: &[&Edit<'_>], expected: &TaggedDoc, permute: bool) {
    let orders = if permute { permutations(clients) } else { vec![clients.to_vec()] };
    for order in orders {
        let mut full = Transform::new(start.node.clone());
        for edit in order {
            let mut original = Transform::new(start.node.clone());
            edit(&mut original);
            let original = Arc::new(original);
            let mut rebased = Transform::new(original.doc.clone());
            let from = original.steps.len() + full.steps.len();
            let local = original.steps.iter().zip(&original.docs).map(|(step, doc)| Rebaseable {
                step: step.clone(),
                inverted: step.invert(doc),
                origin: Arc::clone(&original),
            }).collect::<Vec<_>>();
            rebase_steps(&local, &full.steps, &mut rebased).unwrap();
            for step in rebased.steps.into_iter().skip(from) {
                full.step(step).unwrap();
            }
        }
        assert_eq!(full.doc, expected.node);
        for (tag, pos) in &start.tags {
            let mapped = full.mapping.map_result(*pos, 1);
            let expected = expected.get_tag(tag);
            if mapped.deleted() {
                assert_eq!(expected, None, "tag {tag}");
            } else {
                assert_eq!(Some(mapped.pos), expected, "tag {tag}");
            }
        }
    }
}

fn permutations<T: Copy>(values: &[T]) -> Vec<Vec<T>> {
    if values.len() < 2 {
        return vec![values.to_vec()];
    }
    let mut orders = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let remainder = [&values[..index], &values[index + 1..]].concat();
        for others in permutations(&remainder) {
            let mut order = Vec::with_capacity(values.len());
            order.push(*value);
            order.extend(others);
            orders.push(order);
        }
    }
    orders
}

#[test]
fn supports_concurrent_typing() {
    rebase(
        &node!("doc"; node!("paragraph"; "h<1>ell<2>o")),
        &[&|tr| type_text(tr, 2, "X"), &|tr| type_text(tr, 5, "Y")],
        &node!("doc"; node!("paragraph"; "hX<1>ellY<2>o")),
        true,
    );
}

#[test]
fn supports_multiple_concurrently_typed_chars() {
    rebase(
        &node!("doc"; node!("paragraph"; "h<1>ell<2>o")),
        &[&|tr| {
            type_text(tr, 2, "X");
            type_text(tr, 3, "Y");
            type_text(tr, 4, "Z");
        }, &|tr| {
            type_text(tr, 5, "U");
            type_text(tr, 6, "V");
        }],
        &node!("doc"; node!("paragraph"; "hXYZ<1>ellUV<2>o")),
        true,
    );
}

#[test]
fn supports_three_concurrent_typers() {
    rebase(
        &node!("doc"; node!("paragraph"; "h<1>ell<2>o th<3>ere")),
        &[&|tr| type_text(tr, 2, "X"), &|tr| type_text(tr, 5, "Y"), &|tr| type_text(tr, 9, "Z")],
        &node!("doc"; node!("paragraph"; "hX<1>ellY<2>o thZ<3>ere")),
        true,
    );
}

#[test]
fn handles_wrapping_changed_blocks() {
    rebase(
        &node!("doc"; node!("paragraph"; "<1>hell<2>o<3>")),
        &[&|tr| type_text(tr, 5, "X"), &|tr| wrap(tr, 1, "blockquote")],
        &node!("doc"; node!("blockquote"; node!("paragraph"; "<1>hellX<2>o<3>"))),
        true,
    );
}

#[test]
fn handles_insertions_in_deleted_content() {
    rebase(
        &node!("doc"; node!("paragraph"; "hello<1> wo<2>rld<3>!")),
        &[&|tr| { tr.delete(6, 12); }, &|tr| type_text(tr, 9, "X")],
        &node!("doc"; node!("paragraph"; "hello<3>!")),
        true,
    );
}

#[test]
fn allows_deleting_same_content_twice() {
    let deletion = |tr: &mut Transform| { tr.delete(6, 12); };
    rebase(
        &node!("doc"; node!("paragraph"; "hello<1> wo<2>rld<3>!")),
        &[&deletion, &deletion],
        &node!("doc"; node!("paragraph"; "hello<3>!")),
        false,
    );
}

#[test]
fn joins_block_being_edited() {
    rebase(
        &node!("doc"; node!("bullet_list";
            node!("list_item"; node!("paragraph"; "one")), "<1>",
            node!("list_item"; node!("paragraph"; "tw<2>o"))
        )),
        &[&|tr| type_text(tr, 12, "A"), &|tr| { tr.join(8, 1); }],
        &node!("doc"; node!("bullet_list";
            node!("list_item"; node!("paragraph"; "one"), node!("paragraph"; "twA<2>o"))
        )),
        true,
    );
}

#[test]
fn supports_typing_concurrently_with_marking() {
    rebase(
        &node!("doc"; node!("paragraph"; "hello <1>wo<2>rld<3>")),
        &[&|tr| { tr.add_mark(7, 12, mark()); }, &|tr| type_text(tr, 9, "_")],
        &node!("doc"; node!("paragraph"; "hello <1>", em("wo"), "_<2>", em("rld<3>"))),
        false,
    );
}

#[test]
fn keeps_marks_added_concurrently() {
    rebase(
        &node!("doc"; node!("paragraph"; em("<1>hello"), " world<2>")),
        &[&|tr| { tr.add_mark(1, 12, mark()); }, &|tr| { tr.remove_mark(1, 12, mark()); }],
        &node!("doc"; node!("paragraph"; "<1>hello", em(" world<2>"))),
        false,
    );
}

#[test]
fn keeps_concurrently_unmarked_text_unmarked() {
    rebase(
        &node!("doc"; node!("paragraph"; "<1>hello ", em("world<2>"))),
        &[&|tr| { tr.remove_mark(1, 12, mark()); }, &|tr| { tr.add_mark(1, 12, mark()); }],
        &node!("doc"; node!("paragraph"; em("<1>hello "), "world<2>")),
        false,
    );
}

#[test]
fn deletes_inserts_in_replaced_context() {
    let start = node!("doc";
        node!("paragraph"; "b<before>efore"),
        node!("blockquote"; node!("bullet_list";
            node!("list_item"; node!("paragraph"; "o<1>ne")),
            node!("list_item"; node!("paragraph"; "t<2>wo")),
            node!("list_item"; node!("paragraph"; "thr<3>ee"))
        )),
        node!("paragraph"; "a<after>fter")
    );
    let replacement = node!("doc";
        node!("paragraph"; "a"), node!("blockquote"; node!("paragraph"; "b")), node!("paragraph"; "c"));
    rebase(
        &start,
        &[&|tr| { tr.replace(start.tag("1"), start.tag("3"), replacement.slice(2, 9, false).unwrap()); },
          &|tr| type_text(tr, start.tag("2"), "ayay")],
        &node!("doc";
            node!("paragraph"; "b<before>efore"),
            node!("blockquote"; node!("bullet_list"; node!("list_item";
                node!("paragraph"; "o"), node!("blockquote"; node!("paragraph"; "b")),
                node!("paragraph"; "<3>ee")
            ))),
            node!("paragraph"; "a<after>fter")
        ),
        false,
    );
}

#[test]
fn maps_through_inserts() {
    rebase(
        &node!("doc"; node!("paragraph"; "X<1>X<2>X")),
        &[&|tr| type_text(tr, 2, "hello"), &|tr| {
            type_text(tr, 3, "goodbye");
            tr.delete(4, 7);
        }],
        &node!("doc"; node!("paragraph"; "Xhello<1>Xgbye<2>X")),
        true,
    );
}

#[test]
fn handles_concurrent_removal_of_blocks() {
    let start = node!("doc"; node!("paragraph"; "a"), "<1>", node!("paragraph"; "b"), "<2>", node!("paragraph"; "c"));
    let deletion = |tr: &mut Transform| { tr.delete(start.tag("1"), start.tag("2")); };
    rebase(
        &start,
        &[&deletion, &deletion],
        &node!("doc"; node!("paragraph"; "a"), "<2>", node!("paragraph"; "c")),
        false,
    );
}

#[test]
fn discards_edits_in_removed_blocks() {
    let start = node!("doc"; node!("paragraph"; "a"), "<1>", node!("paragraph"; "b<2>"), "<3>", node!("paragraph"; "c"));
    rebase(
        &start,
        &[&|tr| { tr.delete(start.tag("1"), start.tag("3")); }, &|tr| type_text(tr, start.tag("2"), "ay")],
        &node!("doc"; node!("paragraph"; "a"), "<3>", node!("paragraph"; "c")),
        true,
    );
}

#[test]
fn preserves_double_block_inserts() {
    let insert = |tr: &mut Transform| {
        let node = test_schema().node("paragraph", None, None, None).unwrap();
        tr.replace_with(3, 3, Fragment::from_nodes(vec![node]));
    };
    rebase(
        &node!("doc"; node!("paragraph"; "a"), "<1>", node!("paragraph"; "b")),
        &[&insert, &insert],
        &node!("doc"; node!("paragraph"; "a"), node!("paragraph";), node!("paragraph";), "<1>", node!("paragraph"; "b")),
        false,
    );
}
