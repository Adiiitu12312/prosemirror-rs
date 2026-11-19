//! DOM cases from prosemirror-model 1.24.1 test/test-dom.ts.

mod support;

use std::sync::OnceLock;

use prosemirror_model::{
    AttributeSpec, DomAttribute, DomElement, DomNode, DomOutputSpec as Output,
    DomParseOptions as Options, DomParseRule as Rule, DomParser, DomPosition, DomRuleKind,
    DomSerializer, Fragment, MarkSpec, MarkToDom, NodeSpec, NodeToDom, ParseAttrs, Schema,
    Whitespace,
};
use serde_json::{json, Value};

fn attrs_to_dom(data: Option<&Value>, names: &[&str]) -> Vec<DomAttribute> {
    names.iter().filter_map(|name| {
        data?.get(*name)?.as_str().map(|value| DomAttribute::new(*name, value))
    }).collect()
}

fn content(tag: &str) -> Output {
    Output::element(tag, vec![Output::Hole])
}

fn schema() -> Schema {
    let base = support::schema("block+");
    let nodes = base.node_types().map(|kind| {
        let mut spec = kind.spec.clone();
        let (rules, to_dom): (Vec<Rule>, Option<NodeToDom>) = match kind.name.as_str() {
            "paragraph" => (vec![Rule::tag("p")], Some(|_| content("p"))),
            "blockquote" => (vec![Rule::tag("blockquote")], Some(|_| content("blockquote"))),
            "horizontal_rule" => (vec![Rule::tag("hr")], Some(|_| Output::element("hr", Vec::new()))),
            "heading" => {
                let rules = (1..=6).map(|level| Rule {
                    attrs: Some(json!({"level":level})),
                    ..Rule::tag(format!("h{level}"))
                }).collect();
                (rules, Some(|node| content(&format!("h{}", node.attrs().unwrap()["level"]))))
            }
            "code_block" => {
                spec.code = true;
                (
                    vec![Rule { preserve_whitespace: Some(Whitespace::Full), ..Rule::tag("pre") }],
                    Some(|_| Output::element("pre", vec![content("code")])),
                )
            }
            "image" => (
                vec![Rule {
                    get_tag_attrs: Some(|el| ParseAttrs::Accept(Some(json!({
                        "src":el.attr("src"), "alt":el.attr("alt"), "title":el.attr("title"),
                    })))),
                    ..Rule::tag("img[src]")
                }],
                Some(|node| Output::element_with_attrs("img", attrs_to_dom(node.attrs(), &["src", "alt", "title"]), Vec::new())),
            ),
            "hard_break" => (vec![Rule::tag("br")], Some(|_| Output::element("br", Vec::new()))),
            "bullet_list" => (vec![Rule::tag("ul")], Some(|_| content("ul"))),
            "ordered_list" => (
                vec![Rule {
                    get_tag_attrs: Some(|el| ParseAttrs::Accept(Some(json!({
                        "order":el.attr("start").and_then(|start| start.parse::<i64>().ok()).unwrap_or(1),
                    })))),
                    ..Rule::tag("ol")
                }],
                Some(|node| {
                    let order = &node.attrs().unwrap()["order"];
                    let attrs = if order.as_i64() == Some(1) {
                        Vec::new()
                    } else {
                        vec![DomAttribute::new("start", order.to_string())]
                    };
                    Output::element_with_attrs("ol", attrs, vec![Output::Hole])
                }),
            ),
            "list_item" => (vec![Rule::tag("li")], Some(|_| content("li"))),
            _ => (Vec::new(), None),
        };
        spec.parse_dom = rules;
        spec.to_dom = to_dom;
        (kind.name.clone(), spec)
    }).collect();
    let marks = base.mark_types().map(|kind| {
        let mut spec = kind.spec.clone();
        let (rules, to_dom): (Vec<Rule>, Option<MarkToDom>) = match kind.name.as_str() {
            "link" => (
                vec![Rule {
                    get_tag_attrs: Some(|el| ParseAttrs::Accept(Some(json!({
                        "href":el.attr("href"), "title":el.attr("title"),
                    })))),
                    ..Rule::tag("a[href]")
                }],
                Some(|mark, _| Output::element_with_attrs("a", attrs_to_dom(mark.attrs.as_ref(), &["href", "title"]), vec![Output::Hole])),
            ),
            "em" => (
                vec![Rule::tag("i"), Rule::tag("em"), Rule::style("font-style=italic"), Rule {
                    clear_mark: Some(|mark| mark.type_name == "em"),
                    ..Rule::style("font-style=normal")
                }],
                Some(|_, _| content("em")),
            ),
            "strong" => (
                vec![Rule::tag("strong"), Rule {
                    get_tag_attrs: Some(|el| {
                        if el.style("font-weight") == Some("normal") { ParseAttrs::Reject } else { ParseAttrs::Accept(None) }
                    }),
                    ..Rule::tag("b")
                }, Rule {
                    clear_mark: Some(|mark| mark.type_name == "strong"),
                    ..Rule::style("font-weight=400")
                }, Rule {
                    get_style_attrs: Some(|value| {
                        if matches!(value, "bold" | "bolder") || value.parse::<u32>().is_ok_and(|value| value >= 500) {
                            ParseAttrs::Accept(None)
                        } else {
                            ParseAttrs::Reject
                        }
                    }),
                    ..Rule::style("font-weight")
                }],
                Some(|_, _| content("strong")),
            ),
            "code" => (vec![Rule::tag("code")], Some(|_, _| content("code"))),
            _ => (Vec::new(), None),
        };
        spec.parse_dom = rules;
        spec.to_dom = to_dom;
        (kind.name.clone(), spec)
    }).collect();
    Schema::try_new(nodes, marks, None).unwrap()
}

fn dom(value: &Value) -> DomNode {
    if let Some(text) = value.as_str() {
        return DomNode::text(text);
    }
    if let Some(comment) = value["comment"].as_str() {
        return DomNode::Comment(comment.to_owned());
    }
    DomNode::Element(DomElement {
        name: value["name"].as_str().unwrap().to_owned(),
        namespace: value["ns"].as_str().map(str::to_owned),
        attributes: value["attrs"].as_array().unwrap().iter().map(|attr| DomAttribute {
            name: attr["name"].as_str().unwrap().to_owned(),
            value: attr["value"].as_str().unwrap().to_owned(),
            namespace: attr["ns"].as_str().map(str::to_owned),
        }).collect(),
        children: value["children"].as_array().unwrap().iter().map(dom).collect(),
    })
}

fn normalize_dom(dom: &DomNode) -> DomNode {
    fn normalize_children(children: &[DomNode]) -> Vec<DomNode> {
        let mut result = Vec::new();
        for child in children {
            if let (Some(DomNode::Text(last)), DomNode::Text(text)) = (result.last_mut(), child) {
                last.push_str(text);
            } else {
                result.push(normalize_dom(child));
            }
        }
        result
    }

    match dom {
        DomNode::Element(element) => {
            let mut attributes = element.attributes.clone();
            attributes.sort_by(|left, right| (&left.namespace, &left.name).cmp(&(&right.namespace, &right.name)));
            DomNode::Element(DomElement { attributes, children: normalize_children(&element.children), ..element.clone() })
        }
        DomNode::Fragment(children) => DomNode::Fragment(normalize_children(children)),
        leaf => leaf.clone(),
    }
}

fn position(case: &Value, field: &str) -> Option<usize> {
    case[field].as_u64().map(|value| value.try_into().unwrap())
}

fn run_case(index: usize) {
    static CASES: OnceLock<Vec<Value>> = OnceLock::new();
    let case = &CASES.get_or_init(|| serde_json::from_str(include_str!("upstream_dom_cases.json")).unwrap())[index];
    let schema = schema();
    let parser = DomParser::from_schema(&schema);
    let input = dom(&case["dom"]);
    let whitespace = match &case["whitespace"] {
        Value::Bool(true) => Some(Whitespace::Preserve),
        Value::String(mode) if mode == "full" => Some(Whitespace::Full),
        _ => None,
    };
    let top_node = (!case["topNode"].is_null()).then(|| support::node(&schema, &case["topNode"]));
    let top_match = position(case, "topStart").and_then(|start| top_node.as_ref()?.content_match_at(start));
    let options = Options {
        preserve_whitespace: whitespace, from: position(case, "from"), to: position(case, "to"),
        top_node: top_node.as_ref(), top_match: top_match.as_ref(), ..Options::default()
    };
    if case["slice"] == true {
        let expected = Fragment::from_nodes(case["expected"].as_array().unwrap()
            .iter().map(|child| support::node(&schema, child)).collect());
        let slice = parser.parse_slice(&input, options).unwrap();
        assert_eq!(slice.content, expected);
        assert_eq!(slice.open_start, position(case, "start").unwrap());
        assert_eq!(slice.open_end, position(case, "end").unwrap());
    } else {
        let expected = support::node(&schema, &case["expected"]);
        if case["roundTrip"] == true {
            let serialized = DomSerializer::from_schema(&schema).serialize_fragment(expected.content()).unwrap();
            let declared = DomNode::Fragment(input.children().to_vec());
            assert_eq!(normalize_dom(&serialized), normalize_dom(&declared));
            assert_eq!(parser.parse(&serialized, options).unwrap(), expected);
        } else {
            assert_eq!(parser.parse(&input, options).unwrap(), expected);
        }
    }
}

macro_rules! dom_cases {
    ($($name:ident => $index:literal,)*) => {
        $(
            #[test]
            fn $name() {
                run_case($index);
            }
        )*
    };
}

include!("upstream_dom_generated.in");

fn el(name: &str, children: Vec<DomNode>) -> DomNode {
    DomNode::element(name, children)
}

fn txt(text: &str) -> DomNode {
    DomNode::text(text)
}

fn styled(name: &str, style: &str, children: Vec<DomNode>) -> DomNode {
    with_attrs(name, &[("style", style)], children)
}

fn with_attrs(name: &str, attrs: &[(&str, &str)], children: Vec<DomNode>) -> DomNode {
    DomNode::Element(DomElement {
        attributes: attrs.iter().map(|(name, value)| DomAttribute::new(*name, *value)).collect(),
        ..DomElement::new(name, children)
    })
}

fn pm(name: &str, children: Vec<Value>) -> Value {
    json!({"type": name, "content": children})
}

fn paragraph(children: Vec<Value>) -> Value {
    pm("paragraph", children)
}

fn text(value: &str) -> Value {
    json!({"type":"text", "text":value})
}

fn document(children: Vec<Value>) -> Value {
    pm("doc", children)
}

fn mark_text(value: &str, marks: Vec<Value>) -> Value {
    json!({"type":"text", "text":value, "marks":marks})
}

fn custom_schema(
    top: Option<&str>,
    modify: impl FnOnce(&mut Vec<(String, NodeSpec)>, &mut Vec<(String, MarkSpec)>),
) -> Schema {
    let parser_schema = schema();
    let mut nodes = parser_schema.node_types().map(|kind| (kind.name.clone(), kind.spec.clone())).collect();
    let mut marks = parser_schema.mark_types().map(|kind| (kind.name.clone(), kind.spec.clone())).collect();
    modify(&mut nodes, &mut marks);
    Schema::try_new(nodes, marks, top).unwrap()
}

fn check_round_trip(schema: &Schema, input: DomNode, content: Value) {
    let expected = support::node(schema, &content);
    let serializer = DomSerializer::from_schema(schema);
    let dom = serializer.serialize_fragment(expected.content()).unwrap();
    assert_eq!(normalize_dom(&dom), normalize_dom(&input));
    let parser = DomParser::from_schema(schema);
    assert_eq!(parser.parse(&dom, Options::default()).unwrap(), expected);
}

#[test]
fn can_parse_marks_on_block_nodes() {
    let schema = custom_schema(None, |nodes, marks| {
        nodes.iter_mut().find(|(name, _)| name == "doc").unwrap().1.marks = Some("comment".to_owned());
        marks.push(("comment".to_owned(), MarkSpec {
            parse_dom: vec![Rule::tag("div.comment")],
            to_dom: Some(|_, _| Output::element_with_attrs(
                "div", vec![DomAttribute::new("class", "comment")], vec![Output::Hole],
            )),
            ..MarkSpec::default()
        }));
    });
    let mut second = paragraph(vec![text("two")]);
    let mut third = paragraph(vec![mark_text("three", vec![json!({"type":"strong"})])]);
    second["marks"] = json!([{"type":"comment"}]);
    third["marks"] = second["marks"].clone();
    check_round_trip(&schema, DomNode::Fragment(vec![
        el("p", vec![txt("one")]),
        with_attrs("div", &[("class", "comment")], vec![
            el("p", vec![txt("two")]), el("p", vec![el("strong", vec![txt("three")])]),
        ]),
        el("p", vec![txt("four")]),
    ]), document(vec![paragraph(vec![text("one")]), second, third, paragraph(vec![text("four")])]));
}

#[test]
fn parses_unique_non_exclusive_same_typed_marks() {
    let schema = custom_schema(None, |_, marks| marks.push(("comment".to_owned(), MarkSpec {
        attributes: [("id".to_owned(), AttributeSpec::with_default(Value::Null))].into(),
        excludes: Some(String::new()),
        parse_dom: vec![Rule {
            get_tag_attrs: Some(|el| ParseAttrs::Accept(Some(json!({
                "id":el.attr("data-id").and_then(|value| value.parse::<i64>().ok()),
            })))),
            ..Rule::tag("span.comment")
        }],
        to_dom: Some(|mark, _| Output::element_with_attrs("span", vec![
            DomAttribute::new("class", "comment"),
            DomAttribute::new("data-id", mark.attrs.as_ref().unwrap()["id"].to_string()),
        ], vec![Output::Hole])),
        ..MarkSpec::default()
    })));
    let dom = DomNode::Fragment(vec![el("p", vec![with_attrs("span", &[("class","comment"),("data-id","1")], vec![
        with_attrs("span", &[("class","comment"),("data-id","2")], vec![txt("double comment")]),
    ])])]);
    let marks = vec![json!({"type":"comment","attrs":{"id":1}}), json!({"type":"comment","attrs":{"id":2}})];
    check_round_trip(&schema, dom, document(vec![paragraph(vec![mark_text("double comment", marks)])]));
}

#[test]
fn serializes_non_spanning_marks_correctly() {
    let schema = custom_schema(None, |_, marks| marks.push(("test".to_owned(), MarkSpec {
        parse_dom: vec![Rule::tag("test")],
        to_dom: Some(|_, _| content("test")),
        spanning: false,
        ..MarkSpec::default()
    })));
    let input = DomNode::Fragment(vec![el("p", vec![
        el("test", vec![txt("a")]), el("test", vec![with_attrs("img", &[("src", "x")], Vec::new())]), el("test", vec![txt("b")]),
    ])]);
    let mark = json!({"type":"test"});
    let image = json!({"type":"image","attrs":{"src":"x"},"marks":[mark.clone()]});
    check_round_trip(&schema, input, document(vec![paragraph(vec![
        mark_text("a", vec![mark.clone()]), image, mark_text("b", vec![mark]),
    ])]));
}

#[test]
fn serializes_an_element_and_an_attribute_with_xml_namespace() {
    const SVG: &str = "http://www.w3.org/2000/svg";
    const XLINK: &str = "http://www.w3.org/1999/xlink";
    let schema = Schema::new(vec![
        ("doc".to_owned(), NodeSpec { content: Some("svg*".to_owned()), ..NodeSpec::default() }),
        ("text".to_owned(), NodeSpec::default()),
        ("svg".to_owned(), NodeSpec {
            parse_dom: vec![Rule::tag("svg").with_namespace(SVG)],
            to_dom: Some(|_| Output::element(
                format!("{SVG} svg"),
                vec![Output::element_with_attrs("use", vec![DomAttribute {
                    name: "href".to_owned(), value: "#svg-id".to_owned(), namespace: Some(XLINK.to_owned()),
                }], Vec::new())],
            )),
            ..NodeSpec::default()
        }),
    ], Vec::new());
    let expected = DomNode::Fragment(vec![DomNode::Element(DomElement {
        namespace: Some(SVG.to_owned()),
        ..DomElement::new("svg", vec![DomNode::Element(DomElement {
            namespace: Some(SVG.to_owned()),
            attributes: vec![DomAttribute {
                name: "href".to_owned(),
                value: "#svg-id".to_owned(),
                namespace: Some(XLINK.to_owned()),
            }],
            ..DomElement::new("use", Vec::new())
        })])
    })]);
    check_round_trip(&schema, expected, document(vec![pm("svg", Vec::new())]));
}

#[test]
fn can_parse_nested_mark_with_same_type_and_different_attrs() {
    let schema = custom_schema(None, |_, marks| marks.push(("s".to_owned(), MarkSpec {
        attributes: [("data-s".to_owned(), AttributeSpec::with_default(json!("tag")))].into(),
        excludes: Some(String::new()),
        parse_dom: vec![Rule::tag("s"), Rule {
            attrs: Some(json!({"data-s":"style"})),
            ..Rule::style("text-decoration")
        }],
        ..MarkSpec::default()
    })));
    let parser = DomParser::from_schema(&schema);
    let style = "text-decoration: line-through;";
    let style_mark = json!({"type":"s","attrs":{"data-s":"style"}});
    let tag_mark = json!({"type":"s","attrs":{"data-s":"tag"}});
    let both = || mark_text("o", vec![style_mark.clone(), tag_mark.clone()]);
    let style_only = || mark_text("o", vec![style_mark.clone()]);
    for (input, expected) in [
        (
            styled("p", style, vec![txt("o"), styled("s", style, vec![txt("o")]), txt("o")]),
            paragraph(vec![style_only(), both(), style_only()]),
        ),
        (
            el("p", vec![styled("span", style, vec![styled("s", style, vec![txt("o")]), txt("o")]), txt("o")]),
            paragraph(vec![both(), style_only(), text("o")]),
        ),
    ] {
        let slice = parser.parse_slice(&DomNode::Fragment(vec![input]), Options::default()).unwrap();
        assert_eq!((slice.open_start, slice.open_end), (1, 1));
        assert_eq!(slice.content.first_child(), Some(&support::node(&schema, &expected)));
    }
}

#[test]
fn can_temporarily_shadow_a_mark_with_another_configuration_of_same_type() {
    let schema = custom_schema(None, |_, marks| {
        marks.clear();
        marks.push(("color".to_owned(), MarkSpec {
            attributes: [("color".to_owned(), AttributeSpec::required())].into(),
            parse_dom: vec![Rule {
                get_style_attrs: Some(|value| ParseAttrs::Accept(Some(json!({"color":value})))),
                ..Rule::style("color")
            }],
            ..MarkSpec::default()
        }));
    });
    let input = DomNode::Fragment(vec![el("p", vec![styled("span", "color:red", vec![
        txt("abc"), styled("span", "color:blue", vec![txt("def")]), txt("ghi"),
    ])])]);
    let colored = |value, color| mark_text(value, vec![json!({"type":"color","attrs":{"color":color}})]);
    let expect = support::node(&schema, &document(vec![paragraph(vec![
        colored("abc","red"), colored("def","blue"), colored("ghi","red"),
    ])]));
    assert_eq!(DomParser::from_schema(&schema).parse(&input, Options::default()).unwrap(), expect);
}

fn check_position(dom: DomNode, path: Vec<usize>, offset: usize, expected_pos: usize, expected_doc: Value) {
    let schema = schema();
    let mut positions = [DomPosition { path, offset, pos: None }];
    let options = Options { find_positions: Some(&mut positions), ..Options::default() };
    let parsed = DomParser::from_schema(&schema).parse(&dom, options).unwrap();
    assert_eq!(parsed, support::node(&schema, &expected_doc));
    assert_eq!(positions[0].pos, Some(expected_pos));
}

#[test]
fn can_find_a_position_at_the_start_of_a_paragraph() {
    check_position(el("div", vec![el("p", vec![txt("hello")])]), vec![0], 0, 1, document(vec![paragraph(vec![text("hello")])]));
}

#[test]
fn can_find_a_position_at_the_end_of_a_paragraph() {
    check_position(el("div", vec![el("p", vec![txt("hello")])]), vec![0], 1, 6, document(vec![paragraph(vec![text("hello")])]));
}

#[test]
fn can_find_a_position_inside_text() {
    check_position(el("div", vec![el("p", vec![txt("hello")])]), vec![0, 0], 3, 4, document(vec![paragraph(vec![text("hello")])]));
}

#[test]
fn can_find_a_position_inside_an_ignored_node() {
    let input = el("div", vec![el("p", vec![txt("hi")]), el("object", vec![txt("foo")]), el("p", vec![txt("ok")])]);
    check_position(input, vec![1], 0, 4, document(vec![paragraph(vec![text("hi")]), paragraph(vec![text("ok")])]));
}

#[test]
fn can_find_a_position_between_nodes() {
    let input = el("div", vec![el("ul", vec![el("li", vec![txt("foo")]), el("li", vec![txt("bar")])])]);
    let expected = document(vec![pm("bullet_list", vec![
        pm("list_item", vec![paragraph(vec![text("foo")])]), pm("list_item", vec![paragraph(vec![text("bar")])]),
    ])]);
    check_position(input, vec![0], 1, 8, expected);
}

#[test]
fn can_find_a_position_at_the_start_of_the_document() {
    let input = el("div", vec![el("p", vec![txt("hi")])]);
    check_position(input, Vec::new(), 0, 0, document(vec![paragraph(vec![text("hi")])]));
}

#[test]
fn can_find_a_position_at_the_end_of_the_document() {
    let input = el("div", vec![el("p", vec![txt("hi")])]);
    check_position(input, Vec::new(), 1, 4, document(vec![paragraph(vec![text("hi")])]));
}

#[test]
fn uses_a_custom_top_node_when_parsing() {
    let schema = custom_schema(Some("blockquote"), |_, _| ());
    check_round_trip(&schema, DomNode::Fragment(vec![el("p", vec![txt("hello")])]),
        pm("blockquote", vec![paragraph(vec![text("hello")])]));
}
