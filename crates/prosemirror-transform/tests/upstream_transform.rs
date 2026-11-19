//! The tagged upstream transform and structure tests exercise the Rust API with the
//! original edit arguments, documents, positions, and schema definitions.

use std::collections::HashMap;
use std::sync::OnceLock;

use prosemirror_model::{AttributeSpec, Fragment, MarkSpec, Node, NodeRange, NodeSpec, Schema};
use prosemirror_transform::{can_split, find_wrapping, lift_target, Mapping, MarkFilter, Step, Transform};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct Corpus {
    schemas: Vec<SchemaSpec>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SchemaSpec {
    top_node: String,
    nodes: Vec<(String, Value)>,
    marks: Vec<(String, Value)>,
}

impl SchemaSpec {
    fn build(&self) -> Schema {
        let nodes = self.nodes.iter().map(|(name, spec)| {
            (name.clone(), NodeSpec {
                content: string(&spec["content"]),
                marks: string(&spec["marks"]),
                group: string(&spec["group"]),
                attrs: attributes(&spec["attrs"]),
                inline: flag(spec, "inline"),
                atom: flag(spec, "atom"),
                selectable: spec["selectable"].as_bool().unwrap_or(true),
                draggable: flag(spec, "draggable"),
                code: flag(spec, "code"),
                whitespace: string(&spec["whitespace"]),
                defining: flag(spec, "defining"),
                defining_as_context: flag(spec, "definingAsContext"),
                defining_for_content: flag(spec, "definingForContent"),
                isolating: flag(spec, "isolating"),
                linebreak_replacement: flag(spec, "linebreakReplacement"),
                ..NodeSpec::default()
            })
        }).collect();
        let marks = self.marks.iter().map(|(name, spec)| {
            (name.clone(), MarkSpec {
                attributes: attributes(&spec["attrs"]),
                inclusive: spec["inclusive"].as_bool().unwrap_or(true),
                spanning: spec["spanning"].as_bool().unwrap_or(true),
                excludes: string(&spec["excludes"]),
                group: string(&spec["group"]),
                ..MarkSpec::default()
            })
        }).collect();
        Schema::try_new(nodes, marks, Some(&self.top_node)).expect("valid upstream schema")
    }
}

fn attributes(value: &Value) -> HashMap<String, AttributeSpec> {
    value.as_object().into_iter().flatten().map(|(name, spec)| {
        (name.clone(), AttributeSpec::new(spec.get("default").cloned(), spec["validate"].as_str()))
    }).collect()
}

fn string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

fn flag(value: &Value, name: &str) -> bool {
    value[name].as_bool().unwrap_or(false)
}

fn index(value: &Value) -> Option<usize> {
    value.as_u64().and_then(|number| usize::try_from(number).ok())
}

fn number(value: &Value) -> usize {
    index(value).expect("upstream position")
}

fn node_range(value: &Value) -> NodeRange {
    NodeRange {
        start: number(&value["start"]),
        end: number(&value["end"]),
        depth: number(&value["depth"]),
        start_index: number(&value["startIndex"]),
        end_index: number(&value["endIndex"]),
    }
}

fn range_json(range: NodeRange) -> Value {
    json!({
        "start": range.start, "end": range.end, "depth": range.depth,
        "startIndex": range.start_index, "endIndex": range.end_index,
    })
}

fn type_name(value: &Value) -> &str {
    value["name"].as_str().expect("upstream type name")
}

fn optional_attrs(value: &Value) -> Option<Value> {
    (!value.is_null()).then(|| value.clone())
}

fn template(schema: &Schema, value: &Value) -> Node {
    schema.node_type(type_name(&value["type"])).expect("upstream type")
        .try_create(optional_attrs(&value["attrs"]), None, None).expect("valid node template")
}

fn split_types(schema: &Schema, value: &Value) -> Option<Vec<Option<Node>>> {
    value.as_array().map(|types| {
        types.iter().map(|ty| (!ty.is_null()).then(|| template(schema, ty))).collect()
    })
}

fn fragment(schema: &Schema, value: &Value) -> Fragment {
    match value {
        Value::Null => Fragment::empty(),
        Value::Array(items) => Fragment::from_nodes(
            items.iter().map(|node| schema.node_from_json(node).expect("valid node JSON")).collect()
        ),
        _ => Fragment::from_nodes(vec![schema.node_from_json(value).expect("valid node JSON")]),
    }
}

#[derive(Deserialize)]
struct Case {
    title: String,
    queries: Vec<Query>,
    transforms: Vec<Trace>,
}

#[derive(Deserialize)]
struct Query {
    method: String,
    schema: usize,
    doc: Value,
    args: Value,
    expected: Value,
}

impl Query {
    fn check(&self, schema: &Schema) {
        let doc = schema.node_from_json(&self.doc).expect("valid query document");
        let args = &self.args;
        let found = match self.method.as_str() {
            "canSplit" => {
                let types = split_types(schema, &args[2]);
                json!(can_split(&doc, number(&args[0]), index(&args[1]).unwrap_or(1), types.as_deref()))
            }
            "findWrapping" => {
                let range = node_range(&args[0]);
                let inner = (!args[3].is_null()).then(|| node_range(&args[3]));
                json!(find_wrapping(&doc, &range, type_name(&args[1]), optional_attrs(&args[2]), inner.as_ref()))
            }
            "liftTarget" => json!(lift_target(&doc, &node_range(&args[0]))),
            "blockRange" => {
                let from = doc.resolve(number(&args[0])).expect("valid starting position");
                let to = doc.resolve(number(&args[1])).expect("valid end position");
                from.block_range(&to).map_or(Value::Null, range_json)
            }
            other => panic!("unsupported upstream query {other}"),
        };
        assert_eq!(found, self.expected, "{method} {args}", method = self.method);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Trace {
    schema: usize,
    start: Value,
    operations: Vec<Operation>,
    expected: Value,
    steps: Vec<Value>,
    maps: Vec<Vec<usize>>,
    #[serde(default)]
    round_trip: bool,
    #[serde(default)]
    positions: Vec<(usize, usize)>,
}

impl Trace {
    fn check(&self, schema: &Schema) {
        let before = schema.node_from_json(&self.start).expect("valid upstream starting document");
        let mut transform = Transform::new(before.clone());
        for operation in &self.operations {
            operation.apply(&mut transform, schema);
        }
        let actual = serde_json::to_value(&transform.doc).expect("serializable document");
        assert_eq!(actual, self.expected, "final document");
        assert_eq!(
            transform.steps.iter().map(|step| json!(step)).collect::<Vec<_>>(),
            self.steps,
            "serialized steps",
        );
        assert_eq!(
            transform.mapping.maps.iter().map(|map| map.ranges.clone()).collect::<Vec<_>>(),
            self.maps,
            "step maps",
        );

        if self.round_trip {
            let mut replay = Transform::new(before);
            for step in &self.steps {
                replay.step(Step::from_json(schema, step).expect("valid serialized step"))
                    .expect("step reapplies");
            }
            assert_eq!(replay.doc, transform.doc, "step JSON round trip");
            let mut inverted = Transform::new(transform.doc.clone());
            for (step, doc) in transform.steps.iter().zip(&transform.docs).rev() {
                inverted.step(step.invert(doc)).expect("inverse applies");
            }
            assert_eq!(inverted.doc, *transform.before(), "step inversion");

            let maps = &transform.mapping.maps;
            let mut remapped = Mapping::from_maps(maps.iter().map(|map| map.invert()).collect());
            for (index, map) in maps.iter().enumerate().rev() {
                remapped.append_map(map.clone(), Some(index));
            }
            for &(position, new_position) in &self.positions {
                assert_eq!(transform.mapping.map(position, 1), new_position, "forward mapping");
                assert_eq!(remapped.map(position, 1), position, "mirrored mapping");
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Operation {
    method: String,
    args: Value,
    error: Option<String>,
    attrs_by_node: Option<Vec<(Value, Value)>>,
}

impl Operation {
    fn apply(&self, transform: &mut Transform, schema: &Schema) {
        let args = &self.args;
        let start = transform.steps.len();
        let mark = |json| schema.mark_from_json(json).expect("valid mark JSON");
        let node = |json| schema.node_from_json(json).expect("valid node JSON");
        let slice = |json| schema.slice_from_json(json).expect("valid slice JSON");
        match self.method.as_str() {
            "addMark" => {
                transform.add_mark(number(&args[0]), number(&args[1]), mark(&args[2]));
            }
            "removeMark" => {
                let from = number(&args[0]);
                let to = number(&args[1]);
                if let Some(name) = args[2]["name"].as_str() {
                    transform.remove_marks(from, to, MarkFilter::Type(name));
                } else if args[2].is_null() {
                    transform.remove_marks(from, to, MarkFilter::All);
                } else {
                    transform.remove_mark(from, to, mark(&args[2]));
                }
            }
            "addNodeMark" => {
                transform.add_node_mark(number(&args[0]), mark(&args[1]));
            }
            "removeNodeMark" => {
                if let Some(name) = args[1]["name"].as_str() {
                    transform.remove_node_mark_type(number(&args[0]), name);
                } else {
                    transform.remove_node_mark(number(&args[0]), mark(&args[1]));
                }
            }
            "delete" => {
                transform.delete(number(&args[0]), number(&args[1]));
            }
            "deleteRange" => {
                transform.delete_range(number(&args[0]), number(&args[1]));
            }
            "insert" => {
                transform.insert(number(&args[0]), fragment(schema, &args[1]));
            }
            "join" => {
                transform.join(number(&args[0]), index(&args[1]).unwrap_or(1));
            }
            "lift" => {
                transform.lift(&node_range(&args[0]), number(&args[1]));
            }
            "replace" => {
                let from = number(&args[0]);
                transform.replace(from, index(&args[1]).unwrap_or(from), slice(&args[2]));
            }
            "replaceRange" => {
                transform.replace_range(number(&args[0]), number(&args[1]), slice(&args[2]));
            }
            "replaceRangeWith" => {
                transform.replace_range_with(number(&args[0]), number(&args[1]), node(&args[2]));
            }
            "setBlockType" => {
                let from = number(&args[0]);
                let to = index(&args[1]).unwrap_or(from);
                let type_name = type_name(&args[2]);
                if let Some(attrs_by_node) = &self.attrs_by_node {
                    transform.set_block_type_with(from, to, type_name, |old_node| {
                        let json = json!(old_node);
                        attrs_by_node.iter().find(|(node, _)| node == &json)
                            .and_then(|(_, attrs)| optional_attrs(attrs))
                    });
                } else {
                    transform.set_block_type(from, to, type_name, optional_attrs(&args[3]));
                }
            }
            "setNodeMarkup" => {
                let name = (!args[1].is_null()).then(|| type_name(&args[1]));
                let marks = args[3].as_array().map(|marks| marks.iter().map(mark).collect());
                transform.set_node_markup(number(&args[0]), name, optional_attrs(&args[2]), marks);
            }
            "setNodeAttribute" => {
                transform.set_node_attribute(number(&args[0]), args[1].as_str().unwrap(), args[2].clone());
            }
            "setDocAttribute" => {
                transform.set_doc_attribute(args[0].as_str().unwrap(), args[1].clone());
            }
            "split" => {
                let types = split_types(schema, &args[2]);
                transform.split_with_types(number(&args[0]), index(&args[1]).unwrap_or(1), types.as_deref());
            }
            "step" => {
                let result = transform.step(Step::from_json(schema, &args[0]).expect("valid step JSON"));
                assert_eq!(result.err(), self.error, "explicit step result");
            }
            "wrap" => {
                let wrappers = args[1].as_array().expect("wrapper templates")
                    .iter().map(|node| template(schema, node)).collect::<Vec<_>>();
                transform.wrap(&node_range(&args[0]), &wrappers);
            }
            other => panic!("unsupported upstream operation {other}"),
        }
        if let Some(error) = &self.error {
            assert_eq!(transform.steps.len(), start, "operation should fail: {error}");
        }
    }
}

fn run_case(index: usize) {
    static CORPUS: OnceLock<Corpus> = OnceLock::new();
    let corpus = CORPUS.get_or_init(|| {
        serde_json::from_str(include_str!("upstream/cases.json")).expect("valid upstream test corpus")
    });
    let case = &corpus.cases[index];
    for query in &case.queries {
        query.check(&corpus.schemas[query.schema].build());
    }
    for trace in &case.transforms {
        trace.check(&corpus.schemas[trace.schema].build());
    }
    assert!(!case.title.is_empty(), "upstream test should have a title");
}

macro_rules! upstream_cases {
    ($($name:ident => $index:literal,)*) => {$(
        #[test]
        fn $name() {
            run_case($index);
        }
    )*};
}

include!("upstream/coverage.rs");
