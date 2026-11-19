//! Rust port of prosemirror-schema-basic@1.2.3/src/schema-basic.ts.

use prosemirror_model::{AttributeSpec, MarkSpec, NodeSpec, Schema};
use serde_json::{json, Value};
use std::sync::OnceLock;

pub fn nodes() -> Vec<(String, NodeSpec)> {
    let block = || Some("block".into());
    let inline = || Some("inline".into());
    let inline_content = || Some("inline*".into());
    vec![
        (
            "doc".into(),
            NodeSpec { content: Some("block+".into()), ..Default::default() },
        ),
        (
            "paragraph".into(),
            NodeSpec { content: inline_content(), group: block(), ..Default::default() },
        ),
        (
            "blockquote".into(),
            NodeSpec {
                content: Some("block+".into()),
                group: block(),
                defining: true,
                ..Default::default()
            },
        ),
        (
            "horizontal_rule".into(),
            NodeSpec { group: block(), ..Default::default() },
        ),
        (
            "heading".into(),
            NodeSpec {
                attrs: [("level".into(), AttributeSpec::new(Some(json!(1)), Some("number")))].into(),
                content: inline_content(),
                group: block(),
                defining: true,
                ..Default::default()
            },
        ),
        (
            "code_block".into(),
            NodeSpec {
                content: Some("text*".into()),
                marks: Some(String::new()),
                group: block(),
                code: true,
                defining: true,
                ..Default::default()
            },
        ),
        (
            "text".into(),
            NodeSpec { group: inline(), inline: true, ..Default::default() },
        ),
        (
            "image".into(),
            NodeSpec {
                inline: true,
                group: inline(),
                draggable: true,
                attrs: [
                    ("src".into(), AttributeSpec::new(None, Some("string"))),
                    ("alt".into(), AttributeSpec::new(Some(Value::Null), Some("string|null"))),
                    ("title".into(), AttributeSpec::new(Some(Value::Null), Some("string|null"))),
                ].into(),
                ..Default::default()
            },
        ),
        (
            "hard_break".into(),
            NodeSpec { inline: true, group: inline(), selectable: false, ..Default::default() },
        ),
    ]
}

pub fn marks() -> Vec<(String, MarkSpec)> {
    vec![
        (
            "link".into(),
            MarkSpec {
                attributes: [
                    ("href".into(), AttributeSpec::new(None, Some("string"))),
                    ("title".into(), AttributeSpec::new(Some(Value::Null), Some("string|null"))),
                ].into(),
                inclusive: false,
                ..Default::default()
            },
        ),
        ("em".into(), MarkSpec::default()),
        ("strong".into(), MarkSpec::default()),
        ("code".into(), MarkSpec::default()),
    ]
}

pub fn schema() -> &'static Schema {
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    SCHEMA.get_or_init(|| Schema::new(nodes(), marks()))
}
