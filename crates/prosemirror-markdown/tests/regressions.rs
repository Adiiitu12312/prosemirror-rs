use prosemirror_markdown::{
    default_markdown_parser, default_markdown_serializer, schema, MarkdownError,
    MarkdownParser, MarkdownSerializerOptions, ParseSpec, Token,
};
use serde_json::{json, Value};

#[test]
fn preserves_whitespace_only_code_spans_on_parse() {
    let node = default_markdown_parser().parse("Three spaces: `   `").unwrap();
    assert_eq!(node.child(0).child(1).text(), Some("   "));
}

#[test]
fn reference_definitions_supply_links_without_becoming_nodes() {
    let node = default_markdown_parser().parse("[Rust][link]\n\n[link]: https://rust-lang.org").unwrap();
    assert_eq!(node.child_count(), 1);
    let mark = &node.child(0).child(0).marks()[0];
    assert_eq!(mark.type_name, "link");
    assert_eq!(mark.attrs.as_ref().unwrap()["href"], "https://rust-lang.org");
}

#[test]
fn normalizes_internationalized_link_hosts_and_display_text() {
    let parser = default_markdown_parser();
    let doc = parser.parse("<https://xn--mnich-kva.example/a%20b>").unwrap();
    assert_eq!(doc.text_content(), "https://münich.example/a b");
    let mark = &doc.child(0).child(0).marks()[0];
    assert_eq!(mark.attrs.as_ref().unwrap()["href"], "https://xn--mnich-kva.example/a%20b");
    let same = parser.parse("<https://münich.example/a%20b>").unwrap();
    assert_eq!(doc, same);
}

#[test]
fn passes_an_environment_and_complete_token_stream_to_custom_parsers() {
    let parser = MarkdownParser::new(
        schema().clone(),
        |src: &str, env: &mut Value| {
            env["seen"] = json!(src);
            Ok(vec![
                Token::new("heading_open", "h2"),
                Token { content: src.to_owned(), ..Token::new("text", "") },
                Token::new("heading_close", "h2"),
            ])
        },
        [("heading".to_owned(), ParseSpec::block("heading").with_get_attrs(|_, stream, index| {
            Some(json!({"level": stream[index + 2].tag[1..].parse::<usize>().unwrap()}))
        }))],
    ).unwrap();
    let mut env = json!({});
    let doc = parser.parse_with_environment("Custom", &mut env).unwrap();
    assert_eq!(env["seen"], "Custom");
    assert_eq!(doc.child(0).attrs().unwrap()["level"], 2);
    assert_eq!(doc.text_content(), "Custom");
}

#[test]
fn handles_unsupported_nodes_according_to_serializer_strictness() {
    let paragraph = schema().node_from_json(&json!({
        "type":"doc", "content":[{"type":"paragraph","content":[{"type":"text","text":"hi"}]}],
    })).unwrap();
    let mut serializer = default_markdown_serializer().clone();
    serializer.nodes.remove("paragraph");
    assert!(matches!(serializer.serialize(&paragraph), Err(MarkdownError::UnsupportedNode(name)) if name == "paragraph"));
    assert_eq!(serializer.serialize_with_options(&paragraph, MarkdownSerializerOptions {
        strict: Some(false), ..MarkdownSerializerOptions::default()
    }).unwrap(), "hi");
}
