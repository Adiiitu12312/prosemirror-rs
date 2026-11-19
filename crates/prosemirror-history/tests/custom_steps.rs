use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use prosemirror_history::{history, redo, redo_depth, undo, undo_depth, HistoryOptions};
use prosemirror_model::{ModelError, Node, Schema};
use prosemirror_state::{EditorState, EditorStateConfig};
use prosemirror_test_builder::{doc, p, test_schema};
use prosemirror_transform::{CustomStep, Mappable, Step, StepMap, StepResult};
use serde_json::{json, Map, Value};

const ID: &str = "test.historyReplace";
static DECODES: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
struct HistoryEdit(Step);

impl CustomStep for HistoryEdit {
    fn apply(&self, doc: &Node) -> StepResult {
        self.0.apply(doc)
    }

    fn get_map(&self) -> StepMap {
        self.0.get_map()
    }

    fn invert(&self, doc: &Node) -> Step {
        Step::custom(ID, Self(self.0.invert(doc)))
    }

    fn map(&self, mapping: &dyn Mappable) -> Option<Step> {
        self.0.map(mapping).map(|step| Step::custom(ID, Self(step)))
    }

    fn json_fields(&self) -> Map<String, Value> {
        Map::from_iter([("edit".into(), serde_json::to_value(&self.0).unwrap())])
    }
}

fn decode(schema: &Schema, json: &Value) -> Result<Arc<dyn CustomStep>, ModelError> {
    DECODES.fetch_add(1, Ordering::Relaxed);
    Ok(Arc::new(HistoryEdit(Step::from_json(schema, &json["edit"])?)))
}

#[test]
fn persisted_custom_steps_rebind_and_survive_history_remapping() {
    Step::json_id(ID, decode).unwrap();
    let schema = test_schema();
    let plugin = history(HistoryOptions::default());
    let mut state = EditorState::create(EditorStateConfig {
        schema: schema.clone(),
        doc: Some(doc!(p!("ab")).node),
        selection: None,
        stored_marks: None,
        plugins: vec![plugin.clone()],
    });
    let step = serde_json::from_value(json!({
        "stepType": ID,
        "edit": {
            "stepType": "replace", "from": 2, "to": 2,
            "slice": {"content": [{"type": "text", "text": "X"}]},
        },
    })).unwrap();
    let mut transaction = state.tr();
    transaction.step(step).unwrap();
    state = state.apply(&transaction);
    assert_eq!(state.doc, doc!(p!("aXb")).node);
    assert_eq!(undo_depth(&state), 1);
    assert_eq!(DECODES.load(Ordering::Relaxed), 1);

    let mut remote = state.tr();
    remote.insert_text("!", 1, 1).set_meta("addToHistory", false.into());
    state = state.apply(&remote);
    assert_eq!(state.doc, doc!(p!("!aXb")).node);
    assert_eq!(DECODES.load(Ordering::Relaxed), 1);

    let fields = [("undoHistory", &plugin)];
    let json = state.to_json_with_plugins(&fields).unwrap();
    assert_eq!(json["undoHistory"]["done"]["items"][0]["step"]["stepType"], ID);
    state = EditorState::from_json_with_plugins(schema, &json, vec![plugin.clone()], &fields).unwrap();
    assert_eq!(undo_depth(&state), 1);
    assert_eq!(DECODES.load(Ordering::Relaxed), 2);
    assert!(undo(&state.clone(), |transaction| {
        assert!(transaction.steps()[0].as_custom::<HistoryEdit>().is_some());
        state = state.apply(&transaction);
    }));
    assert_eq!(state.doc, doc!(p!("!ab")).node);
    assert_eq!(redo_depth(&state), 1);
    assert!(redo(&state.clone(), |transaction| {
        assert!(transaction.steps()[0].as_custom::<HistoryEdit>().is_some());
        state = state.apply(&transaction);
    }));
    assert_eq!(state.doc, doc!(p!("!aXb")).node);
    assert_eq!(DECODES.load(Ordering::Relaxed), 2);
}
