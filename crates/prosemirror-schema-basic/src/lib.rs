//! Rust port of prosemirror-schema-basic@1.2.3/src/schema-basic.ts.

use prosemirror_model::{AttributeSpec, DomAttribute, DomOutputSpec, DomParseRule, MarkSpec, NodeSpec, ParseAttrs, Schema, Whitespace};
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
            NodeSpec {
                content: inline_content(),
                group: block(),
                parse_dom: vec![DomParseRule::tag("p")],
                to_dom: Some(|_| container("p")),
                ..Default::default()
            },
        ),
        (
            "blockquote".into(),
            NodeSpec {
                content: Some("block+".into()),
                group: block(),
                defining: true,
                parse_dom: vec![DomParseRule::tag("blockquote")],
                to_dom: Some(|_| container("blockquote")),
                ..Default::default()
            },
        ),
        (
            "horizontal_rule".into(),
            NodeSpec {
                group: block(),
                parse_dom: vec![DomParseRule::tag("hr")],
                to_dom: Some(|_| DomOutputSpec::element("hr", Vec::new())),
                ..Default::default()
            },
        ),
        (
            "heading".into(),
            NodeSpec {
                attrs: [("level".into(), AttributeSpec::new(Some(json!(1)), Some("number")))].into(),
                content: inline_content(),
                group: block(),
                defining: true,
                parse_dom: (1..=6).map(|level| DomParseRule {
                    get_tag_attrs: Some(|element| {
                        let level = element.name[1..].parse::<u8>().ok();
                        ParseAttrs::Accept(Some(json!({ "level": level })))
                    }),
                    ..DomParseRule::tag(&format!("h{level}"))
                }).collect(),
                to_dom: Some(|node| {
                    let level = node.attrs().and_then(|attrs| attrs["level"].as_u64()).unwrap_or(1);
                    container(&format!("h{level}"))
                }),
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
                parse_dom: vec![DomParseRule {
                    preserve_whitespace: Some(Whitespace::Full),
                    ..DomParseRule::tag("pre")
                }],
                to_dom: Some(|_| DomOutputSpec::element("pre", vec![container("code")])),
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
                parse_dom: vec![DomParseRule {
                    get_tag_attrs: Some(|element| ParseAttrs::Accept(Some(json!({
                        "src": element.attr("src"),
                        "alt": element.attr("alt"),
                        "title": element.attr("title"),
                    })))),
                    ..DomParseRule::tag("img[src]")
                }],
                to_dom: Some(|node| DomOutputSpec::element_with_attrs(
                    "img", html_attributes(node.attrs(), &["src", "alt", "title"]), Vec::new(),
                )),
                ..Default::default()
            },
        ),
        (
            "hard_break".into(),
            NodeSpec {
                inline: true,
                group: inline(),
                selectable: false,
                parse_dom: vec![DomParseRule::tag("br")],
                to_dom: Some(|_| DomOutputSpec::element("br", Vec::new())),
                ..Default::default()
            },
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
                parse_dom: vec![DomParseRule {
                    get_tag_attrs: Some(|element| ParseAttrs::Accept(Some(json!({
                        "href": element.attr("href"),
                        "title": element.attr("title"),
                    })))),
                    ..DomParseRule::tag("a[href]")
                }],
                to_dom: Some(|mark, _| DomOutputSpec::element_with_attrs(
                    "a", html_attributes(mark.attrs.as_ref(), &["href", "title"]), vec![DomOutputSpec::Hole],
                )),
                ..Default::default()
            },
        ),
        ("em".into(), MarkSpec {
            parse_dom: vec![
                DomParseRule::tag("i"),
                DomParseRule::tag("em"),
                DomParseRule::style("font-style=italic"),
                DomParseRule {
                    clear_mark: Some(|mark| mark.type_name == "em"),
                    ..DomParseRule::style("font-style=normal")
                },
            ],
            to_dom: Some(|_, _| container("em")),
            ..Default::default()
        }),
        ("strong".into(), MarkSpec {
            parse_dom: vec![
                DomParseRule::tag("strong"),
                DomParseRule {
                    get_tag_attrs: Some(|element| {
                        if element.style("font-weight") == Some("normal") {
                            ParseAttrs::Reject
                        } else {
                            ParseAttrs::Accept(None)
                        }
                    }),
                    ..DomParseRule::tag("b")
                },
                DomParseRule {
                    clear_mark: Some(|mark| mark.type_name == "strong"),
                    ..DomParseRule::style("font-weight=400")
                },
                DomParseRule {
                    get_style_attrs: Some(|weight| {
                        let numeric = weight.len() >= 3 && matches!(weight.as_bytes().first(), Some(b'5'..=b'9'))
                            && weight.bytes().all(|byte| byte.is_ascii_digit());
                        if weight == "bold" || weight == "bolder" || numeric {
                            ParseAttrs::Accept(None)
                        } else {
                            ParseAttrs::Reject
                        }
                    }),
                    ..DomParseRule::style("font-weight")
                },
            ],
            to_dom: Some(|_, _| container("strong")),
            ..Default::default()
        }),
        ("code".into(), MarkSpec {
            parse_dom: vec![DomParseRule::tag("code")],
            to_dom: Some(|_, _| container("code")),
            ..Default::default()
        }),
    ]
}

pub fn schema() -> &'static Schema {
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    SCHEMA.get_or_init(|| Schema::new(nodes(), marks()))
}

fn container(tag: &str) -> DomOutputSpec {
    DomOutputSpec::element(tag, vec![DomOutputSpec::Hole])
}

fn html_attributes(attrs: Option<&Value>, names: &[&str]) -> Vec<DomAttribute> {
    names.iter().filter_map(|&name| {
        let value = attrs?.get(name)?.as_str()?;
        Some(DomAttribute::new(name, value))
    }).collect()
}
