use std::collections::HashMap;
use std::sync::OnceLock;

use prosemirror_model::{Node, NodeSpec, Schema};
use prosemirror_test_builder::test_schema;
use prosemirror_transform::{Mapping, StepMap};
use prosemirror_view::{remove_overlap, Decoration, DecorationSet, DecorationType};
use serde_json::{json, Value};

pub fn verify_case(index: usize) {
    static CASES: OnceLock<Value> = OnceLock::new();
    let cases = CASES.get_or_init(|| serde_json::from_str(include_str!("../decoration_cases.json")).unwrap());
    let case = &cases[index];
    let label = format!("{} / {}", case["group"], case["title"]);
    let mut sets: HashMap<usize, DecorationSet> = HashMap::new();
    for step in case["steps"].as_array().unwrap() {
        let previous = read_usize(&step["previous"]).map(|id| &sets[&id]);
        let new_set = match step["op"].as_str().unwrap() {
            "create" => DecorationSet::create(&document(step), spans(step)),
            "add" => previous.unwrap().add(&document(step), spans(step)),
            "remove" => previous.unwrap().remove(&spans(step)),
            "map" => {
                let old = previous.unwrap();
                let mut mapping = Mapping::new();
                for (index, map) in step["maps"].as_array().unwrap().iter().enumerate() {
                    let ranges = serde_json::from_value(map["ranges"].clone()).unwrap();
                    let mirror = read_usize(&map["mirror"]).filter(|&other| other < index);
                    mapping.append_map(StepMap::new_inverted(ranges, map["inverted"].as_bool().unwrap()), mirror);
                }
                let mut dropped = Vec::new();
                let mapped = old.map_with(&mapping, &document(step), |spec| dropped.push(spec.clone()));
                assert_eq!(json!(dropped), step["dropped"], "{label}: dropped specs");
                for path in step["reused"].as_array().unwrap() {
                    assert!(child_at(old, &path[0]).ptr_eq(child_at(&mapped, &path[1])), "{label}: shared subtree");
                }
                mapped
            }
            "find" => {
                let start = read_usize(&step["from"]);
                let end = read_usize(&step["to"]);
                let found = if let Some(names) = step["names"].as_array() {
                    previous.unwrap().find_where(start, end, |spec| names.contains(&spec["name"]))
                } else {
                    previous.unwrap().find(start, end)
                };
                assert_eq!(json!(found.iter().map(span_value).collect::<Vec<_>>()), step["expected"], "{label}: find");
                continue;
            }
            "remove_overlap" => {
                let spans = spans(step);
                let result = remove_overlap(&spans);
                let borrowed = matches!(result, std::borrow::Cow::Borrowed(_));
                assert_eq!(borrowed, step["borrowed"].as_bool().unwrap(), "{label}: borrowed");
                assert_eq!(json!(result.iter().map(span_value).collect::<Vec<_>>()), step["expected"], "{label}: overlap");
                continue;
            }
            unknown => panic!("unknown upstream decoration operation {unknown}"),
        };
        assert_eq!(set_value(&new_set), step["expected"], "{label}: decoration tree");
        sets.insert(read_usize(&step["id"]).unwrap(), new_set);
    }
}

fn child_at<'a>(mut set: &'a DecorationSet, path: &Value) -> &'a DecorationSet {
    for index in path.as_array().unwrap() {
        set = &set.children()[read_usize(index).unwrap()].decorations;
    }
    set
}

fn document(step: &Value) -> Node {
    let extra = step["extra_nodes"].as_array().unwrap();
    if extra.is_empty() {
        return test_schema().node_from_json(&step["doc"]).unwrap();
    }
    let mut nodes = prosemirror_schema_list::add_list_nodes(
        prosemirror_schema_basic::nodes(), "paragraph block*", Some("block"),
    );
    for pair in extra {
        nodes.push((pair[0].as_str().unwrap().into(), NodeSpec {
            inline: pair[1]["inline"].as_bool().unwrap_or(false),
            content: pair[1]["content"].as_str().map(str::to_owned),
            ..Default::default()
        }));
    }
    Schema::new(nodes, prosemirror_schema_basic::marks()).node_from_json(&step["doc"]).unwrap()
}

fn spans(step: &Value) -> Vec<Decoration> {
    step["spans"].as_array().unwrap().iter().map(|span| {
        let from = read_usize(&span["from"]).unwrap();
        let to = read_usize(&span["to"]).unwrap();
        let attrs = serde_json::from_value(span["attrs"].clone()).unwrap();
        let spec = span["spec"].clone();
        match span["kind"].as_str().unwrap() {
            "widget" => Decoration::widget(from, 0, spec),
            "inline" => Decoration::inline(from, to, attrs, spec),
            "node" => Decoration::node(from, to, attrs, spec),
            kind => panic!("unknown span kind {kind}"),
        }
    }).collect()
}

fn read_usize(value: &Value) -> Option<usize> {
    value.as_u64()?.try_into().ok()
}

fn span_value(span: &Decoration) -> Value {
    let (kind, attrs) = match &span.kind {
        DecorationType::Widget { .. } => ("widget", json!({})),
        DecorationType::Inline { attrs, .. } => ("inline", json!(attrs)),
        DecorationType::Node { attrs } => ("node", json!(attrs)),
    };
    json!({"from": span.from, "to": span.to, "kind": kind, "attrs": attrs, "spec": span.spec()})
}

fn set_value(set: &DecorationSet) -> Value {
    let local: Vec<_> = set.local().iter().map(span_value).collect();
    let children: Vec<_> = set.children().iter().map(|child| {
        json!({"from": child.from, "to": child.to, "set": set_value(&child.decorations)})
    }).collect();
    json!({"local": local, "children": children})
}
