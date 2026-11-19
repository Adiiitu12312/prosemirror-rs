use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;

use crate::state::{EditorState, EditorStateConfig};
use crate::transaction::Transaction;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PluginKey {
    pub name: String,
}

impl PluginKey {
    pub fn new(name: impl Into<String>) -> Self {
        static KEYS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
        let name = name.into();
        let mut keys = KEYS.get_or_init(Mutex::default).lock().unwrap();
        let count = keys.entry(name.clone()).or_default();
        let key = if *count == 0 {
            format!("{name}$")
        } else {
            format!("{name}${count}")
        };
        *count += 1;
        Self { name: key }
    }

    pub fn get<'a>(&self, state: &'a EditorState) -> Option<&'a Plugin> {
        state.plugins.iter().find(|plugin| plugin.key == *self)
    }

    pub fn get_state<'a>(&self, state: &'a EditorState) -> Option<&'a Value> {
        state.plugin_states.get(&self.name)
    }
}

impl From<&PluginKey> for String {
    fn from(key: &PluginKey) -> Self {
        key.name.clone()
    }
}

impl AsRef<str> for PluginKey {
    fn as_ref(&self) -> &str {
        &self.name
    }
}

pub trait StateField: Send + Sync {
    fn init(&self, config: &Value) -> Value;
    fn apply(&self, tr: &Transaction, value: &Value) -> Value;

    fn init_with_state(&self, _config: &EditorStateConfig, _state: &EditorState) -> Value {
        self.init(&Value::Null)
    }

    fn apply_with_state(
        &self,
        tr: &Transaction,
        value: &Value,
        _old_state: &EditorState,
        _new_state: &EditorState,
    ) -> Value {
        self.apply(tr, value)
    }

    fn to_json(&self, value: &Value) -> Value {
        value.clone()
    }
    fn from_json(&self, value: &Value) -> Value {
        value.clone()
    }

    fn deserialize_with_state(
        &self,
        _config: &EditorStateConfig,
        value: &Value,
        _state: &EditorState,
    ) -> Value {
        self.from_json(value)
    }
}

type FilterTransaction = dyn Fn(&Transaction, &EditorState) -> bool + Send + Sync;
type AppendTransaction =
    dyn Fn(&[Arc<Transaction>], &EditorState, &EditorState) -> Option<Transaction> + Send + Sync;
type Prop = dyn Fn(&Plugin) -> Value + Send + Sync;
type TextInput = dyn Fn(&EditorState, usize, usize, &str, bool) -> Option<Transaction> + Send + Sync;
type CompositionEnd = dyn Fn(&EditorState) -> Option<Transaction> + Send + Sync;

#[derive(Clone)]
pub struct Plugin {
    pub key: PluginKey,
    pub field: Option<Arc<dyn StateField>>,
    pub spec: HashMap<String, Value>,
    pub(crate) filter_transaction: Option<Arc<FilterTransaction>>,
    pub(crate) append_transaction: Option<Arc<AppendTransaction>>,
    text_input: Option<Arc<TextInput>>,
    composition_end: Option<Arc<CompositionEnd>>,
    props: HashMap<String, Arc<Prop>>,
}

impl Plugin {
    pub fn new(key: PluginKey, field: Option<Arc<dyn StateField>>) -> Self {
        Self {
            key,
            field,
            spec: HashMap::new(),
            filter_transaction: None,
            append_transaction: None,
            text_input: None,
            composition_end: None,
            props: HashMap::new(),
        }
    }

    pub fn unkeyed(field: Option<Arc<dyn StateField>>) -> Self {
        Self::new(PluginKey::new("plugin"), field)
    }

    pub fn with_filter_transaction(
        mut self,
        filter: impl Fn(&Transaction, &EditorState) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.filter_transaction = Some(Arc::new(filter));
        self
    }

    pub fn with_append_transaction(
        mut self,
        append: impl Fn(&[Arc<Transaction>], &EditorState, &EditorState) -> Option<Transaction>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        self.append_transaction = Some(Arc::new(append));
        self
    }

    pub fn with_prop(
        mut self,
        name: impl Into<String>,
        prop: impl Fn(&Plugin) -> Value + Send + Sync + 'static,
    ) -> Self {
        self.props.insert(name.into(), Arc::new(prop));
        self
    }

    pub fn with_text_input(
        mut self,
        handle: impl Fn(&EditorState, usize, usize, &str, bool) -> Option<Transaction>
            + Send + Sync + 'static,
    ) -> Self {
        self.text_input = Some(Arc::new(handle));
        self
    }

    pub fn with_composition_end(
        mut self,
        handle: impl Fn(&EditorState) -> Option<Transaction> + Send + Sync + 'static,
    ) -> Self {
        self.composition_end = Some(Arc::new(handle));
        self
    }

    pub fn handle_text_input(
        &self,
        state: &EditorState,
        from: usize,
        to: usize,
        text: &str,
        composing: bool,
    ) -> Option<Transaction> {
        self.text_input.as_ref()?.as_ref()(state, from, to, text, composing)
    }

    pub fn handle_composition_end(&self, state: &EditorState) -> Option<Transaction> {
        self.composition_end.as_ref()?.as_ref()(state)
    }

    pub fn call_prop(&self, name: &str) -> Option<Value> {
        self.props.get(name).map(|prop| prop(self))
    }

    pub fn get_state<'a>(&self, state: &'a EditorState) -> Option<&'a Value> {
        self.key.get_state(state)
    }
}

impl From<&Plugin> for String {
    fn from(plugin: &Plugin) -> Self {
        (&plugin.key).into()
    }
}

impl AsRef<str> for Plugin {
    fn as_ref(&self) -> &str {
        self.key.as_ref()
    }
}
