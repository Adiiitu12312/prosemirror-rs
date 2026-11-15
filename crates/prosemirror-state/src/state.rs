use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use prosemirror_model::{Mark, Node, Schema};
use crate::plugin::Plugin;
use crate::selection::Selection;
use crate::transaction::Transaction;

#[derive(Clone)]
pub struct EditorState {
    pub doc: Node,
    pub selection: Selection,
    pub stored_marks: Option<Vec<Mark>>,
    pub schema: Schema,
    pub plugins: Vec<Plugin>,
    pub plugin_states: HashMap<String, Value>,
}

pub struct EditorStateConfig {
    pub schema: Schema,
    pub doc: Option<Node>,
    pub selection: Option<Selection>,
    pub stored_marks: Option<Vec<Mark>>,
    pub plugins: Vec<Plugin>,
}

impl EditorState {
    pub fn create(config: EditorStateConfig) -> Self {
        let doc = config.doc.unwrap_or_else(|| {
            config.schema.node("doc", None, None, None).unwrap()
        });
        let selection = config.selection.unwrap_or_else(|| Selection::cursor(0));
        let mut plugin_states = HashMap::new();
        for p in &config.plugins {
            if let Some(ref field) = p.field {
                let init_val = field.init(&Value::Null);
                plugin_states.insert(p.key.name.clone(), init_val);
            }
        }

        Self {
            doc,
            selection,
            stored_marks: config.stored_marks,
            schema: config.schema,
            plugins: config.plugins,
            plugin_states,
        }
    }

    pub fn tr(&self) -> Transaction {
        let mut tr = Transaction::new(self.doc.clone(), self.selection.clone());
        tr.stored_marks = self.stored_marks.clone();
        tr
    }

    pub fn apply(&self, tr: &Transaction) -> Self {
        let mut new_plugin_states = HashMap::new();
        for p in &self.plugins {
            if let Some(ref field) = p.field {
                let prev_val = self.plugin_states.get(&p.key.name).unwrap_or(&Value::Null);
                let next_val = field.apply(tr, prev_val);
                new_plugin_states.insert(p.key.name.clone(), next_val);
            }
        }

        Self {
            doc: tr.doc().clone(),
            selection: tr.selection.clone(),
            stored_marks: tr.stored_marks.clone(),
            schema: self.schema.clone(),
            plugins: self.plugins.clone(),
            plugin_states: new_plugin_states,
        }
    }

    pub fn get_plugin_state(&self, plugin_name: &str) -> Option<&Value> {
        self.plugin_states.get(plugin_name)
    }

    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "doc": self.doc,
            "selection": self.selection,
        })
    }

    pub fn from_json(schema: &Schema, json: &Value) -> Result<Self, prosemirror_model::ModelError> {
        let doc_val = json.get("doc").ok_or_else(|| {
            prosemirror_model::ModelError::Serialization("Missing 'doc' in state JSON".to_string())
        })?;
        let doc: Node = serde_json::from_value(doc_val.clone())
            .map_err(|e| prosemirror_model::ModelError::Serialization(e.to_string()))?;

        let selection: Selection = if let Some(sel_val) = json.get("selection") {
            serde_json::from_value(sel_val.clone())
                .map_err(|e| prosemirror_model::ModelError::Serialization(e.to_string()))?
        } else {
            Selection::cursor(0)
        };

        Ok(Self::create(EditorStateConfig {
            schema: schema.clone(),
            doc: Some(doc),
            selection: Some(selection),
            stored_marks: None,
            plugins: Vec::new(),
        }))
    }
}
