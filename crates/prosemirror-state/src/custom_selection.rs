use std::any::Any;
use std::collections::{hash_map::Entry, HashMap};
use std::fmt::Debug;
use std::sync::{Arc, OnceLock, RwLock};

use prosemirror_model::{ModelError, Node, ResolvedPos, Slice};
use prosemirror_transform::Mappable;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::transaction::insertion_bias;
use crate::{Selection, SelectionBookmark, SelectionRange, Transaction};

/// An editor selection with its own mapping and JSON representation.
pub trait CustomSelection: Any + Debug + Send + Sync {
    fn anchor(&self) -> &ResolvedPos;
    fn head(&self) -> &ResolvedPos;
    fn map(&self, doc: &Node, mapping: &dyn Mappable) -> Selection;
    fn json_fields(&self) -> Map<String, Value>;

    fn eq(&self, other: &dyn CustomSelection) -> bool {
        self.json_fields() == other.json_fields()
    }

    /// The main range comes first. The default range spans the anchor and head.
    fn ranges(&self) -> Vec<SelectionRange> {
        let anchor = self.anchor();
        let head = self.head();
        let (from, to) = if anchor.pos <= head.pos { (anchor, head) } else { (head, anchor) };
        vec![SelectionRange { from: from.clone(), to: to.clone() }]
    }

    fn visible(&self) -> bool { true }

    fn content(&self) -> Result<Slice, ModelError> {
        let ranges = self.ranges();
        let range = ranges.first().ok_or_else(|| ModelError::Serialization("Missing selection range".into()))?;
        range.from.doc().slice(range.from.pos, range.to.pos, true)
    }

    fn replace(&self, tr: &mut Transaction, slice: Slice) {
        tr.replace_selection_ranges(self.ranges(), insertion_bias(&slice), |tr, from, to| {
            tr.replace_range(from, to, slice);
        });
    }

    fn replace_with(&self, tr: &mut Transaction, node: Node) {
        let bias = if node.is_inline() { -1 } else { 1 };
        tr.replace_selection_ranges(self.ranges(), bias, |tr, from, to| {
            tr.replace_range_with(from, to, node);
        });
    }

    /// The default bookmark restores a nearby text selection.
    fn bookmark(&self) -> SelectionBookmark {
        Selection::between(self.anchor(), self.head(), 0).bookmark()
    }
}

/// A document-independent bookmark that can map and restore a custom selection.
pub trait CustomSelectionBookmark: Any + Debug + Send + Sync {
    fn map(&self, mapping: &dyn Mappable) -> SelectionBookmark;
    fn resolve(&self, doc: &Node) -> Selection;
    fn json_fields(&self) -> Map<String, Value>;
}

pub type SelectionDecoder = fn(&Node, &Value) -> Result<Arc<dyn CustomSelection>, ModelError>;
pub type BookmarkDecoder = fn(&Value) -> Result<Arc<dyn CustomSelectionBookmark>, ModelError>;

fn selection_decoders() -> &'static RwLock<HashMap<String, SelectionDecoder>> {
    static DECODERS: OnceLock<RwLock<HashMap<String, SelectionDecoder>>> = OnceLock::new();
    DECODERS.get_or_init(RwLock::default)
}

fn bookmark_decoders() -> &'static RwLock<HashMap<String, BookmarkDecoder>> {
    static DECODERS: OnceLock<RwLock<HashMap<String, BookmarkDecoder>>> = OnceLock::new();
    DECODERS.get_or_init(RwLock::default)
}

fn register<T>(decoders: &RwLock<HashMap<String, T>>, id: String, decode: T, kind: &str) -> Result<(), ModelError> {
    if id.is_empty() {
        return Err(ModelError::Serialization(format!("Empty {kind} JSON ID")));
    }
    match decoders.write().unwrap().entry(id) {
        Entry::Vacant(entry) => {
            entry.insert(decode);
            Ok(())
        }
        Entry::Occupied(entry) => Err(ModelError::Serialization(format!("Duplicate use of {kind} JSON ID {}", entry.key()))),
    }
}

pub(crate) fn is_builtin_id(id: &str) -> bool {
    matches!(id, "text" | "node" | "all" | "gapcursor" | "cell")
}

pub(crate) fn register_selection(id: String, decode: SelectionDecoder) -> Result<(), ModelError> {
    if is_builtin_id(&id) {
        return Err(ModelError::Serialization(format!("Duplicate use of selection JSON ID {id}")));
    }
    register(selection_decoders(), id, decode, "selection")
}

pub(crate) fn register_bookmark(id: String, decode: BookmarkDecoder) -> Result<(), ModelError> {
    register(bookmark_decoders(), id, decode, "bookmark")
}

/// Holds a runtime selection and the ID used to register its JSON decoder.
#[derive(Clone, Debug)]
pub struct RegisteredSelection {
    id: String,
    pub(crate) implementation: Arc<dyn CustomSelection>,
    pub(crate) ranges: Vec<SelectionRange>,
}

impl RegisteredSelection {
    pub(crate) fn from_json(doc: &Node, id: &str, json: &Value) -> Result<Self, ModelError> {
        let decode = selection_decoders().read().unwrap().get(id).copied()
            .ok_or_else(|| ModelError::Serialization(format!("No selection type {id} defined")))?;
        let selection = Self::new(id.to_owned(), decode(doc, json)?)?;
        if !selection.doc().ptr_eq(doc) {
            return Err(ModelError::Serialization("Selection must point at the current document".into()));
        }
        Ok(selection)
    }

    pub(crate) fn new(id: String, implementation: Arc<dyn CustomSelection>) -> Result<Self, ModelError> {
        if id.is_empty() || is_builtin_id(&id) {
            return Err(ModelError::Serialization(format!("Invalid custom selection JSON ID {id}")));
        }
        let doc = implementation.anchor().doc();
        let ranges = implementation.ranges();
        if ranges.is_empty() || !implementation.head().doc().ptr_eq(doc) || ranges.iter().any(|range| {
            range.from.pos > range.to.pos || !range.from.doc().ptr_eq(doc) || !range.to.doc().ptr_eq(doc)
        }) {
            return Err(ModelError::Serialization("Invalid custom selection ranges".into()));
        }
        Ok(Self { id, implementation, ranges })
    }

    pub(crate) fn doc(&self) -> &Node {
        self.implementation.anchor().doc()
    }
}

impl PartialEq for RegisteredSelection {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.implementation.eq(other.implementation.as_ref())
    }
}

impl Eq for RegisteredSelection {}

impl Serialize for RegisteredSelection {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields = self.implementation.json_fields();
        fields.insert("type".into(), self.id.clone().into());
        fields.serialize(serializer)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RegisteredBookmark {
    id: String,
    pub implementation: Arc<dyn CustomSelectionBookmark>,
}

impl RegisteredBookmark {
    pub fn new(id: String, implementation: Arc<dyn CustomSelectionBookmark>) -> Self {
        Self { id, implementation }
    }
}

impl PartialEq for RegisteredBookmark {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.implementation.json_fields() == other.implementation.json_fields()
    }
}

impl Eq for RegisteredBookmark {}

#[derive(Serialize, Deserialize)]
struct BookmarkJson {
    #[serde(rename = "bookmarkType")]
    id: String,
    value: Map<String, Value>,
}

impl Serialize for RegisteredBookmark {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        BookmarkJson { id: self.id.clone(), value: self.implementation.json_fields() }.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RegisteredBookmark {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let BookmarkJson { id, value } = BookmarkJson::deserialize(deserializer)?;
        let decode = bookmark_decoders().read().unwrap().get(&id).copied()
            .ok_or_else(|| serde::de::Error::custom(format!("No bookmark type {id} defined")))?;
        let implementation = decode(&Value::Object(value)).map_err(serde::de::Error::custom)?;
        Ok(Self::new(id, implementation))
    }
}
