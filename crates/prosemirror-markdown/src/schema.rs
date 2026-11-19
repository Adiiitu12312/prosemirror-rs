use std::sync::OnceLock;

use prosemirror_model::{AttributeSpec, DomAttribute, DomOutputSpec, DomParseRule, Node, NodeSpec, ParseAttrs, Schema};
use serde_json::{json, Value};

/// The document schema used by the default CommonMark parser and serializer.
pub fn schema() -> &'static Schema {
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        let mut nodes = prosemirror_schema_basic::nodes();
        for (name, spec) in &mut nodes {
            for attr in spec.attrs.values_mut() {
                attr.validate = None;
            }
            match name.as_str() {
                "blockquote" => spec.defining = false,
                "horizontal_rule" => spec.to_dom = Some(|_| DomOutputSpec::element(
                    "div", vec![DomOutputSpec::element("hr", Vec::new())],
                )),
                "heading" => spec.content = Some("(text | image)*".to_owned()),
                "code_block" => {
                    spec.attrs.insert("params".to_owned(), AttributeSpec::with_default(json!("")));
                    spec.parse_dom[0].get_tag_attrs = Some(|node| {
                        ParseAttrs::Accept(Some(json!({"params": node.attr("data-params").unwrap_or_default()})))
                    });
                    spec.to_dom = Some(|node| {
                        let attributes = text_attr(node, "params").filter(|value| !value.is_empty())
                            .map(|params| DomAttribute::new("data-params", params))
                            .into_iter().collect();
                        DomOutputSpec::element_with_attrs("pre", attributes, vec![container("code")])
                    });
                }
                _ => (),
            }
        }
        let text_index = nodes.iter().position(|(name, _)| name == "text").expect("basic schema has text");
        nodes.splice(text_index..text_index, [
            ("ordered_list".to_owned(), NodeSpec {
                attrs: [
                    ("order".to_owned(), AttributeSpec::with_default(json!(1))),
                    ("tight".to_owned(), AttributeSpec::with_default(json!(false))),
                ].into(),
                parse_dom: vec![DomParseRule {
                    get_tag_attrs: Some(|el| ParseAttrs::Accept(Some(json!({
                        "order": el.attr("start").map_or_else(|| json!(1), parse_number),
                        "tight": el.attr("data-tight").is_some(),
                    })))),
                    ..DomParseRule::tag("ol")
                }],
                to_dom: Some(|node| {
                    let mut attrs = Vec::new();
                    if let Some(start) = node.attrs().and_then(|attrs| attrs.get("order")) {
                        if start.as_f64() != Some(1.0) {
                            attrs.push(DomAttribute::new("start", start.to_string()));
                        }
                    }
                    attrs.extend(tight_attribute(node));
                    DomOutputSpec::element_with_attrs("ol", attrs, vec![DomOutputSpec::Hole])
                }),
                ..list_spec()
            }),
            ("bullet_list".to_owned(), NodeSpec {
                attrs: [("tight".to_owned(), AttributeSpec::with_default(json!(false)))].into(),
                parse_dom: vec![DomParseRule {
                    get_tag_attrs: Some(|el| ParseAttrs::Accept(Some(json!({"tight": el.attr("data-tight").is_some()})))),
                    ..DomParseRule::tag("ul")
                }],
                to_dom: Some(|node| DomOutputSpec::element_with_attrs(
                    "ul", tight_attribute(node).into_iter().collect(), vec![DomOutputSpec::Hole],
                )),
                ..list_spec()
            }),
            ("list_item".to_owned(), NodeSpec {
                content: Some("block+".to_owned()),
                defining: true,
                parse_dom: vec![DomParseRule::tag("li")],
                to_dom: Some(|_| container("li")),
                ..NodeSpec::default()
            }),
        ]);
        let mut marks = prosemirror_schema_basic::marks();
        marks.rotate_left(1);
        let code = marks.remove(2);
        marks.push(code);
        for (name, spec) in &mut marks {
            for attr in spec.attributes.values_mut() {
                attr.validate = None;
            }
            match name.as_str() {
                "em" => spec.to_dom = Some(|_, _| DomOutputSpec::element("em", Vec::new())),
                "strong" => spec.to_dom = Some(|_, _| DomOutputSpec::element("strong", Vec::new())),
                "code" => {
                    spec.code = true;
                    spec.to_dom = Some(|_, _| DomOutputSpec::element("code", Vec::new()));
                }
                _ => (),
            }
        }
        Schema::new(nodes, marks)
    })
}

fn text_attr<'a>(node: &'a Node, name: &str) -> Option<&'a str> {
    node.attrs()?.get(name)?.as_str()
}

fn tight_attribute(node: &Node) -> Option<DomAttribute> {
    node.attrs()?.get("tight")?.as_bool()?.then(|| DomAttribute::new("data-tight", "true"))
}

fn parse_number(value: &str) -> Value {
    let value = value.trim();
    if value.is_empty() {
        json!(0)
    } else if let Ok(value) = value.parse::<i64>() {
        json!(value)
    } else {
        value.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map_or(Value::Null, Value::Number)
    }
}

fn container(tag: &str) -> DomOutputSpec {
    DomOutputSpec::element(tag, vec![DomOutputSpec::Hole])
}

fn list_spec() -> NodeSpec {
    NodeSpec {
        content: Some("list_item+".to_owned()),
        group: Some("block".to_owned()),
        ..NodeSpec::default()
    }
}
