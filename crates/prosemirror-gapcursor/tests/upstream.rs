//! Port of the upstream archived prosemirror-gapcursor test/test-gapcursor.ts.
//! The test file was added after the 1.4.0 tag used for the implementation.

use prosemirror_gapcursor::valid_gap_cursor;
use prosemirror_model::{Fragment, Node, NodeSpec, Schema};

fn schema() -> Schema {
    Schema::new(
        vec![
            ("doc".into(), NodeSpec { content: Some("block+".into()), ..NodeSpec::default() }),
            ("text".into(), NodeSpec { group: Some("inline".into()), ..NodeSpec::default() }),
            ("paragraph".into(), NodeSpec {
                group: Some("block".into()), content: Some("inline*".into()), ..NodeSpec::default()
            }),
            ("section".into(), NodeSpec {
                group: Some("block".into()), content: Some("block+".into()), ..NodeSpec::default()
            }),
            ("maybe_section".into(), NodeSpec {
                group: Some("block".into()), content: Some("block*".into()), ..NodeSpec::default()
            }),
            ("atom_block".into(), NodeSpec { group: Some("block".into()), atom: true, ..NodeSpec::default() }),
        ],
        Vec::new(),
    )
}

fn node(schema: &Schema, name: &str, children: Vec<Node>) -> Node {
    schema.node(name, None, Some(Fragment::from_nodes(children)), None).unwrap()
}

#[test]
fn allows_a_gap_at_document_edges_adjacent_to_atom_blocks() {
    let schema = schema();
    let doc = node(&schema, "doc", vec![node(&schema, "atom_block", Vec::new())]);
    assert!(valid_gap_cursor(&doc.resolve(0).unwrap()));
    assert!(valid_gap_cursor(&doc.resolve(doc.content().size()).unwrap()));
}

#[test]
fn disallows_a_gap_at_document_edges_adjacent_to_textblocks() {
    let schema = schema();
    let para = node(&schema, "paragraph", vec![schema.text("hi", None)]);
    let doc = node(&schema, "doc", vec![para]);
    assert!(!valid_gap_cursor(&doc.resolve(0).unwrap()));
    assert!(!valid_gap_cursor(&doc.resolve(doc.content().size()).unwrap()));
}

#[test]
fn allows_a_gap_at_nested_block_edges_adjacent_to_atom_blocks() {
    let schema = schema();
    let section = node(&schema, "section", vec![node(&schema, "atom_block", Vec::new())]);
    let end = 1 + section.content().size();
    let doc = node(&schema, "doc", vec![section]);
    let start = doc.resolve(1).unwrap();
    let end = doc.resolve(end).unwrap();
    assert_eq!(start.parent().type_name(), "section");
    assert_eq!(end.parent().type_name(), "section");
    assert!(valid_gap_cursor(&start));
    assert!(valid_gap_cursor(&end));
}

#[test]
fn allows_a_gap_in_an_empty_block() {
    let schema = schema();
    let section = node(&schema, "maybe_section", Vec::new());
    let end = 1 + section.content().size();
    let doc = node(&schema, "doc", vec![section]);
    let start = doc.resolve(1).unwrap();
    let end = doc.resolve(end).unwrap();
    assert_eq!(start.parent().type_name(), "maybe_section");
    assert_eq!(end.parent().type_name(), "maybe_section");
    assert!(valid_gap_cursor(&start));
    assert!(valid_gap_cursor(&end));
}
