use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use prosemirror_model::{Mark, ModelError, Node, Schema};
use serde::{Serialize, Serializer};
use serde_json::Value;

use crate::plugin::{ErasedValue, Plugin};
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
    typed_plugin_states: HashMap<String, ErasedValue>,
    pub scroll_to_selection: usize,
}

#[derive(Clone)]
pub struct EditorStateConfig {
    pub schema: Schema,
    pub doc: Option<Node>,
    pub selection: Option<Selection>,
    pub stored_marks: Option<Vec<Mark>>,
    pub plugins: Vec<Plugin>,
}

pub struct ApplyTransactionResult {
    pub state: EditorState,
    pub transactions: Vec<Arc<Transaction>>,
}

impl EditorState {
    pub fn create(config: EditorStateConfig) -> Self {
        Self::try_create(config).expect("valid editor state configuration")
    }

    pub fn try_create(config: EditorStateConfig) -> Result<Self, ModelError> {
        Self::initialize(config, None)
    }

    fn initialize(
        config: EditorStateConfig,
        plugin_json: Option<(&Value, &[(&str, &Plugin)])>,
    ) -> Result<Self, ModelError> {
        check_plugins(&config.plugins)?;
        let doc = match &config.doc {
            Some(doc) => doc.clone(),
            None => config
                .schema
                .top_node_type()
                .create_and_fill(None, None, None)?
                .ok_or_else(|| ModelError::Serialization("Cannot create default document".into()))?,
        };
        let schema = doc.schema().cloned().unwrap_or_else(|| config.schema.clone());
        let mut state = Self {
            selection: config.selection.clone().unwrap_or_else(|| Selection::at_start(&doc)),
            doc,
            stored_marks: config.stored_marks.clone(),
            schema,
            plugins: config.plugins.clone(),
            plugin_states: HashMap::new(),
            typed_plugin_states: HashMap::new(),
            scroll_to_selection: 0,
        };
        for plugin in &state.plugins {
            if let Some(field) = &plugin.field {
                let serialized = plugin_json.and_then(|(json, fields)| {
                    fields
                        .iter()
                        .find(|(_, candidate)| candidate.key == plugin.key)
                        .and_then(|(property, _)| json.get(*property))
                });
                let value = if let Some(value) = serialized {
                    field.deserialize_with_state(&config, value, &state)
                } else {
                    field.init_with_state(&config, &state)
                };
                state.plugin_states.insert(plugin.key.name.clone(), value);
            }
            if let Some(field) = &plugin.typed_field {
                let value = field.init(&config, &state);
                state.typed_plugin_states.insert(plugin.key.name.clone(), value);
            }
        }
        Ok(state)
    }

    pub fn tr(&self) -> Transaction {
        let mut tr = Transaction::new(self.doc.clone(), self.selection.clone());
        tr.stored_marks = self.stored_marks.clone();
        tr
    }

    pub fn handle_text_input(
        &self,
        from: usize,
        to: usize,
        text: &str,
        composing: bool,
    ) -> Option<Transaction> {
        self.plugins.iter().find_map(|plugin| {
            plugin.handle_text_input(self, from, to, text, composing)
        })
    }

    pub fn handle_composition_end(&self) -> Option<Transaction> {
        self.plugins.iter().find_map(|plugin| plugin.handle_composition_end(self))
    }

    pub fn apply(&self, tr: &Transaction) -> Self {
        self.apply_transaction(tr).state
    }

    pub fn apply_transaction(&self, tr: &Transaction) -> ApplyTransactionResult {
        if !self.filter_transaction(tr, None) {
            return ApplyTransactionResult {
                state: self.clone(),
                transactions: Vec::new(),
            };
        }

        let root = Arc::new(tr.clone());
        let mut transactions = vec![Arc::clone(&root)];
        let mut state = self.apply_inner(tr);
        if self.plugins.iter().all(|plugin| plugin.append_transaction.is_none()) {
            return ApplyTransactionResult { state, transactions };
        }
        let mut seen = vec![(self.clone(), 0); self.plugins.len()];
        loop {
            let mut have_new = false;
            for (index, plugin) in self.plugins.iter().enumerate() {
                let Some(append) = &plugin.append_transaction else {
                    continue;
                };
                let (old_state, count) = &seen[index];
                if *count < transactions.len() {
                    if let Some(mut appended) = append(&transactions[*count..], old_state, &state) {
                        if state.filter_transaction(&appended, Some(index)) {
                            appended.set_appended_transaction(Arc::clone(&root));
                            state = state.apply_inner(&appended);
                            transactions.push(Arc::new(appended));
                            have_new = true;
                        }
                    }
                }
                seen[index] = (state.clone(), transactions.len());
            }
            if !have_new {
                return ApplyTransactionResult { state, transactions };
            }
        }
    }

    pub fn filter_transaction(&self, tr: &Transaction, ignore: Option<usize>) -> bool {
        self.plugins.iter().enumerate().all(|(index, plugin)| {
            Some(index) == ignore
                || plugin
                    .filter_transaction
                    .as_ref()
                    .is_none_or(|filter| filter(tr, self))
        })
    }

    fn apply_inner(&self, tr: &Transaction) -> Self {
        assert_eq!(tr.before(), &self.doc, "Applying a mismatched transaction");
        let selection = tr.selection();
        let mut state = Self {
            doc: tr.doc().clone(),
            stored_marks: selection.cursor_pos().and_then(|_| tr.stored_marks.clone()),
            selection,
            schema: self.schema.clone(),
            plugins: self.plugins.clone(),
            plugin_states: HashMap::new(),
            typed_plugin_states: HashMap::new(),
            scroll_to_selection: self.scroll_to_selection + tr.scrolled_into_view() as usize,
        };
        for plugin in &self.plugins {
            if let Some(field) = &plugin.field {
                let prev = plugin.get_state(self).unwrap_or(&Value::Null);
                let next = field.apply_with_state(tr, prev, self, &state);
                state.plugin_states.insert(plugin.key.name.clone(), next);
            }
            if let Some(field) = &plugin.typed_field {
                let prev = self.typed_plugin_states.get(&plugin.key.name)
                    .expect("initialized plugin state");
                let next = field.apply(tr, prev, self, &state);
                state.typed_plugin_states.insert(plugin.key.name.clone(), next);
            }
        }
        state
    }

    pub fn reconfigure(&self, plugins: Vec<Plugin>) -> Result<Self, ModelError> {
        check_plugins(&plugins)?;
        let config = EditorStateConfig {
            schema: self.schema.clone(),
            doc: None,
            selection: None,
            stored_marks: None,
            plugins: plugins.clone(),
        };
        let mut state = Self {
            plugins,
            plugin_states: HashMap::new(),
            typed_plugin_states: HashMap::new(),
            ..self.clone()
        };
        for plugin in &state.plugins {
            if let Some(field) = &plugin.field {
                let value = plugin
                    .get_state(self)
                    .cloned()
                    .unwrap_or_else(|| field.init_with_state(&config, &state));
                state.plugin_states.insert(plugin.key.name.clone(), value);
            }
            if let Some(field) = &plugin.typed_field {
                let value = self.typed_plugin_states.get(&plugin.key.name)
                    .filter(|value| field.accepts(value))
                    .cloned()
                    .unwrap_or_else(|| field.init(&config, &state));
                state.typed_plugin_states.insert(plugin.key.name.clone(), value);
            }
        }
        Ok(state)
    }

    pub fn get_plugin_state(&self, name: &str) -> Option<&Value> {
        self.plugin_states.get(name).or_else(|| {
            self.plugins.iter().find_map(|plugin| {
                let (label, suffix) = plugin.key.name.rsplit_once('$')?;
                if label == name && suffix.bytes().all(|byte| byte.is_ascii_digit()) {
                    plugin.get_state(self)
                } else {
                    None
                }
            })
        })
    }

    pub fn get_typed_plugin_state<T: Any + Send + Sync>(
        &self,
        key: impl AsRef<str>,
    ) -> Option<&T> {
        self.typed_plugin_states.get(key.as_ref())?.downcast_ref()
    }

    pub fn to_json(&self) -> Value {
        let mut json = serde_json::json!({
            "doc": self.doc,
            "selection": self.selection,
        });
        if let Some(marks) = &self.stored_marks {
            json["storedMarks"] = serde_json::json!(marks);
        }
        json
    }

    pub fn to_json_with_plugins(&self, fields: &[(&str, &Plugin)]) -> Result<Value, ModelError> {
        let mut json = self.to_json();
        for &(name, plugin) in fields {
            if matches!(name, "doc" | "selection") {
                return Err(ModelError::Serialization(format!("Reserved editor state field {name:?}")));
            }
            if let (Some(field), Some(value)) = (&plugin.field, plugin.get_state(self)) {
                json[name] = field.to_json(value);
            }
        }
        Ok(json)
    }

    pub fn from_json(schema: &Schema, json: &Value) -> Result<Self, ModelError> {
        Self::from_json_with_plugins(schema, json, Vec::new(), &[])
    }

    pub fn from_json_with_plugins(
        schema: &Schema,
        json: &Value,
        plugins: Vec<Plugin>,
        fields: &[(&str, &Plugin)],
    ) -> Result<Self, ModelError> {
        let required = |name| {
            json.get(name)
                .ok_or_else(|| ModelError::Serialization(format!("Missing {name:?} in state JSON")))
        };
        let doc = schema.node_from_json(required("doc")?)?;
        let selection = Selection::from_json(&doc, required("selection")?)?;
        let stored_marks = json
            .get("storedMarks")
            .filter(|marks| !marks.is_null())
            .map(|marks| {
                let array = marks.as_array().ok_or_else(|| {
                    ModelError::Serialization("storedMarks must be an array".into())
                })?;
                array
                    .iter()
                    .map(|mark| schema.mark_from_json(mark))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        Self::initialize(
            EditorStateConfig {
                schema: schema.clone(),
                doc: Some(doc),
                selection: Some(selection),
                stored_marks,
                plugins,
            },
            Some((json, fields)),
        )
    }
}

impl Serialize for EditorState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}

fn check_plugins(plugins: &[Plugin]) -> Result<(), ModelError> {
    let mut keys = HashSet::new();
    for plugin in plugins {
        if !keys.insert(&plugin.key) {
            return Err(ModelError::Serialization(format!(
                "Adding different instances of a keyed plugin ({})",
                plugin.key.name
            )));
        }
    }
    Ok(())
}
