//! Port of prosemirror-test-builder@1.1.1/test/test-marks.ts.

use prosemirror_model::{AttributeSpec, MarkSpec, NodeSpec, Schema};
use prosemirror_test_builder::{builders, Builders, TaggedDoc};
use serde_json::json;

fn schema() -> Schema {
    Schema::new(
        vec![
            ("doc".into(), NodeSpec { content: Some("block+".into()), ..NodeSpec::default() }),
            ("p".into(), NodeSpec { content: Some("inline*".into()), group: Some("block".into()), ..NodeSpec::default() }),
            ("text".into(), NodeSpec { group: Some("inline".into()), ..NodeSpec::default() }),
        ],
        vec![("a".into(), MarkSpec {
            attributes: [("href".into(), AttributeSpec::required())].into(),
            excludes: Some(String::new()),
            ..MarkSpec::default()
        })],
    )
}

fn linked_doc(builders: &Builders<'_>, inner_href: Option<&str>, text: &str) -> TaggedDoc {
    let text = if let Some(href) = inner_href {
        builders.mark("a", Some(json!({ "href": href })), vec![text.into()]).unwrap()
    } else {
        text.into()
    };
    let outer = builders.mark("a", Some(json!({ "href": "/foo" })), vec![text]).unwrap();
    let paragraph = builders.node("p", None, vec![outer]).unwrap();
    builders.node("doc", None, vec![paragraph.into()]).unwrap()
}

#[test]
fn deduplicates_identical_marks() {
    let schema = schema();
    let builders = builders(&schema, Default::default());
    let actual = linked_doc(&builders, Some("/foo"), "click <p>here");
    let expected = linked_doc(&builders, None, "click here");
    assert_eq!(actual.node, expected.node);
    assert_eq!(actual.node.node_at(actual.tag("p")).unwrap().marks().len(), 1);
}

#[test]
fn marks_of_same_type_with_different_attributes_are_distinct() {
    let schema = schema();
    let builders = builders(&schema, Default::default());
    let actual = linked_doc(&builders, Some("/bar"), "click <p>here");
    assert_eq!(actual.node.node_at(actual.tag("p")).unwrap().marks().len(), 2);
}
