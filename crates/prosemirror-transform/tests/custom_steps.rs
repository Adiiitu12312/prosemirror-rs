use std::sync::{Arc, Once};

use prosemirror_model::{Fragment, ModelError, Node, NodeSpec, Schema};
use prosemirror_transform::{CustomStep, Mappable, Step, StepMap, StepResult, Transform};
use serde_json::{json, Map, Value};

const ID: &str = "test.replaceWithMention";

#[derive(Debug)]
struct MentionEdit(Step);

impl MentionEdit {
    fn registered(step: Step) -> Step {
        Step::custom(ID, Self(step))
    }
}

impl CustomStep for MentionEdit {
    fn apply(&self, doc: &Node) -> StepResult {
        self.0.apply(doc)
    }

    fn get_map(&self) -> StepMap {
        self.0.get_map()
    }

    fn invert(&self, doc: &Node) -> Step {
        Self::registered(self.0.invert(doc))
    }

    fn map(&self, mapping: &dyn Mappable) -> Option<Step> {
        self.0.map(mapping).map(Self::registered)
    }

    fn merge(&self, other: &Step) -> Option<Step> {
        self.0.merge(&other.as_custom::<Self>()?.0).map(Self::registered)
    }

    fn json_fields(&self) -> Map<String, Value> {
        Map::from_iter([("edit".into(), serde_json::to_value(&self.0).unwrap())])
    }
}

fn decode_mention(schema: &Schema, json: &Value) -> Result<Arc<dyn CustomStep>, ModelError> {
    Ok(Arc::new(MentionEdit(Step::from_json(schema, &json["edit"])?)))
}

fn register() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| Step::json_id(ID, decode_mention).unwrap());
}

fn fixture() -> (Schema, Node) {
    let schema = Schema::new(vec![
        ("doc".into(), NodeSpec { content: Some("paragraph+".into()), ..NodeSpec::default() }),
        ("paragraph".into(), NodeSpec { content: Some("inline*".into()), ..NodeSpec::default() }),
        ("text".into(), NodeSpec { group: Some("inline".into()), ..NodeSpec::default() }),
        ("mention".into(), NodeSpec { group: Some("inline".into()), inline: true, ..NodeSpec::default() }),
    ], Vec::new());
    let doc = schema.node_from_json(&json!({
        "type": "doc",
        "content": [{"type": "paragraph", "content": [{"type": "text", "text": "abc"}]}],
    })).unwrap();
    (schema, doc)
}

fn insertion(pos: usize) -> Value {
    json!({
        "stepType": ID,
        "edit": {
            "stepType": "replace", "from": pos, "to": pos,
            "slice": {"content": [{"type": "mention"}]},
        },
    })
}

#[test]
fn reserves_builtin_ids_and_rejects_duplicate_registrations() {
    register();
    for id in [ID, "replace", "docAttr"] {
        assert_eq!(Step::json_id(id, decode_mention), Err(ModelError::Serialization(
            format!("Duplicate use of step JSON ID {id}"),
        )));
    }
}

#[test]
fn rejects_missing_and_unknown_step_ids() {
    let (schema, _) = fixture();
    for json in [Value::Null, json!({}), json!({"stepType": ""})] {
        assert_eq!(Step::from_json(&schema, &json), Err(ModelError::Serialization(
            "Invalid input for Step.fromJSON".into(),
        )));
    }
    assert_eq!(Step::from_json(&schema, &json!({"stepType": "test.noDecoder"})),
        Err(ModelError::Serialization("No step type test.noDecoder defined".into())));
}

#[test]
fn schema_aware_decoding_resolves_the_implementation_and_its_payload() {
    register();
    let (schema, doc) = fixture();
    let json = insertion(2);
    let step = Step::from_json(&schema, &json).unwrap();
    assert!(step.as_custom::<MentionEdit>().is_some());
    assert_eq!(serde_json::to_value(&step).unwrap(), json);
    let StepResult::Ok(updated) = step.apply(&doc) else { panic!("inserted mention") };
    let inserted = updated.node_at(2).unwrap();
    assert_eq!(inserted.type_name(), "mention");
    assert!(inserted.is_inline() && inserted.schema().is_some());
}

#[test]
fn transform_resolves_a_custom_step_decoded_with_plain_serde() {
    register();
    let (_, doc) = fixture();
    let json = insertion(2);
    let raw: Step = serde_json::from_value(json.clone()).unwrap();
    assert!(raw.as_custom::<MentionEdit>().is_none());
    assert_eq!(serde_json::to_value(&raw).unwrap(), json);
    assert!(matches!(raw.apply(&doc), StepResult::Ok(_)));

    let mut transform = Transform::new(doc.clone());
    transform.step(raw).unwrap();
    assert_eq!(transform.before(), &doc);
    assert!(transform.steps[0].as_custom::<MentionEdit>().is_some());
    assert_eq!(transform.mapping.map(2, 1), 3);
    assert_eq!(transform.doc.node_at(2).unwrap().type_name(), "mention");
}

#[test]
fn custom_steps_map_invert_and_merge_through_the_step_interface() {
    register();
    let (schema, doc) = fixture();
    let step = Step::from_json(&schema, &insertion(2)).unwrap();
    assert!(step.map(&StepMap::new(vec![1, 3, 0])).is_none());
    let mut transform = Transform::new(doc);
    transform.insert(1, Fragment::from_nodes(vec![schema.text("Z", None)]));
    let doc_before_edit = transform.doc.clone();
    let mapped = step.map(&transform.mapping).unwrap();
    assert_eq!(serde_json::to_value(&mapped).unwrap(), insertion(3));
    let inverse = mapped.invert(&transform.doc);
    assert!(inverse.as_custom::<MentionEdit>().is_some());
    transform.step(mapped).unwrap();
    assert_eq!(transform.doc.node_at(3).unwrap().type_name(), "mention");
    transform.step(inverse).unwrap();
    assert_eq!(transform.doc, doc_before_edit);

    let left = Step::from_json(&schema, &insertion(2)).unwrap();
    let right = Step::from_json(&schema, &insertion(3)).unwrap();
    let merged = left.merge(&right).unwrap();
    assert!(merged.as_custom::<MentionEdit>().is_some());
    let StepResult::Ok(intermediate) = left.apply(&transform.doc) else { panic!("left insertion") };
    assert_eq!(merged.apply(&transform.doc), right.apply(&intermediate));
}

#[test]
fn invalid_custom_payload_keeps_the_transform_unchanged() {
    register();
    let (_, doc) = fixture();
    let step: Step = serde_json::from_value(json!({"stepType": ID})).unwrap();
    let mut transform = Transform::new(doc.clone());
    assert!(transform.step(step).is_err());
    assert_eq!(transform.doc, doc);
    assert!(transform.steps.is_empty());
    assert!(transform.docs.is_empty());
    assert!(transform.mapping.maps.is_empty());
}
