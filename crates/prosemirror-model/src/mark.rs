use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::MarkType;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mark {
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default, skip_serializing_if = "crate::node::empty_attrs")]
    pub attrs: Option<Value>,
    #[serde(skip)]
    rules: Option<Arc<MarkRules>>,
}

#[derive(Debug)]
struct MarkRules {
    rank: usize,
    inclusive: bool,
    spanning: bool,
    excluded: HashSet<String>,
}

impl PartialEq for Mark {
    fn eq(&self, other: &Self) -> bool {
        self.type_name == other.type_name && crate::node::same_attrs(self.attrs.as_ref(), other.attrs.as_ref())
    }
}

impl Mark {
    pub fn new(type_name: impl Into<String>, attrs: Option<Value>) -> Self {
        Self {
            type_name: type_name.into(),
            attrs,
            rules: None,
        }
    }

    pub(crate) fn from_type(mark_type: &MarkType, attrs: Option<Value>) -> Self {
        Self {
            type_name: mark_type.name.clone(),
            attrs,
            rules: Some(Arc::new(MarkRules {
                rank: mark_type.rank,
                inclusive: mark_type.spec.inclusive,
                spanning: mark_type.spec.spanning,
                excluded: mark_type.excluded.clone(),
            })),
        }
    }

    pub fn is_inclusive(&self) -> bool {
        self.rules.as_ref().map_or(self.type_name != "link", |rules| rules.inclusive)
    }

    pub fn is_spanning(&self) -> bool {
        self.rules.as_ref().is_none_or(|rules| rules.spanning)
    }

    fn excludes(&self, other: &Self) -> bool {
        self.rules.as_ref().map_or(self.type_name == other.type_name, |rules| {
            rules.excluded.contains(&other.type_name)
        })
    }

    fn compare_rank(&self, other: &Self) -> Ordering {
        fn rank(mark: &Mark) -> usize {
            mark.rules.as_ref().map_or_else(|| {
                match mark.type_name.as_str() {
                    "link" => 0,
                    "em" => 1,
                    "strong" => 2,
                    "code" => 3,
                    _ => usize::MAX,
                }
            }, |rules| rules.rank)
        }
        rank(self).cmp(&rank(other))
    }

    pub fn is_in_set(&self, set: &[Mark]) -> bool {
        set.iter().any(|m| m == self)
    }

    pub fn add_to_set(set: &[Mark], mark: Mark) -> Vec<Mark> {
        let mut result = Vec::with_capacity(set.len() + 1);
        let mut placed = false;
        for other in set {
            if other == &mark {
                return set.to_vec();
            }
            if mark.excludes(other) {
                continue;
            }
            if other.excludes(&mark) {
                return set.to_vec();
            }
            if !placed && mark.compare_rank(other).is_lt() {
                result.push(mark.clone());
                placed = true;
            }
            result.push(other.clone());
        }
        if !placed {
            result.push(mark);
        }
        result
    }

    pub fn remove_from_set(set: &[Mark], type_name: &str) -> Vec<Mark> {
        set.iter().filter(|m| m.type_name != type_name).cloned().collect()
    }

    pub fn remove_exact_from_set(&self, set: &[Mark]) -> Vec<Mark> {
        let mut result = set.to_vec();
        if let Some(index) = set.iter().position(|mark| mark == self) {
            result.remove(index);
        }
        result
    }

    pub fn set_from(mut marks: Vec<Mark>) -> Vec<Mark> {
        marks.sort_by(Mark::compare_rank);
        marks
    }

    pub fn same_set(a: &[Mark], b: &[Mark]) -> bool {
        a == b
    }
}
