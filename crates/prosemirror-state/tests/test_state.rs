// Ported from prosemirror-state 1.4.3, test/test-state.ts.
use std::sync::Arc;

use prosemirror_model::Node;
use prosemirror_state::{EditorState, EditorStateConfig, Plugin, PluginKey, Selection, StateField, Transaction};
use prosemirror_test_builder::{doc, p, test_schema};
use serde_json::{json, Value};

struct MessageCount;

impl StateField for MessageCount {
    fn init(&self, _: &Value) -> Value {
        json!(0)
    }

    fn apply(&self, _: &Transaction, count: &Value) -> Value {
        json!(count.as_u64().unwrap() + 1)
    }
}

fn message_count_plugin() -> Plugin {
    Plugin::new(PluginKey::new("messageCount"), Some(Arc::new(MessageCount)))
        .with_prop("testProp", |plugin| json!(plugin.key.name))
}

fn transaction_plugin() -> Plugin {
    Plugin::unkeyed(None)
        .with_filter_transaction(|tr, _| tr.get_meta("filtered").and_then(Value::as_bool) != Some(true))
        .with_append_transaction(|transactions, _, state| {
            let append = transactions.last()?.get_meta("append")?.as_bool()?;
            if !append {
                return None;
            }
            let mut tr = state.tr();
            tr.insert_text_at_selection("A");
            Some(tr)
        })
}

fn create(doc: Option<Node>, plugins: Vec<Plugin>) -> EditorState {
    EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc,
        selection: None,
        stored_marks: None,
        plugins,
    })
}

#[test]
fn creates_default_doc() {
    assert_eq!(create(None, vec![]).doc, doc!(p!()).node);
}

#[test]
fn creates_default_selection() {
    let state = create(Some(doc!(p!("foo")).node), vec![]);
    assert_eq!(state.selection.from(), 1);
    assert_eq!(state.selection.to(&state.doc), 1);
}

#[test]
fn applies_transform_transactions() {
    let state = create(None, vec![]);
    let mut tr = state.tr();
    let new_state = state.apply(tr.insert_text_at_selection("hi"));
    assert_eq!(state.doc, doc!(p!()).node);
    assert_eq!(new_state.doc, doc!(p!("hi")).node);
    assert_eq!(new_state.selection.from(), 3);
}

#[test]
fn supports_plugin_fields() {
    let plugin = message_count_plugin();
    let state = create(None, vec![plugin.clone()]);
    let new_state = state.apply(&state.tr()).apply(&state.tr());
    assert_eq!(plugin.get_state(&state), Some(&json!(0)));
    assert_eq!(plugin.get_state(&new_state), Some(&json!(2)));
}

#[test]
fn can_be_serialized_to_json() {
    let plugin = message_count_plugin();
    let mut state = create(Some(doc!(p!("ok")).node), vec![plugin.clone()]);
    let mut tr = state.tr();
    state = state.apply(tr.set_selection(Selection::cursor(3)));
    let fields = [("count", &plugin)];
    let json = state.to_json_with_plugins(&fields).unwrap();
    assert_eq!(
        json,
        json!({
            "doc": {"type": "doc", "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "ok"}]}
            ]},
            "selection": {"type": "text", "anchor": 3, "head": 3},
            "count": 1
        })
    );
    let copy = EditorState::from_json_with_plugins(
        test_schema(), &json, vec![plugin.clone()], &fields
    ).unwrap();
    assert_eq!(copy.doc, state.doc);
    assert_eq!(copy.selection.from(), 3);
    assert_eq!(plugin.get_state(&copy), Some(&json!(1)));

    let limited = state.to_json();
    assert!(limited.get("doc").is_some());
    assert!(limited.get("messageCount$").is_none());
    let copy = EditorState::from_json_with_plugins(
        test_schema(), &limited, vec![plugin.clone()], &[]
    ).unwrap();
    assert_eq!(plugin.get_state(&copy), Some(&json!(0)));
}

#[test]
fn supports_specifying_and_persisting_stored_marks() {
    let state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc!(p!("ok")).node),
        selection: None,
        stored_marks: Some(vec![test_schema().mark_type("em").unwrap().create(None)]),
        plugins: vec![],
    });
    assert_eq!(state.stored_marks.as_ref().unwrap().len(), 1);
    let copy = EditorState::from_json(test_schema(), &state.to_json()).unwrap();
    assert_eq!(copy.stored_marks.unwrap().len(), 1);
}

#[test]
fn supports_reconfiguration() {
    let plugin = message_count_plugin();
    let state = create(None, vec![plugin.clone()]);
    assert_eq!(plugin.get_state(&state), Some(&json!(0)));
    let without = state.reconfigure(vec![]).unwrap();
    assert_eq!(plugin.get_state(&without), None);
    assert!(without.plugins.is_empty());
    assert_eq!(without.doc, doc!(p!()).node);
    let readded = without.reconfigure(vec![plugin.clone()]).unwrap();
    assert_eq!(plugin.get_state(&readded), Some(&json!(0)));
    assert_eq!(readded.plugins.len(), 1);
}

#[test]
fn allows_plugins_to_filter_transactions() {
    let state = create(None, vec![transaction_plugin()]);
    let mut tr = state.tr();
    let applied = state.apply_transaction(tr.insert_text_at_selection("X"));
    assert_eq!(applied.state.doc, doc!(p!("X")).node);
    assert_eq!(applied.transactions.len(), 1);
    let mut tr = state.tr();
    let applied = state.apply_transaction(
        tr.insert_text_at_selection("Y").set_meta("filtered", json!(true))
    );
    assert_eq!(applied.state.to_json(), state.to_json());
    assert!(applied.transactions.is_empty());
}

#[test]
fn allows_plugins_to_append_transactions() {
    let state = create(None, vec![transaction_plugin()]);
    let mut tr = state.tr();
    let applied = state.apply_transaction(
        tr.insert_text_at_selection("X").set_meta("append", json!(true))
    );
    assert_eq!(applied.state.doc, doc!(p!("XA")).node);
    assert_eq!(applied.transactions.len(), 2);
}

#[test]
fn stores_reference_to_root_transaction_for_appended_transactions() {
    let plugin = Plugin::unkeyed(None).with_append_transaction(|_, _, state| {
        let mut tr = state.tr();
        tr.insert_text_at_selection("Y");
        Some(tr)
    });
    let state = create(None, vec![plugin]);
    let mut tr = state.tr();
    let applied = state.apply_transaction(tr.insert_text_at_selection("X"));
    assert_eq!(applied.transactions.len(), 2);
    assert!(std::ptr::eq(
        applied.transactions[1].appended_transaction().unwrap(),
        applied.transactions[0].as_ref(),
    ));
}

#[test]
fn supports_embedding_state_in_json() {
    let state = create(None, vec![]);
    let value = json!({"someKey": state});
    assert!(!serde_json::to_string(&value).unwrap().is_empty());
}

#[test]
fn calls_prop_functions_bound_to_plugin() {
    let plugin = message_count_plugin();
    assert_eq!(plugin.call_prop("testProp"), Some(json!(plugin.key.name)));
}

#[test]
fn finds_plugins_by_key() {
    let plugin = message_count_plugin();
    let state = create(None, vec![plugin.clone()]);
    assert_eq!(plugin.key.get(&state).unwrap().key, plugin.key);
    assert_eq!(plugin.key.get_state(&state), Some(&json!(0)));
}

#[test]
fn generates_new_keys() {
    assert_ne!(Plugin::unkeyed(None).key, Plugin::unkeyed(None).key);
    assert_ne!(PluginKey::new("foo"), PluginKey::new("foo"));
}

#[test]
fn multiple_steps_map_selection_from_last_explicit_position() {
    let state = create(Some(doc!(p!("abc")).node), vec![]);
    let mut tr = state.tr();
    tr.insert_text("x", 1, 1)
        .set_selection(Selection::cursor(3))
        .insert_text("y", 1, 1)
        .insert_text("z", 1, 1);
    assert_eq!(tr.selection(), Selection::cursor(5));
    assert_eq!(state.apply(&tr).selection, Selection::cursor(5));
}

#[test]
fn multiple_append_hooks_receive_only_unseen_transactions() {
    let first = Plugin::unkeyed(None).with_append_transaction(|transactions, old, new| {
        let last = transactions.last()?;
        if last.get_meta("second") == Some(&json!(true)) {
            assert_eq!(old.doc, doc!(p!("x")).node);
            assert_eq!(new.doc, doc!(p!("xy")).node);
            let mut tr = new.tr();
            tr.insert_text_at_selection("z");
            return Some(tr);
        }
        None
    });
    let second = Plugin::unkeyed(None).with_append_transaction(|transactions, _, new| {
        let last = transactions.last()?;
        if last.get_meta("root") == Some(&json!(true)) {
            let mut tr = new.tr();
            tr.insert_text_at_selection("y").set_meta("second", json!(true));
            return Some(tr);
        }
        None
    });
    let state = create(None, vec![first, second]);
    let mut tr = state.tr();
    let applied = state.apply_transaction(tr.insert_text_at_selection("x").set_meta("root", json!(true)));
    assert_eq!(applied.state.doc, doc!(p!("xyz")).node);
    assert_eq!(applied.transactions.len(), 3);
}
