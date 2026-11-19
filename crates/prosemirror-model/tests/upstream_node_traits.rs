//! Additional cases from prosemirror-model 1.24.1 test/test-node.ts.

mod support;

use prosemirror_model::{AttributeSpec, Fragment, Node, NodeSpec, Schema};
use serde_json::{json, Value};

fn element(name: &str, children: Vec<Value>) -> Value {
    json!({"type":name,"content":children})
}

fn text(value: &str) -> Value {
    json!({"type":"text","text":value})
}

fn marked(value: &str, marks: &[&str]) -> Value {
    let marks: Vec<_> = marks.iter().map(|name| json!({"type": name})).collect();
    json!({"type":"text","text":value,"marks":marks})
}

fn p(children: Vec<Value>) -> Value {
    element("paragraph", children)
}

fn doc(children: Vec<Value>) -> Value {
    element("doc", children)
}

fn image() -> Value {
    json!({"type":"image","attrs":{"src":"img.png"}})
}

fn br() -> Value {
    json!({"type":"hard_break"})
}

fn hr() -> Value {
    json!({"type":"horizontal_rule"})
}

fn from_basic(value: Value) -> Node {
    support::node(&support::schema("block+"), &value)
}

fn custom_schema() -> Schema {
    let block = |expr: &str| NodeSpec { content: Some(expr.to_owned()), ..NodeSpec::default() };
    let nodes = [
        ("doc", block("paragraph+")),
        ("paragraph", block("(text|contact)*")),
        ("text", NodeSpec {
            to_debug_string: Some(|_| "custom_text".to_owned()),
            ..NodeSpec::default()
        }),
        ("contact", NodeSpec {
            inline: true,
            attrs: [
                ("name".to_owned(), AttributeSpec::required()),
                ("email".to_owned(), AttributeSpec::required()),
            ].into(),
            leaf_text: Some(|node| {
                let attrs = node.attrs().unwrap();
                format!("{} <{}>", attrs["name"].as_str().unwrap(), attrs["email"].as_str().unwrap())
            }),
            ..NodeSpec::default()
        }),
        ("hard_break", NodeSpec {
            to_debug_string: Some(|_| "custom_hard_break".to_owned()),
            ..NodeSpec::default()
        }),
    ];
    Schema::try_new(
        nodes.into_iter().map(|(name, spec)| (name.to_owned(), spec)).collect(),
        Vec::new(),
        None,
    ).unwrap()
}

fn contact(schema: &Schema, name: &str, email: &str) -> Node {
    schema.node("contact", Some(json!({"name":name,"email":email})), None, None).unwrap()
}

fn contact_paragraph(schema: &Schema, contact: Node) -> Node {
    let content = Fragment::from_nodes(vec![schema.text("Hello ", None), contact]);
    schema.node("paragraph", None, Some(content), None).unwrap()
}

#[test]
fn to_string_nests() {
    let list = element("bullet_list", vec![
        element("list_item", vec![p(vec![text("hey")]), p(Vec::new())]),
        element("list_item", vec![p(vec![text("foo")])]),
    ]);
    assert_eq!(
        from_basic(doc(vec![list])).to_string(),
        r#"doc(bullet_list(list_item(paragraph("hey"), paragraph), list_item(paragraph("foo"))))"#,
    );
}

#[test]
fn to_string_shows_inline_children() {
    let doc = from_basic(doc(vec![p(vec![text("foo"), image(), br(), text("bar")])]));
    assert_eq!(doc.to_string(), r#"doc(paragraph("foo", image, hard_break, "bar"))"#);
}

#[test]
fn to_string_shows_marks() {
    let doc = from_basic(doc(vec![p(vec![
        text("foo"), marked("bar", &["em"]), marked("quux", &["em", "strong"]), marked("baz", &["code"]),
    ])]));
    assert_eq!(
        doc.to_string(),
        r#"doc(paragraph("foo", em("bar"), em(strong("quux")), code("baz")))"#,
    );
}

#[test]
fn text_between_works_with_a_custom_leaf_text_function() {
    let doc = from_basic(doc(vec![p(vec![text("foo"), image(), br()])]));
    let leaf = |node: &Node| match node.type_name() {
        "image" => "<image>".to_owned(),
        "hard_break" => "<break>".to_owned(),
        _ => String::new(),
    };
    assert_eq!(
        doc.text_between_with(0, doc.content().size(), Some(""), Some(&leaf)),
        "foo<image><break>",
    );
}

#[test]
fn text_between_works_with_leaf_text() {
    let schema = custom_schema();
    let para = contact_paragraph(&schema, contact(&schema, "Alice", "alice@example.com"));
    let doc = schema.node("doc", None, Some(Fragment::from_nodes(vec![para])), None).unwrap();
    assert_eq!(doc.text_between(0, doc.content().size()), "Hello Alice <alice@example.com>");
}

#[test]
fn text_between_prefers_custom_leaf_text() {
    let schema = custom_schema();
    let para = contact_paragraph(&schema, contact(&schema, "Alice", "alice@example.com"));
    let doc = schema.node("doc", None, Some(Fragment::from_nodes(vec![para])), None).unwrap();
    assert_eq!(
        doc.text_between_with(0, doc.content().size(), Some(""), Some(&|_| "<anonymous>".to_owned())),
        "Hello <anonymous>",
    );
}

#[test]
fn text_between_adds_block_separator_around_empty_paragraphs() {
    let doc = from_basic(doc(vec![p(vec![text("one")]), p(Vec::new()), p(vec![text("two")])]));
    assert_eq!(doc.text_between_with(0, 12, Some("\n"), None), "one\n\ntwo");
}

#[test]
fn text_between_adds_block_separator_around_leaf_nodes() {
    let doc = from_basic(doc(vec![p(vec![text("one")]), hr(), hr(), p(vec![text("two")])]));
    assert_eq!(
        doc.text_between_with(0, 12, Some("\n"), Some(&|_| "---".to_owned())),
        "one\n---\n---\ntwo",
    );
}

#[test]
fn text_between_omits_block_separator_around_non_rendered_leaf_nodes() {
    let doc = from_basic(doc(vec![p(vec![text("one")]), hr(), hr(), p(vec![text("two")])]));
    assert_eq!(doc.text_between_with(0, 12, Some("\n"), None), "one\ntwo");
}

#[test]
fn text_content_works_on_a_whole_doc() {
    assert_eq!(from_basic(doc(vec![p(vec![text("foo")])])).text_content(), "foo");
}

#[test]
fn text_content_works_on_a_text_node() {
    assert_eq!(support::schema("block+").text("foo", None).text_content(), "foo");
}

#[test]
fn text_content_works_on_a_nested_element() {
    let list = element("bullet_list", vec![
        element("list_item", vec![p(vec![text("hi")])]),
        element("list_item", vec![p(vec![marked("a", &["em"]), text("b")])]),
    ]);
    assert_eq!(from_basic(doc(vec![list])).text_content(), "hiab");
}

#[test]
fn check_notices_invalid_content() {
    let doc = from_basic(doc(vec![element("list_item", vec![text("x")])]));
    assert!(doc.check().unwrap_err().to_string().contains("Invalid content for node 'doc'"));
}

#[test]
fn check_notices_marks_in_wrong_places() {
    let doc = from_basic(doc(vec![json!({"type":"paragraph","marks":[{"type":"em"}]})]));
    assert!(doc.check().unwrap_err().to_string().contains("Invalid content for node 'doc'"));
}

#[test]
fn check_notices_incorrect_mark_sets() {
    let node = from_basic(marked("a", &["em", "em"]));
    assert!(node.check().unwrap_err().to_string().contains("Invalid collection of marks"));
}

#[test]
fn check_notices_wrong_attribute_types() {
    let schema = support::schema("block+");
    let image = schema.node_type("image").unwrap().create(Some(json!({"src":true})), None, None);
    assert!(image.check().unwrap_err().to_string()
        .contains("Expected value of type string for attribute src on type image, got boolean"));
}

#[test]
fn from_wraps_a_single_node() {
    let empty_para = from_basic(p(Vec::new()));
    let expected = from_basic(doc(vec![p(Vec::new())]));
    assert_eq!(expected.copy(Some(Fragment::from_nodes(vec![empty_para]))), expected);
}

#[test]
fn from_wraps_an_array() {
    let expected = from_basic(p(vec![br(), text("foo")]));
    let inputs = vec![from_basic(br()), from_basic(text("foo"))];
    assert_eq!(expected.copy(Some(Fragment::from_nodes(inputs))), expected);
}

#[test]
fn from_preserves_a_fragment() {
    let expected = from_basic(doc(vec![p(vec![text("foo")])]));
    assert_eq!(expected.copy(Some(expected.content().clone())), expected);
}

#[test]
fn from_accepts_null() {
    let expected = from_basic(p(Vec::new()));
    assert_eq!(expected.copy(None), expected);
}

#[test]
fn from_joins_adjacent_text() {
    let expected = from_basic(p(vec![text("ab")]));
    let fragment = Fragment::from_nodes(vec![from_basic(text("a")), from_basic(text("b"))]);
    assert_eq!(expected.copy(Some(fragment)), expected);
}

fn round_trip(value: Value) {
    let schema = support::schema("block+");
    let first = support::node(&schema, &value);
    let second = support::node(&schema, &serde_json::to_value(&first).unwrap());
    assert_eq!(first, second);
}

#[test]
fn to_json_serializes_a_simple_node() {
    round_trip(doc(vec![p(vec![text("foo")])]));
}

#[test]
fn to_json_serializes_marks() {
    let link = json!({"type":"text", "text":"x", "marks":[{"type":"link","attrs":{"href":"foo"}}]});
    round_trip(doc(vec![p(vec![text("foo"), marked("bar", &["em"]), marked("baz", &["em", "strong"]), text(" "), link])]));
}

#[test]
fn to_json_serializes_inline_leaf_nodes() {
    let mut image = image();
    image["marks"] = json!([{"type":"em"}]);
    round_trip(doc(vec![p(vec![text("foo"), image, marked("bar", &["em"])])]));
}

#[test]
fn to_json_serializes_block_leaf_nodes() {
    round_trip(doc(vec![p(vec![text("a")]), hr(), p(vec![text("b")]), p(Vec::new())]));
}

#[test]
fn to_json_serializes_nested_nodes() {
    let list = element("bullet_list", vec![
        element("list_item", vec![p(vec![text("a")]), p(vec![text("b")])]),
        element("list_item", vec![p(vec![image()])]),
    ]);
    round_trip(doc(vec![element("blockquote", vec![list, p(vec![text("c")])]), p(vec![text("d")])]));
}

#[test]
fn to_string_defaults_for_text() {
    assert_eq!(from_basic(text("hello")).to_string(), r#""hello""#);
}

#[test]
fn to_string_defaults_for_hard_break() {
    assert_eq!(from_basic(br()).to_string(), "hard_break");
}

#[test]
fn to_string_can_be_defined_in_the_node_spec() {
    assert_eq!(custom_schema().text("hello", None).to_string(), "custom_text");
}

#[test]
fn fragment_respects_custom_to_string() {
    let schema = custom_schema();
    let fragment = Fragment::from_nodes(vec![
        schema.text("hello", None),
        schema.node("hard_break", None, None, None).unwrap(),
        schema.text("world", None),
    ]);
    assert_eq!(fragment.to_string(), "<custom_text, custom_hard_break, custom_text>");
}

#[test]
fn leaf_text_customizes_text_content() {
    let schema = custom_schema();
    let contact = contact(&schema, "Bob", "bob@example.com");
    let paragraph = contact_paragraph(&schema, contact.clone());
    assert_eq!(contact.text_content(), "Bob <bob@example.com>");
    assert_eq!(paragraph.text_content(), "Hello Bob <bob@example.com>");
}
