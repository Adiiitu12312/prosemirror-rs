//! Rust port of prosemirror-schema-list@1.4.1/src/schema-list.ts.

mod commands;

pub use commands::{lift_list_item, sink_list_item, split_list_item, split_list_item_keep_marks, wrap_in_list, DispatchFn};

use prosemirror_model::{AttributeSpec, DomAttribute, DomOutputSpec, DomParseRule, NodeSpec, ParseAttrs};
use serde_json::json;

pub fn ordered_list() -> NodeSpec {
    NodeSpec {
        attrs: [("order".into(), AttributeSpec::new(Some(json!(1)), Some("number")))].into(),
        parse_dom: vec![DomParseRule {
            get_tag_attrs: Some(|element| {
                let order = element.attr("start").map_or_else(|| json!(1), |start| {
                    let start = start.trim();
                    if start.is_empty() {
                        json!(0)
                    } else if let Ok(integer) = start.parse::<i64>() {
                        json!(integer)
                    } else {
                        json!(start.parse::<f64>().unwrap_or(f64::NAN))
                    }
                });
                ParseAttrs::Accept(Some(json!({ "order": order })))
            }),
            ..DomParseRule::tag("ol")
        }],
        to_dom: Some(|node| {
            let order = node.attrs().and_then(|attrs| attrs["order"].as_f64()).unwrap_or(1.0);
            let attrs = if order == 1.0 { Vec::new() } else { vec![DomAttribute::new("start", order.to_string())] };
            DomOutputSpec::element_with_attrs("ol", attrs, vec![DomOutputSpec::Hole])
        }),
        ..Default::default()
    }
}

pub fn bullet_list() -> NodeSpec {
    NodeSpec {
        parse_dom: vec![DomParseRule::tag("ul")],
        to_dom: Some(|_| DomOutputSpec::element("ul", vec![DomOutputSpec::Hole])),
        ..Default::default()
    }
}

pub fn list_item() -> NodeSpec {
    NodeSpec {
        defining: true,
        parse_dom: vec![DomParseRule::tag("li")],
        to_dom: Some(|_| DomOutputSpec::element("li", vec![DomOutputSpec::Hole])),
        ..Default::default()
    }
}

pub fn add_list_nodes(
    mut nodes: Vec<(String, NodeSpec)>,
    item_content: &str,
    list_group: Option<&str>,
) -> Vec<(String, NodeSpec)> {
    let lists = [
        (
            "ordered_list",
            NodeSpec {
                content: Some("list_item+".into()),
                group: list_group.map(str::to_owned),
                ..ordered_list()
            },
        ),
        (
            "bullet_list",
            NodeSpec {
                content: Some("list_item+".into()),
                group: list_group.map(str::to_owned),
                ..bullet_list()
            },
        ),
        (
            "list_item",
            NodeSpec { content: Some(item_content.into()), ..list_item() },
        ),
    ];
    nodes.retain(|(name, _)| !lists.iter().any(|(list, _)| name == list));
    nodes.extend(lists.map(|(name, spec)| (name.to_owned(), spec)));
    nodes
}
