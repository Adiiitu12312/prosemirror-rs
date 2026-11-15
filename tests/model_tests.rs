use prosemirror_model::{Fragment, Mark, Node, Slice};
use prosemirror_test_builder::{build_doc, p, text, test_schema};

#[test]
fn test_node_creation_and_size() {
    let tagged = build_doc(|ctx| {
        ctx.wrap_block("doc", |inner| {
            vec![
                p(vec![text("hello")])(inner),
            ]
        })
    });

    let doc = tagged.node;
    assert_eq!(doc.type_name(), "doc");
    assert_eq!(doc.child_count(), 1);

    let para = doc.child(0);
    assert_eq!(para.type_name(), "paragraph");
    assert_eq!(para.node_size(), 7); // 5 text + 2 delimiters

    assert_eq!(doc.text_between(1, 6), "hello");
}

#[test]
fn test_mark_operations() {
    let em = Mark::new("em", None);
    let strong = Mark::new("strong", None);

    let mut set = Vec::new();
    set = Mark::add_to_set(&set, em.clone());
    assert_eq!(set.len(), 1);
    assert!(em.is_in_set(&set));

    set = Mark::add_to_set(&set, strong.clone());
    assert_eq!(set.len(), 2);

    set = Mark::remove_from_set(&set, "em");
    assert_eq!(set.len(), 1);
    assert!(!em.is_in_set(&set));
    assert!(strong.is_in_set(&set));
}

#[test]
fn test_resolved_pos() {
    let tagged = build_doc(|ctx| {
        ctx.wrap_block("doc", |inner| {
            vec![
                p(vec![text("foo")])(inner),
                p(vec![text("bar")])(inner),
            ]
        })
    });

    let doc = tagged.node;
    let rpos = doc.resolve(2).expect("should resolve");
    assert_eq!(rpos.depth, 1);
    assert_eq!(rpos.parent().type_name(), "paragraph");
    assert_eq!(rpos.start(1), 1);
    assert_eq!(rpos.end(1), 4);
}

#[test]
fn test_golden_model_fixtures() {
    let json_str = include_str!("../fixtures/golden_fixtures.json");
    let val: serde_json::Value = serde_json::from_str(json_str).expect("parse json");
    let model_cases = val["model"].as_array().expect("model array");

    for case in model_cases {
        let doc_json = &case["doc"];
        let node: Node = serde_json::from_value(doc_json.clone()).expect("deserialize doc");
        let serialized = serde_json::to_value(&node).expect("serialize doc");
        assert_eq!(&serialized, doc_json, "Doc JSON serialization must match JS golden fixture");
    }
}
