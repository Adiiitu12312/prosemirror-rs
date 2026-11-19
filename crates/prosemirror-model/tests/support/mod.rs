#![allow(dead_code, unused_imports)]

use std::sync::OnceLock;

use prosemirror_model::{AttributeSpec, Fragment, Mark, MarkSpec, Node, NodeSpec, Schema, Slice};
use serde_json::{json, Value};

macro_rules! cases {
    ($suite:literal; $($name:ident => $index:literal,)*) => {
        $(
            #[test]
            fn $name() {
                crate::support::run($suite, $index);
            }
        )*
    };
}
pub(crate) use cases;

pub(crate) fn schema(content: &str) -> Schema {
    let block = |content: &str| NodeSpec {
        content: Some(content.to_owned()),
        group: Some("block".to_owned()),
        ..NodeSpec::default()
    };
    let nodes = [
        ("doc", NodeSpec { content: Some(content.to_owned()), ..NodeSpec::default() }),
        ("paragraph", block("inline*")),
        ("blockquote", block("block+")),
        ("horizontal_rule", NodeSpec { group: Some("block".to_owned()), ..NodeSpec::default() }),
        ("heading", NodeSpec {
            attrs: [("level".into(), AttributeSpec::with_default(json!(1)))].into(),
            ..block("inline*")
        }),
        ("code_block", NodeSpec { marks: Some(String::new()), ..block("text*") }),
        ("text", NodeSpec { group: Some("inline".to_owned()), ..NodeSpec::default() }),
        ("image", NodeSpec {
            group: Some("inline".to_owned()),
            inline: true,
            attrs: [
                ("src".into(), AttributeSpec::new(None, Some("string"))),
                ("title".into(), AttributeSpec::with_default(Value::Null)),
                ("alt".into(), AttributeSpec::with_default(Value::Null)),
            ].into(),
            ..NodeSpec::default()
        }),
        ("hard_break", NodeSpec {
            group: Some("inline".to_owned()),
            inline: true,
            selectable: false,
            ..NodeSpec::default()
        }),
        ("ordered_list", NodeSpec {
            attrs: [("order".into(), AttributeSpec::with_default(json!(1)))].into(),
            ..block("list_item+")
        }),
        ("bullet_list", block("list_item+")),
        ("list_item", NodeSpec { content: Some("paragraph block*".to_owned()), ..NodeSpec::default() }),
    ];
    let marks = [
        ("link", MarkSpec {
            attributes: [
                ("href".into(), AttributeSpec::required()),
                ("title".into(), AttributeSpec::with_default(Value::Null)),
            ].into(),
            inclusive: false,
            ..MarkSpec::default()
        }),
        ("em", MarkSpec::default()),
        ("strong", MarkSpec::default()),
        ("code", MarkSpec::default()),
    ];
    Schema::try_new(
        nodes.into_iter().map(|(name, spec)| (name.to_owned(), spec)).collect(),
        marks.into_iter().map(|(name, spec)| (name.to_owned(), spec)).collect(),
        None,
    ).unwrap()
}

fn position(case: &Value, field: &str) -> usize {
    case[field].as_u64().unwrap().try_into().unwrap()
}

pub(crate) fn node(schema: &Schema, value: &Value) -> Node {
    schema.node_from_json(value).unwrap()
}

fn fragment(schema: &Schema, value: &Value) -> Fragment {
    if let Some(content) = value.as_array() {
        Fragment::from_nodes(content.iter().map(|child| node(schema, child)).collect())
    } else {
        Fragment::empty()
    }
}

fn match_content(case: &Value) {
    let schema = schema(case["expr"].as_str().unwrap());
    let start = schema.top_node_type().content_match().clone();
    let matching = case["input"].as_str().unwrap().split_whitespace()
        .try_fold(start, |matched, name| matched.match_type(name));
    assert_eq!(matching.is_some_and(|matched| matched.valid_end()), case["expected"].as_bool().unwrap());
}

fn fill_content(case: &Value, use_middle: bool) {
    let schema = schema(case["expr"].as_str().unwrap());
    let start = schema.top_node_type().content_match();
    let before = fragment(&schema, &case["before"]);
    let after = fragment(&schema, &case["after"]);
    let matched = start.match_fragment(&before, 0, before.child_count()).unwrap();
    let fits = case["fits"].as_bool().unwrap();
    if use_middle {
        let middle = fragment(&schema, &case["mid"]);
        let left = matched.fill_before(&schema, &middle, false, 0);
        let right = left.as_ref().and_then(|left| {
            let prefix = before.append(left).append(&middle);
            start.match_fragment(&prefix, 0, prefix.child_count())?
                .fill_before(&schema, &after, true, 0)
        });
        if fits {
            assert_eq!(left, Some(fragment(&schema, &case["left"])));
            assert_eq!(right, Some(fragment(&schema, &case["right"])));
        } else {
            assert_eq!(right, None);
        }
    } else {
        let result = matched.fill_before(&schema, &after, true, 0);
        assert_eq!(result, fits.then(|| fragment(&schema, &case["result"])));
    }
}

fn mark_case(basic: &Schema, case: &Value) {
    let nodes = [
        ("doc", NodeSpec { content: Some("paragraph+".to_owned()), ..NodeSpec::default() }),
        ("paragraph", NodeSpec { content: Some("text*".to_owned()), ..NodeSpec::default() }),
        ("text", NodeSpec::default()),
    ];
    let marks = [
        ("remark", MarkSpec {
            attributes: [("id".to_owned(), AttributeSpec::required())].into(),
            excludes: Some(String::new()),
            inclusive: false,
            ..MarkSpec::default()
        }),
        ("user", MarkSpec {
            attributes: [("id".to_owned(), AttributeSpec::required())].into(),
            excludes: Some("_".to_owned()),
            ..MarkSpec::default()
        }),
        ("strong", MarkSpec { excludes: Some("em-group".to_owned()), ..MarkSpec::default() }),
        ("em", MarkSpec { group: Some("em-group".to_owned()), ..MarkSpec::default() }),
    ];
    let custom = Schema::try_new(
        nodes.into_iter().map(|(name, spec)| (name.to_owned(), spec)).collect(),
        marks.into_iter().map(|(name, spec)| (name.to_owned(), spec)).collect(),
        None,
    ).unwrap();
    let mark = |value: &Value| {
        let schema = if value["schema"] == "custom" { &custom } else { basic };
        schema.mark_from_json(&value["json"]).unwrap()
    };
    let set = |value: &Value| {
        value.as_array().unwrap().iter().map(&mark).collect::<Vec<Mark>>()
    };
    match case["kind"].as_str().unwrap() {
        "sameSet" => assert_eq!(
            Mark::same_set(&set(&case["a"]), &set(&case["b"])),
            case["expected"].as_bool().unwrap(),
        ),
        "eq" => assert_eq!(
            mark(&case["a"]) == mark(&case["b"]),
            case["expected"].as_bool().unwrap(),
        ),
        "addToSet" => assert_eq!(
            Mark::add_to_set(&set(&case["set"]), mark(&case["mark"])),
            set(&case["expected"]),
        ),
        "removeFromSet" => assert_eq!(
            mark(&case["mark"]).remove_exact_from_set(&set(&case["set"])),
            set(&case["expected"]),
        ),
        "resolved" => {
            let doc = node(basic, &case["doc"]);
            let resolved = doc.resolve(position(case, "from")).unwrap();
            assert_eq!(mark(&case["mark"]).is_in_set(&resolved.marks()), case["expected"].as_bool().unwrap());
        }
        "resolvedSet" => {
            let doc = node(&custom, &case["doc"]);
            let resolved = doc.resolve(position(case, "from")).unwrap();
            assert_eq!(resolved.marks(), set(&case["expected"]));
        }
        _ => panic!("unknown mark case {case}"),
    }
}

pub(crate) fn run(suite: &str, index: usize) {
    static CASES: OnceLock<Value> = OnceLock::new();
    let cases = CASES.get_or_init(|| serde_json::from_str(include_str!("../upstream_cases.json")).unwrap());
    let case = &cases[suite][index];
    if suite == "content" {
        match case["kind"].as_str().unwrap() {
            "match" => match_content(case),
            "fill" => fill_content(case, false),
            "fill3" => fill_content(case, true),
            _ => panic!("unknown content case {case}"),
        }
        return;
    }

    let schema = schema("block+");
    if suite == "mark" {
        mark_case(&schema, case);
        return;
    }
    if suite == "diff" {
        let a = node(&schema, &case["a"]);
        let b = node(&schema, &case["b"]);
        let found = match case["kind"].as_str().unwrap() {
            "start" => a.content().find_diff_start(b.content()),
            "end" => a.content().find_diff_end(b.content()).map(|result| result.a),
            _ => panic!("unknown diff case {case}"),
        };
        assert_eq!(json!(found), case["expected"]);
        return;
    }

    let doc = node(&schema, &case["doc"]);
    let from = position(case, "from");
    let to = position(case, "to");
    match suite {
        "node" => match case["kind"].as_str().unwrap() {
            "cut" => assert_eq!(doc.cut(from, to), node(&schema, &case["expected"])),
            "between" => {
                let mut visited = Vec::new();
                doc.nodes_between(from, to, |child, pos, _, _| {
                    visited.push(child.text().unwrap_or_else(|| child.type_name()).to_owned());
                    if !child.is_text() {
                        assert!(std::ptr::eq(doc.node_at(pos).unwrap(), child));
                    }
                    true
                });
                assert_eq!(json!(visited), case["expected"]);
            }
            _ => panic!("unknown node case {case}"),
        },
        "slice" => {
            let include_parents = case["kind"] == "includeParents";
            let slice = doc.slice(from, to, include_parents).unwrap();
            if include_parents {
                assert_eq!(slice.to_string(), case["expected"].as_str().unwrap());
            } else {
                assert_eq!(slice.content, fragment(&schema, &case["expected"]));
                assert_eq!(slice.open_start, position(case, "start"));
                assert_eq!(slice.open_end, position(case, "end"));
            }
        }
        "replace" => {
            let insert = &case["insert"];
            let slice = if insert.is_null() {
                Slice::empty()
            } else {
                node(&schema, &insert["doc"])
                    .slice(position(insert, "from"), position(insert, "to"), false).unwrap()
            };
            let result = doc.replace(from, to, &slice);
            if let Some(pattern) = case["pattern"].as_str() {
                assert!(result.unwrap_err().to_string().to_lowercase().contains(pattern));
            } else {
                assert_eq!(result.unwrap(), node(&schema, &case["expected"]));
            }
        }
        _ => panic!("unknown test suite {suite}"),
    }
}

