//! Cases from prosemirror-model 1.24.1 test/test-resolve.ts.

mod support;

use prosemirror_model::Node;
use serde_json::json;

fn text(value: &str) -> serde_json::Value {
    json!({"type":"text", "text":value})
}

fn marked(value: &str) -> serde_json::Value {
    json!({"type":"text", "text":value, "marks":[{"type":"em"}]})
}

fn p(children: Vec<serde_json::Value>) -> serde_json::Value {
    json!({"type":"paragraph", "content":children})
}

fn quote(children: Vec<serde_json::Value>) -> serde_json::Value {
    json!({"type":"blockquote", "content":children})
}

enum Edge<'a> {
    Empty,
    Node(&'a Node),
    Text(&'a str),
}

impl Edge<'_> {
    fn check(self, actual: Option<Node>, pos: usize) {
        match self {
            Self::Empty => assert_eq!(actual, None, "position {pos}"),
            Self::Node(node) => assert_eq!(actual.as_ref(), Some(node), "position {pos}"),
            Self::Text(text) => assert_eq!(actual.as_ref().and_then(Node::text), Some(text), "position {pos}"),
        }
    }
}

#[test]
fn resolve_should_reflect_the_document_structure() {
    let schema = support::schema("block+");
    let doc = support::node(&schema, &json!({
        "type":"doc",
        "content":[p(vec![text("ab")]), quote(vec![p(vec![marked("cd"), text("ef")])])]
    }));
    let p1 = doc.child(0);
    let blk = doc.child(1);
    let p2 = blk.child(0);
    let root = (&doc, 0, 12);
    let first_para = (p1, 1, 3);
    let block = (blk, 5, 11);
    let second_para = (p2, 6, 10);
    use Edge::{Empty, Node as N, Text as T};
    let cases = [
        (vec![root], 0, Empty, N(p1)),
        (vec![root, first_para], 0, Empty, T("ab")),
        (vec![root, first_para], 1, T("a"), T("b")),
        (vec![root, first_para], 2, T("ab"), Empty),
        (vec![root], 4, N(p1), N(blk)),
        (vec![root, block], 0, Empty, N(p2)),
        (vec![root, block, second_para], 0, Empty, T("cd")),
        (vec![root, block, second_para], 1, T("c"), T("d")),
        (vec![root, block, second_para], 2, T("cd"), T("ef")),
        (vec![root, block, second_para], 3, T("e"), T("f")),
        (vec![root, block, second_para], 4, T("ef"), Empty),
        (vec![root, block], 6, N(p2), Empty),
        (vec![root], 12, N(blk), Empty),
    ];
    assert_eq!(cases.len(), doc.content().size() + 1);
    for (pos, (levels, parent_offset, before, after)) in cases.into_iter().enumerate() {
        let resolved = doc.resolve(pos).unwrap();
        assert_eq!(resolved.depth, levels.len() - 1, "position {pos}");
        for (depth, (node, start, end)) in levels.into_iter().enumerate() {
            assert_eq!(resolved.node(depth), node, "position {pos}, depth {depth}");
            assert_eq!(resolved.start(depth), start, "position {pos}, depth {depth}");
            assert_eq!(resolved.end(depth), end, "position {pos}, depth {depth}");
            if depth > 0 {
                assert_eq!(resolved.before(depth), start - 1);
                assert_eq!(resolved.after(depth), end + 1);
            }
        }
        assert_eq!(resolved.parent_offset, parent_offset);
        before.check(resolved.node_before(), pos);
        after.check(resolved.node_after(), pos);
    }
}

#[test]
fn resolve_has_a_working_pos_at_index_method() {
    let schema = support::schema("block+");
    let doc = support::node(&schema, &json!({
        "type":"doc",
        "content":[quote(vec![
            p(vec![text("one")]),
            quote(vec![p(vec![text("two "), marked("three")]), p(vec![text("four")])])
        ])]
    }));
    let resolved = doc.resolve(12).unwrap();
    for (depth, index, expected) in [
        (resolved.depth, 0, 8), (resolved.depth, 1, 12), (resolved.depth, 2, 17),
        (2, 0, 7), (2, 1, 18), (2, 2, 24),
        (1, 0, 1), (1, 1, 6), (1, 2, 25), (0, 0, 0), (0, 1, 26),
    ] {
        assert_eq!(resolved.pos_at_index(index, depth), expected);
    }
}
