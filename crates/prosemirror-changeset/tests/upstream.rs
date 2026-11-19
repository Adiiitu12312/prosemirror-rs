use prosemirror_changeset::{Change, Span};
use prosemirror_model::{Fragment, Node};
use prosemirror_schema_basic::schema;

#[path = "upstream/changed_range.rs"]
mod changed_range;
#[path = "upstream/changes.rs"]
mod changes;
#[path = "upstream/diff.rs"]
mod diff;
#[path = "upstream/merge.rs"]
mod merge;
#[path = "upstream/simplify.rs"]
mod simplify;

fn node(name: &str, children: Vec<Node>) -> Node {
    schema().node(name, None, Some(Fragment::from_nodes(children)), None).unwrap()
}

fn text(value: &str) -> Node {
    schema().text(value, None)
}

fn paragraph(value: &str) -> Node {
    node("paragraph", if value.is_empty() { Vec::new() } else { vec![text(value)] })
}

fn doc(children: Vec<Node>) -> Node {
    node("doc", children)
}

fn single_paragraph(value: &str) -> Node {
    doc(vec![paragraph(value)])
}

fn range([from_a, to_a, from_b, to_b]: [usize; 4], metadata: u8) -> Change<u8> {
    Change::new(
        from_a, to_a, from_b, to_b,
        vec![Span::new(to_a - from_a, metadata)],
        vec![Span::new(to_b - from_b, metadata)],
    )
}

fn positions(changes: &[Change<u8>]) -> Vec<[usize; 4]> {
    changes.iter().map(Change::ranges).collect()
}
