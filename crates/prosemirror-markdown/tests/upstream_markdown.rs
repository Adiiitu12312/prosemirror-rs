//! Cases from test-parse.ts and test-custom-parser.ts in prosemirror-markdown 1.13.4.

use std::sync::OnceLock;

use prosemirror_markdown::{
    default_markdown_parser, default_markdown_serializer, schema, CommonMarkTokenizer,
    MarkdownParser, MarkdownSerializer, MarkdownSerializerOptions, ParseSpec,
};
use serde_json::Value;

fn custom_parser() -> MarkdownParser {
    MarkdownParser::new(
        schema().clone(), CommonMarkTokenizer::default(),
        [
            ("blockquote".to_owned(), ParseSpec::ignore()),
            ("paragraph".to_owned(), ParseSpec::block("paragraph")),
            ("softbreak".to_owned(), ParseSpec::node("hard_break")),
        ],
    ).unwrap()
}

fn run_case(index: usize) {
    static CASES: OnceLock<Vec<Value>> = OnceLock::new();
    let case = &CASES.get_or_init(|| serde_json::from_str(include_str!("upstream_cases.json")).unwrap())[index];
    for check in case["checks"].as_array().unwrap() {
        let expected = schema().node_from_json(&check["doc"]).unwrap();
        let markdown = check["text"].as_str().unwrap();
        match check["operation"].as_str().unwrap() {
            "parse" => assert_eq!(default_markdown_parser().parse(markdown).unwrap(), expected, "{case}"),
            "serialize" => assert_eq!(default_markdown_serializer().serialize(&expected).unwrap(), markdown, "{case}"),
            "customParse" => assert_eq!(custom_parser().parse(markdown).unwrap(), expected, "{case}"),
            "customSerialize" => {
                let defaults = default_markdown_serializer();
                let serializer = MarkdownSerializer::new(defaults.nodes.clone(), defaults.marks.clone(), MarkdownSerializerOptions {
                    escape_extra_characters: Some(regex::Regex::new(check["pattern"].as_str().unwrap()).unwrap()),
                    ..MarkdownSerializerOptions::default()
                });
                assert_eq!(serializer.serialize(&expected).unwrap(), markdown, "{case}");
            }
            _ => panic!("unknown Markdown assertion {check}"),
        }
    }
}

macro_rules! cases {
    ($($name:ident => $index:literal,)*) => {
        $(
            #[test]
            fn $name() {
                run_case($index);
            }
        )*
    };
}

include!("upstream_cases.in");
