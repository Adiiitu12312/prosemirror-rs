use std::any::Any;
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
        state.get_exact_plugin_state(&self.name)
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

    #[allow(clippy::wrong_self_convention)]
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

/// A plugin field that holds Rust values such as callbacks or persistent view data.
pub trait TypedStateField: Send + Sync {
    type Value: Any + Send + Sync;

    fn init(&self, config: &EditorStateConfig, state: &EditorState) -> Self::Value;

    fn apply(
        &self,
        tr: &Transaction,
        value: &Self::Value,
        old_state: &EditorState,
        new_state: &EditorState,
    ) -> Self::Value;

    /// Applies the transaction using the same shared object returned by the editor state.
    fn apply_shared(
        &self,
        tr: &Arc<Transaction>,
        value: &Self::Value,
        old_state: &EditorState,
        new_state: &EditorState,
    ) -> Self::Value {
        self.apply(tr, value, old_state, new_state)
    }
}

pub(crate) type ErasedValue = Arc<dyn Any + Send + Sync>;

type SerializeField<T> = fn(&T) -> Value;
type DeserializeField<T> = fn(&Value, &EditorState) -> Option<T>;

struct TypedField<F: TypedStateField> {
    field: F,
    serialize: Option<SerializeField<F::Value>>,
    deserialize: Option<DeserializeField<F::Value>>,
}

pub(crate) trait ErasedStateField: Send + Sync {
    fn init(&self, config: &EditorStateConfig, state: &EditorState) -> ErasedValue;
    fn accepts(&self, value: &ErasedValue) -> bool;
    fn has_json(&self) -> bool;
    fn serialize(&self, value: &ErasedValue) -> Option<Value>;
    fn deserialize(&self, json: &Value, state: &EditorState) -> Option<ErasedValue>;
    fn apply(
        &self,
        tr: &Arc<Transaction>,
        value: &ErasedValue,
        old_state: &EditorState,
        new_state: &EditorState,
    ) -> ErasedValue;
}

impl<F: TypedStateField> ErasedStateField for TypedField<F> {
    fn init(&self, config: &EditorStateConfig, state: &EditorState) -> ErasedValue {
        Arc::new(self.field.init(config, state))
    }

    fn accepts(&self, value: &ErasedValue) -> bool {
        value.is::<F::Value>()
    }

    fn has_json(&self) -> bool {
        self.serialize.is_some()
    }

    fn serialize(&self, value: &ErasedValue) -> Option<Value> {
        let serialize = self.serialize?;
        Some(serialize(value.downcast_ref::<F::Value>().expect("matching plugin field type")))
    }

    fn deserialize(&self, json: &Value, state: &EditorState) -> Option<ErasedValue> {
        let deserialize = self.deserialize?;
        Some(Arc::new(deserialize(json, state)?))
    }

    fn apply(
        &self,
        tr: &Arc<Transaction>,
        value: &ErasedValue,
        old_state: &EditorState,
        new_state: &EditorState,
    ) -> ErasedValue {
        let typed = value.downcast_ref::<F::Value>().expect("matching plugin field type");
        Arc::new(self.field.apply_shared(tr, typed, old_state, new_state))
    }
}

pub(crate) struct StoredTypedState {
    pub(crate) value: ErasedValue,
    field: Arc<dyn ErasedStateField>,
    json: OnceLock<Option<Value>>,
}

impl StoredTypedState {
    pub(crate) fn new(
        value: ErasedValue,
        field: &Arc<dyn ErasedStateField>,
        source_json: Option<&Value>,
    ) -> Arc<Self> {
        Arc::new(Self {
            value,
            field: Arc::clone(field),
            json: source_json.map_or_else(OnceLock::new, |json| OnceLock::from(Some(json.clone()))),
        })
    }

    pub(crate) fn for_field(self: &Arc<Self>, field: &Arc<dyn ErasedStateField>) -> Arc<Self> {
        if Arc::ptr_eq(&self.field, field) {
            Arc::clone(self)
        } else {
            Self::new(Arc::clone(&self.value), field, None)
        }
    }

    pub(crate) fn json(&self) -> Option<&Value> {
        self.json.get_or_init(|| self.field.serialize(&self.value)).as_ref()
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
    pub(crate) typed_field: Option<Arc<dyn ErasedStateField>>,
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
            typed_field: None,
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

    pub fn with_typed_state_field(mut self, field: impl TypedStateField + 'static) -> Self {
        self.typed_field = Some(Arc::new(TypedField { field, serialize: None, deserialize: None }));
        self
    }

    /// Set a typed state field with a JSON representation computed when requested.
    ///
    /// This replaces the JSON-only field. The decoder receives the partially
    /// initialized editor state, including its document and schema.
    pub fn with_serializable_typed_state_field<F: TypedStateField + 'static>(
        mut self,
        field: F,
        serialize: fn(&F::Value) -> Value,
        deserialize: fn(&Value, &EditorState) -> Option<F::Value>,
    ) -> Self {
        self.field = None;
        self.typed_field = Some(Arc::new(TypedField {
            field,
            serialize: Some(serialize),
            deserialize: Some(deserialize),
        }));
        self
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
