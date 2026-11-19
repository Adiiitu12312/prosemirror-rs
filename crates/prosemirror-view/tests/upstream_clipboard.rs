//! Port of prosemirror-view@1.42.9/test/webtest-clipboard.ts using portable DOM fragments.

use prosemirror_model::{DomAttribute, DomNode, DomOutputSpec, DomParseRule, Fragment, Node, NodeSpec, ResolvedPos, Schema, Slice};
use prosemirror_state::Selection;
use prosemirror_test_builder::test_schema;
use prosemirror_view::{parse_from_clipboard, serialize_for_clipboard, ClipboardOptions};
use serde_json::{json, Value};

fn block(tag: &str, children: Vec<DomNode>) -> DomNode {
    DomNode::element(tag, children)
}

fn text(value: &str) -> DomNode {
    DomNode::text(value)
}

fn attrs(mut node: DomNode, attrs: &[(&str, &str)]) -> DomNode {
    if let DomNode::Element(element) = &mut node {
        element.attributes.extend(attrs.iter().map(|&(name, value)| DomAttribute::new(name, value)));
    }
    node
}

fn doc(contents: Value) -> Node {
    test_schema().node_from_json(&json!({"type": "doc", "content": contents})).unwrap()
}

fn paragraph(text: &str) -> Value {
    json!({"type": "paragraph", "content": if text.is_empty() { vec![] } else { vec![json!({"type": "text", "text": text})] }})
}

fn context() -> ResolvedPos {
    doc(json!([paragraph("")])).resolve(1).unwrap()
}

fn html(children: Vec<DomNode>, context: &ResolvedPos) -> Slice {
    parse_from_clipboard(test_schema(), "", Some(block("body", children)), false, context, &ClipboardOptions::default())
        .unwrap().unwrap()
}

fn expect_slice(contents: Value, start: usize, end: usize) -> Slice {
    Slice::new(doc(contents).content().clone(), start, end)
}

fn slice_attr(dom: &DomNode) -> Option<&str> {
    dom.children().first()?.as_element()?.attr("data-pm-slice")
}

#[test]
fn copies_only_the_node_for_a_node_selection() {
    let d = doc(json!([
        {"type":"blockquote","content":[paragraph("a"),{"type":"horizontal_rule"}]},
        paragraph("b")
    ]));
    let slice = Selection::node(4).content(&d).unwrap();
    let copied = serialize_for_clipboard(test_schema(), slice, &ClipboardOptions::default()).unwrap();
    assert_eq!(copied.dom.children()[0].as_element().unwrap().name, "hr");
    assert_eq!(slice_attr(&copied.dom), Some("0 0 []"));
    assert_eq!(html(copied.dom.children().to_vec(), &d.resolve(1).unwrap()), d.slice(4, 5, false).unwrap());
}

#[test]
fn includes_context_for_text_selections() {
    let d = doc(json!([{"type":"blockquote","content":[{"type":"bullet_list","content":[
        {"type":"list_item","content":[paragraph("foo"),paragraph("bar")]}
    ]}]}]));
    let slice = Selection::text(6, 10).content(&d).unwrap();
    let copied = serialize_for_clipboard(test_schema(), slice.clone(), &ClipboardOptions::default()).unwrap();
    assert_eq!(copied.dom.children()[0].as_element().unwrap().name, "li");
    assert_eq!(slice_attr(&copied.dom), Some(r#"2 2 ["blockquote",{},"bullet_list",{}]"#));
    assert_eq!(html(copied.dom.children().to_vec(), &d.resolve(1).unwrap()), slice);
    let parsed = parse_from_clipboard(
        test_schema(), &copied.text, Some(copied.dom), true, &d.resolve(1).unwrap(), &ClipboardOptions::default(),
    ).unwrap().unwrap();
    assert_eq!(parsed, expect_slice(json!([paragraph("o"),paragraph("b")]), 1, 1));
}

#[test]
fn preserves_open_nodes() {
    let d = doc(json!([{"type":"blockquote","content":[{"type":"blockquote","content":[paragraph("foo")]}]}]));
    let slice = Slice::new(Fragment::from_nodes(vec![d.child(0).clone()]), 1, 1);
    let copied = serialize_for_clipboard(test_schema(), slice.clone(), &ClipboardOptions::default()).unwrap();
    assert_eq!(html(copied.dom.children().to_vec(), &d.resolve(1).unwrap()), slice);
}

#[test]
fn uses_clipboard_text_serializer_when_given() {
    let d = doc(json!([paragraph("hello")]));
    let copied = serialize_for_clipboard(test_schema(), d.slice(1, 6, false).unwrap(), &ClipboardOptions {
        text_serializer: Some(&|_| "OK".into()), ..Default::default()
    }).unwrap();
    assert_eq!(copied.text, "OK");
}

#[test]
fn can_read_external_html() {
    let p = || block("p", vec![text("hello")]);
    assert_eq!(html(vec![p(), block("hr", vec![])], &context()), expect_slice(
        json!([paragraph("hello"), {"type":"horizontal_rule"}]), 1, 0,
    ));
    assert_eq!(html(vec![p(), text("bar")], &context()), expect_slice(json!([paragraph("hello"),paragraph("bar")]), 1, 1));
}

#[test]
fn cleans_up_top_level_nodes_in_html() {
    let list = || block("ul", vec![block("li", vec![text("foo")])]);
    let item = |text: &str| json!({"type":"list_item","content":[paragraph(text)]});
    let bullet = json!({"type":"bullet_list","content":[item("foo")]});
    assert_eq!(html(vec![list(), text("bar"), block("br", vec![]), text("baz")], &context()), expect_slice(
        json!([bullet, {"type":"paragraph","content":[{"type":"text","text":"bar"},{"type":"hard_break"},{"type":"text","text":"baz"}]}]), 3, 1,
    ));
    assert_eq!(html(vec![list(), text("bar"), block("br", vec![]), block("p", vec![text("x")])], &context()), expect_slice(
        json!([bullet, {"type":"paragraph","content":[{"type":"text","text":"bar"},{"type":"hard_break"}]},paragraph("x")]), 3, 1,
    ));
    assert_eq!(html(vec![block("li", vec![text("foo")]), block("li", vec![text("bar")]), block("p", vec![text("x")])], &context()), expect_slice(
        json!([{"type":"ordered_list","content":[item("foo"),item("bar")]},paragraph("x")]), 3, 1,
    ));
}

#[test]
fn only_drops_trailing_breaks_in_block_parents() {
    let dom = block("p", vec![block("strong", vec![text("a"), block("br", vec![])]), text(" b")]);
    let expected = json!([{"type":"paragraph","content":[
        {"type":"text","text":"a","marks":[{"type":"strong"}]},
        {"type":"hard_break","marks":[{"type":"strong"}]},
        {"type":"text","text":" b"}
    ]}]);
    assert_eq!(html(vec![dom], &context()), expect_slice(expected, 1, 1));
}

#[test]
fn calls_transform_pasted_dom() {
    let transformed = parse_from_clipboard(test_schema(), "", Some(DomNode::Fragment(vec![text("def")])), false,
        &context(), &ClipboardOptions {
            transform_pasted_dom: Some(&|_| DomNode::Fragment(vec![text("abc")])), ..Default::default()
        },
    ).unwrap().unwrap();
    assert_eq!(transformed, Slice::new(Fragment::from_nodes(vec![test_schema().text("abc", None)]), 0, 0));
}

#[test]
fn calls_transform_pasted_text() {
    let transformed = parse_from_clipboard(test_schema(), "def", None, false, &context(), &ClipboardOptions {
        transform_pasted_text: Some(&|_, _| "abc".to_owned()), ..Default::default()
    }).unwrap().unwrap();
    assert_eq!(transformed, expect_slice(json!([paragraph("abc")]), 1, 1));
}

#[test]
fn allows_text_parsing_to_be_overridden() {
    let transformed = parse_from_clipboard(test_schema(), "abc", None, false, &context(), &ClipboardOptions {
        text_parser: Some(&|text, _, _| Some(Slice::new(
            Fragment::from_nodes(vec![test_schema().text(&text.to_uppercase(), None)]), 0, 0,
        ))), ..Default::default()
    }).unwrap().unwrap();
    assert_eq!(transformed, Slice::new(Fragment::from_nodes(vec![test_schema().text("ABC", None)]), 0, 0));
}

#[test]
fn preserves_attributes() {
    let d = doc(json!([{"type":"ordered_list","attrs":{"order":3},"content":[{"type":"list_item","content":[paragraph("foo")]}]}]));
    let slice = Selection::text(4, 5).content(&d).unwrap();
    let copied = serialize_for_clipboard(test_schema(), slice.clone(), &ClipboardOptions::default()).unwrap();
    assert_eq!(html(copied.dom.children().to_vec(), &d.resolve(1).unwrap()), slice);
}

#[test]
fn uses_style_tags_included_in_the_paste() {
    let children = vec![
        block("p", vec![attrs(block("span", vec![text("bold")]), &[("class", "b")]), text(" text")]),
        block("style", vec![text("span.b { font-weight: bold }")]),
    ];
    let expected = json!([{"type":"paragraph","content":[
        {"type":"text","text":"bold","marks":[{"type":"strong"}]},{"type":"text","text":" text"}
    ]}]);
    assert_eq!(html(children, &context()), expect_slice(expected, 1, 1));
}

fn table_schema() -> Schema {
    Schema::new(vec![
        ("td".into(), NodeSpec {
            content: Some("text*".into()), parse_dom: vec![DomParseRule::tag("td")],
            to_dom: Some(|_| DomOutputSpec::element("td", vec![DomOutputSpec::Hole])), ..Default::default()
        }),
        ("tr".into(), NodeSpec {
            content: Some("td+".into()), parse_dom: vec![DomParseRule::tag("tr")],
            to_dom: Some(|_| DomOutputSpec::element("tr", vec![DomOutputSpec::Hole])), ..Default::default()
        }),
        ("table".into(), NodeSpec {
            content: Some("tr+".into()), parse_dom: vec![DomParseRule::tag("table")],
            to_dom: Some(|_| DomOutputSpec::element("table", vec![DomOutputSpec::element("tbody", vec![DomOutputSpec::Hole])])),
            ..Default::default()
        }),
        ("doc".into(), NodeSpec {content: Some("table+".into()), ..Default::default()}),
        ("text".into(), NodeSpec::default())
    ], vec![])
}

#[test]
fn adds_necessary_wrappers_for_parsing() {
    let schema = table_schema();
    let doc = schema.node_from_json(&json!({"type":"doc","content":[{"type":"table","content":[{"type":"tr","content":[
        {"type":"td","content":[{"type":"text","text":"A"}]},{"type":"td","content":[{"type":"text","text":"B"}]}
    ]}]}]})).unwrap();
    let slice = doc.slice(3, 4, true).unwrap();
    let copied = serialize_for_clipboard(&schema, slice.clone(), &ClipboardOptions::default()).unwrap();
    assert_eq!(copied.dom.children()[0].as_element().unwrap().name, "table");
    let parsed = parse_from_clipboard(&schema, "", Some(copied.dom), false, &doc.resolve(3).unwrap(), &ClipboardOptions::default()).unwrap();
    assert_eq!(parsed, Some(slice));
}

#[test]
fn can_parse_content_wrapped_in_comments() {
    let schema = table_schema();
    let doc = schema.top_node_type().create_and_fill(None, None, None).unwrap().unwrap();
    let row = || block("tr", vec![block("td", vec![block("p", vec![text("123")])])]);
    let table = attrs(block("table", vec![block("tbody", vec![row(), row()])]), &[("data-pm-slice","1 1 -2 []")]);
    let dom = block("body", vec![DomNode::Comment("StartFragment".into()), table, DomNode::Comment("EndFragment".into())]);
    let parsed = parse_from_clipboard(&schema, "", Some(dom), false, &doc.resolve(1).unwrap(), &ClipboardOptions::default()).unwrap().unwrap();
    let slice = schema.slice_from_json(&json!({"openStart":1,"openEnd":1,"content":[
        {"type":"tr","content":[{"type":"td","content":[{"type":"text","text":"123"}]}]},
        {"type":"tr","content":[{"type":"td","content":[{"type":"text","text":"123"}]}]}
    ]})).unwrap();
    assert_eq!(parsed, slice);
}
