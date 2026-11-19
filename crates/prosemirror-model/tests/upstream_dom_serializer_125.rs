//! DOMSerializer cases from prosemirror-model 1.25.13 test/test-dom.ts.

use prosemirror_model::{
    DomAttribute, DomElement, DomNode, DomOutputSpec, DomParseRule, DomSerializer,
    Fragment, NodeSpec, RenderedDom, Schema,
};

#[test]
fn supports_dom_and_content_dom_output_specs_with_element_nodes() {
    let schema = Schema::new(vec![
        ("doc".into(), NodeSpec { content: Some("wrapper+".into()), ..NodeSpec::default() }),
        ("wrapper".into(), NodeSpec {
            content: Some("text*".into()),
            to_dom: Some(|_| DomOutputSpec::Existing(RenderedDom {
                dom: div_with_inner(Vec::new()),
                content_path: Some(vec![0]),
            })),
            parse_dom: vec![DomParseRule { content_element: Some("p.inner".into()), ..DomParseRule::tag("div.outer") }],
            ..NodeSpec::default()
        }),
        ("text".into(), NodeSpec::default()),
    ], Vec::new());
    let content = Fragment::from_nodes(vec![schema.text("hello", None)]);
    let wrapper = schema.node("wrapper", None, Some(content), None).unwrap();
    let actual = DomSerializer::from_schema(&schema)
        .serialize_fragment(&Fragment::from_nodes(vec![wrapper])).unwrap();
    let expected = div_with_inner(vec![DomNode::text("hello")]);
    assert_eq!(actual, DomNode::Fragment(vec![expected]));
}

fn div_with_inner(content: Vec<DomNode>) -> DomNode {
    let inner = DomNode::Element(DomElement {
        attributes: vec![DomAttribute::new("class", "inner")],
        ..DomElement::new("p", content)
    });
    DomNode::Element(DomElement {
        attributes: vec![DomAttribute::new("class", "outer")],
        ..DomElement::new("div", vec![inner])
    })
}

#[test]
fn supports_raw_html_element_output_specs() {
    let rule = DomNode::Element(DomElement::new("hr", Vec::new()));
    let schema = Schema::new(vec![
        ("doc".into(), NodeSpec { content: Some("leaf+".into()), ..NodeSpec::default() }),
        ("leaf".into(), NodeSpec {
            to_dom: Some(|_| DomOutputSpec::Existing(RenderedDom {
                dom: DomNode::Element(DomElement::new("hr", Vec::new())),
                content_path: None,
            })),
            parse_dom: vec![DomParseRule::tag("hr")],
            ..NodeSpec::default()
        }),
        ("text".into(), NodeSpec::default()),
    ], Vec::new());
    let leaf = schema.node("leaf", None, None, None).unwrap();
    let actual = DomSerializer::from_schema(&schema)
        .serialize_fragment(&Fragment::from_nodes(vec![leaf])).unwrap();
    assert_eq!(actual, DomNode::Fragment(vec![rule]));
}

#[test]
fn serializes_text_nodes_directly_and_retains_direct_render_spec_text() {
    let schema = Schema::new(vec![
        ("doc".into(), NodeSpec { content: Some("text*".into()), ..NodeSpec::default() }),
        ("text".into(), NodeSpec::default()),
    ], Vec::new());
    let mut serializer = DomSerializer::from_schema(&schema);
    serializer.nodes.insert("text".into(), |_| DomOutputSpec::element("i", Vec::new()));
    let text = schema.text("hello", None);
    assert_eq!(serializer.serialize_node(&text).unwrap(), DomNode::text("hello"));

    let dom = DomSerializer::render_spec(DomOutputSpec::Text("hello".into())).unwrap();
    assert_eq!(dom.dom, DomNode::text("hello"));

    serializer.nodes.insert("doc".into(), |_| DomOutputSpec::Text("hello".into()));
    let doc = schema.node("doc", None, Some(Fragment::from_nodes(vec![text])), None).unwrap();
    assert!(serializer.serialize_node(&doc).is_err());
}
