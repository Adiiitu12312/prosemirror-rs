use prosemirror_model::{Fragment, Mark, Node, Slice};
use prosemirror_test_builder::{build_doc, p, text};
use prosemirror_transform::{Step, StepMap, Transform};

#[test]
fn test_step_map_mapping() {
    // A single step replacing 5 chars with 2 chars at position 1
    let map = StepMap::single(1, 5, 2);

    assert_eq!(map.map(0, 1), 0);
    assert_eq!(map.map(1, -1), 1);
    assert_eq!(map.map(1, 1), 1);
    assert_eq!(map.map(6, 1), 3);
    assert_eq!(map.map(10, 1), 7); // 10 - 5 + 2 = 7

    let res = map.map_result(3, 1);
    assert!(res.deleted());
}

#[test]
fn test_step_map_invert() {
    let map = StepMap::single(2, 4, 10);
    let inv = map.invert();

    let orig_pos = 15;
    let mapped = map.map(orig_pos, 1);
    let roundtrip = inv.map(mapped, 1);
    assert_eq!(roundtrip, orig_pos);
}

#[test]
fn test_transform_replace_and_undo() {
    let initial = build_doc(|ctx| {
        ctx.wrap_block("doc", |inner| {
            vec![p(vec![text("hello world")])(inner)]
        })
    }).node;

    let mut tr = Transform::new(initial.clone());
    let slice = Slice::new(
        Fragment::from_nodes(vec![Node::new_text("brave new ", Vec::new())]),
        0,
        0,
    );
    tr.replace(7, 7, slice); // Insert at 7
    assert_eq!(tr.doc.text_between(1, 23), "hello brave new world");

    // Invert the step
    let step = &tr.steps[0];
    let inv_step = step.invert(&initial);
    let mut undo_tr = Transform::new(tr.doc.clone());
    undo_tr.step(inv_step).expect("invert should apply");
    assert_eq!(undo_tr.doc.text_between(1, 12), "hello world");
}

#[test]
fn test_transform_add_and_remove_mark() {
    let doc = build_doc(|ctx| {
        ctx.wrap_block("doc", |inner| {
            vec![p(vec![text("emphasized")])(inner)]
        })
    }).node;

    let em = Mark::new("em", None);
    let mut tr = Transform::new(doc);
    tr.add_mark(1, 11, em.clone());

    let child = tr.doc.child(0).child(0);
    assert!(em.is_in_set(child.marks()));

    tr.remove_mark(1, 11, em.clone());
    let child_after = tr.doc.child(0).child(0);
    assert!(!em.is_in_set(child_after.marks()));
}

#[test]
fn test_mapping_mirror_and_recover() {
    use prosemirror_transform::Mapping;
    let mut mapping = Mapping::new();
    let map1 = StepMap::single(5, 5, 0); // Delete 5 chars at pos 5
    let map2 = StepMap::single(5, 0, 5); // Re-insert (undone / mirrored)

    mapping.append_map(map1, None);
    mapping.append_map(map2, Some(0)); // Mirrors map 0

    assert_eq!(mapping.get_mirror(0), Some(1));
    assert_eq!(mapping.get_mirror(1), Some(0));

    // A position inside the deleted range (e.g. 7) should recover its exact position after mirror
    let recovered_pos = mapping.map(7, 1);
    assert_eq!(recovered_pos, 7);
}

#[test]
fn test_golden_transform_fixtures() {
    let json_str = include_str!("../fixtures/golden_fixtures.json");
    let val: serde_json::Value = serde_json::from_str(json_str).expect("parse json");
    let cases = val["transform"].as_array().expect("transform array");

    for case in cases {
        let initial_json = &case["initial_doc"];
        let doc: Node = serde_json::from_value(initial_json.clone()).expect("parse initial");
        let steps_json = case["steps"].as_array().expect("steps");

        let mut tr = Transform::new(doc);
        for s_json in steps_json {
            let step: Step = serde_json::from_value(s_json.clone()).expect("parse step");
            tr.step(step).expect("step must apply cleanly");
        }

        let final_serialized = serde_json::to_value(&tr.doc).expect("serialize");
        assert_eq!(
            &final_serialized,
            &case["final_doc"],
            "Final transformed doc must match JS ProseMirror output 100%"
        );
    }
}
