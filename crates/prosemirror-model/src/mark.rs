use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Mark {
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attrs: Option<Value>,
}

impl Mark {
    pub fn new(type_name: impl Into<String>, attrs: Option<Value>) -> Self {
        Self {
            type_name: type_name.into(),
            attrs,
        }
    }

    pub fn is_in_set(&self, set: &[Mark]) -> bool {
        set.iter().any(|m| m == self)
    }

    pub fn add_to_set(set: &[Mark], mark: Mark) -> Vec<Mark> {
        let mut result = Vec::with_capacity(set.len() + 1);
        let mut added = false;
        for m in set {
            if m.type_name == mark.type_name {
                if m == &mark {
                    return set.to_vec();
                }
                result.push(mark.clone());
                added = true;
            } else {
                result.push(m.clone());
            }
        }
        if !added {
            result.push(mark);
            result.sort();
        }
        result
    }

    pub fn remove_from_set(set: &[Mark], type_name: &str) -> Vec<Mark> {
        set.iter().filter(|m| m.type_name != type_name).cloned().collect()
    }

    pub fn same_set(a: &[Mark], b: &[Mark]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        a.iter().zip(b.iter()).all(|(x, y)| x == y)
    }
}
