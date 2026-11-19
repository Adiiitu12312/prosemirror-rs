//! Behavioral cases for orderedmap 2.1.1's public API and schema order.

use prosemirror_model::{DomNode, DomParseOptions, DomParseRule, DomParser, MarkSpec, NodeSpec, OrderedMap, Schema};

fn named() -> OrderedMap<i32> {
    [("a", 1), ("b", 2), ("c", 3)].into()
}

fn entries(map: &OrderedMap<i32>) -> Vec<(&str, i32)> {
    map.iter().map(|(name, value)| (name, *value)).collect()
}

#[test]
fn constructs_and_reads_bindings_in_order() {
    let map: OrderedMap<_> = [("b", 2), ("a", 1), ("b", 3), ("c", 4)].into_iter().collect();
    assert_eq!(map.size(), 3);
    assert_eq!(map.get("a"), Some(&1));
    assert_eq!(map.get("missing"), None);
    assert_eq!(entries(&map), [("b", 3), ("a", 1), ("c", 4)]);
    assert!(OrderedMap::<i32>::default().is_empty());
}

#[test]
fn replaces_or_renames_keys_at_their_existing_position() {
    let map = named();
    assert_eq!(entries(&map.update("b", 4, None)), [("a", 1), ("b", 4), ("c", 3)]);
    assert_eq!(entries(&map.update("b", 4, Some("c"))), [("a", 1), ("c", 4)]);
    assert_eq!(entries(&map.update("c", 4, Some("a"))), [("b", 2), ("a", 4)]);
    assert_eq!(entries(&map.update("missing", 4, Some("x"))), [("a", 1), ("b", 2), ("c", 3), ("x", 4)]);
    assert_eq!(entries(&map.update("missing", 4, Some("b"))), [("a", 1), ("c", 3), ("b", 4)]);
    assert_eq!(entries(&map.update("a", 4, Some(""))), [("a", 4), ("b", 2), ("c", 3)]);
    assert_eq!(entries(&map), [("a", 1), ("b", 2), ("c", 3)]);
}

#[test]
fn removes_and_repositions_existing_keys() {
    let map = named();
    assert_eq!(entries(&map.remove("b")), [("a", 1), ("c", 3)]);
    assert_eq!(map.remove("missing"), map);
    assert_eq!(entries(&map.add_to_start("b", 4)), [("b", 4), ("a", 1), ("c", 3)]);
    assert_eq!(entries(&map.add_to_end("b", 4)), [("a", 1), ("c", 3), ("b", 4)]);
    assert_eq!(entries(&map.add_before("b", "x", 4)), [("a", 1), ("x", 4), ("b", 2), ("c", 3)]);
    assert_eq!(entries(&map.add_before("missing", "b", 4)), [("a", 1), ("c", 3), ("b", 4)]);
    assert_eq!(entries(&map.add_before("b", "b", 4)), [("a", 1), ("c", 3), ("b", 4)]);
}

#[test]
fn combines_maps_with_the_other_bindings_taking_precedence() {
    let map = named();
    let other = [("c", 4), ("x", 5), ("a", 6)].into_iter().collect();
    assert_eq!(entries(&map.prepend(&other)), [("c", 4), ("x", 5), ("a", 6), ("b", 2)]);
    assert_eq!(entries(&map.append(&other)), [("b", 2), ("c", 4), ("x", 5), ("a", 6)]);
    assert_eq!(entries(&map.subtract(&other)), [("b", 2)]);
    assert_eq!(map.prepend(&OrderedMap::new()), map);
    assert_eq!(map.append(&OrderedMap::new()), map);
}

#[test]
fn visits_keys_in_order_and_converts_to_a_plain_map() {
    let map = named();
    let mut visited = Vec::new();
    map.for_each(|name, value| visited.push((name.to_owned(), *value)));
    assert_eq!(visited, [("a".to_owned(), 1), ("b".to_owned(), 2), ("c".to_owned(), 3)]);
    assert_eq!(map.to_object(), visited.into_iter().collect());
}

#[test]
fn shares_unchanged_values_without_requiring_a_clone_implementation() {
    struct Value(i32);
    let map = OrderedMap::new().add_to_end("a", Value(1));
    let update = map.add_to_start("b", Value(2));
    assert_eq!(update.get("b").unwrap().0, 2);
    assert!(std::ptr::eq(map.get("a").unwrap(), update.get("a").unwrap()));
    assert!(map.get("b").is_none());
}

#[test]
fn passes_node_and_mark_order_through_to_schema_operations() {
    let mut paragraph = NodeSpec {
        group: Some("block".into()), content: Some("text*".into()),
        parse_dom: vec![DomParseRule::tag("p")], ..NodeSpec::default()
    };
    let nodes = [
        ("doc", NodeSpec { content: Some("block+".into()), ..NodeSpec::default() }),
        ("paragraph", paragraph.clone()),
        ("text", NodeSpec::default()),
    ].into_iter().collect();
    let marks = [("em", MarkSpec::default()), ("strong", MarkSpec::default())].into_iter().collect();
    let baseline = Schema::from_ordered_maps(nodes, marks, None).unwrap();
    paragraph.parse_dom.push(DomParseRule::tag("aside"));
    let nodes = baseline.node_specs().add_before("paragraph", "notice", paragraph);
    let marks = baseline.mark_specs().add_to_start("strong", MarkSpec::default());
    let schema = Schema::from_ordered_maps(nodes, marks, None).unwrap();
    assert_eq!(schema.node_types().map(|kind| kind.name.as_str()).collect::<Vec<_>>(),
        ["doc", "notice", "paragraph", "text"]);
    assert_eq!(schema.mark_types().map(|kind| kind.name.as_str()).collect::<Vec<_>>(), ["strong", "em"]);

    let root = DomNode::element("div", vec![DomNode::element("p", vec![DomNode::text("text")])]);
    let parsed = DomParser::from_schema(&schema).parse(&root, DomParseOptions::default()).unwrap();
    assert_eq!(parsed.child(0).type_name(), "notice");
    let filled = schema.top_node_type().create_and_fill(None, None, None).unwrap().unwrap();
    assert_eq!(filled.child(0).type_name(), "notice");

    let em = schema.mark_type("em").unwrap().create(None);
    let strong = schema.mark_type("strong").unwrap().create(None);
    let text = schema.text("text", Some(vec![em, strong]));
    assert_eq!(text.marks().iter().map(|mark| mark.type_name.as_str()).collect::<Vec<_>>(), ["strong", "em"]);
    assert!(baseline.node_type("notice").is_none());
}
