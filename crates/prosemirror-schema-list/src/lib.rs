//! Rust port of prosemirror-schema-list@1.4.1/src/schema-list.ts.

mod commands;

pub use commands::{lift_list_item, sink_list_item, split_list_item, split_list_item_keep_marks, wrap_in_list, DispatchFn};

use prosemirror_model::{AttributeSpec, NodeSpec};
use serde_json::json;

pub fn ordered_list() -> NodeSpec {
    NodeSpec {
        attrs: [("order".into(), AttributeSpec::new(Some(json!(1)), Some("number")))].into(),
        ..Default::default()
    }
}

pub fn bullet_list() -> NodeSpec {
    NodeSpec::default()
}

pub fn list_item() -> NodeSpec {
    NodeSpec { defining: true, ..Default::default() }
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
