use std::sync::Arc;
use serde_json::Value;
use crate::transaction::Transaction;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PluginKey {
    pub name: String,
}

impl PluginKey {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

pub trait StateField: Send + Sync {
    fn init(&self, config: &Value) -> Value;
    fn apply(&self, tr: &Transaction, value: &Value) -> Value;
    fn to_json(&self, value: &Value) -> Value {
        value.clone()
    }
    fn from_json(&self, value: &Value) -> Value {
        value.clone()
    }
}

#[derive(Clone)]
pub struct Plugin {
    pub key: PluginKey,
    pub field: Option<Arc<dyn StateField>>,
}

impl Plugin {
    pub fn new(key: PluginKey, field: Option<Arc<dyn StateField>>) -> Self {
        Self { key, field }
    }
}
