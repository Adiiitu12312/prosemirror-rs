//! Persistent insertion-ordered maps from orderedmap 2.1.1.

use std::collections::HashMap;
use std::sync::Arc;

type Entry<T> = (String, Arc<T>);

/// A small persistent map whose iteration order follows ProseMirror's schema
/// ordering rules. Updates produce a new map and share its unchanged values.
#[derive(Debug, PartialEq, Eq)]
pub struct OrderedMap<T> {
    entries: Arc<Vec<Entry<T>>>,
}

impl<T> Default for OrderedMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for OrderedMap<T> {
    fn clone(&self) -> Self {
        Self { entries: self.entries.clone() }
    }
}

impl<T, K: Into<String>> From<Vec<(K, T)>> for OrderedMap<T> {
    fn from(entries: Vec<(K, T)>) -> Self {
        entries.into_iter().collect()
    }
}

impl<T, K: Into<String>, const N: usize> From<[(K, T); N]> for OrderedMap<T> {
    fn from(entries: [(K, T); N]) -> Self {
        entries.into_iter().collect()
    }
}

impl<T, K: Into<String>> FromIterator<(K, T)> for OrderedMap<T> {
    fn from_iter<I: IntoIterator<Item = (K, T)>>(items: I) -> Self {
        let mut entries: Vec<Entry<T>> = Vec::new();
        for (key, value) in items {
            let key = key.into();
            if let Some((_, current)) = entries.iter_mut().find(|(existing, _)| *existing == key) {
                *current = Arc::new(value);
            } else {
                entries.push((key, Arc::new(value)));
            }
        }
        Self::from_entries(entries)
    }
}

impl<T> OrderedMap<T> {
    pub fn new() -> Self {
        Self::from_entries(Vec::new())
    }

    pub fn get(&self, key: &str) -> Option<&T> {
        self.find(key).map(|index| self.entries[index].1.as_ref())
    }

    /// Replaces a value at its current position, or appends a new binding.
    /// A non-empty replacement key takes precedence over an existing binding
    /// with the same name.
    pub fn update(&self, key: impl Into<String>, value: T, new_key: Option<&str>) -> Self {
        let key = key.into();
        let new_key = new_key.filter(|key| !key.is_empty()).unwrap_or(&key);
        let mut entries = self.entries.as_ref().clone();
        if key != new_key {
            entries.retain(|(name, _)| name.as_str() != new_key);
        }
        let replacement = (new_key.to_owned(), Arc::new(value));
        if let Some(entry) = entries.iter_mut().find(|(name, _)| *name == key) {
            *entry = replacement;
        } else {
            entries.push(replacement);
        }
        Self::from_entries(entries)
    }

    pub fn remove(&self, key: &str) -> Self {
        let Some(index) = self.find(key) else { return self.clone() };
        let mut entries = self.entries.as_ref().clone();
        entries.remove(index);
        Self::from_entries(entries)
    }

    pub fn add_to_start(&self, key: impl Into<String>, value: T) -> Self {
        let key = key.into();
        let mut entries = Vec::with_capacity(self.size() + 1);
        entries.push((key.clone(), Arc::new(value)));
        entries.extend(self.entries.iter().filter(|(name, _)| *name != key).cloned());
        Self::from_entries(entries)
    }

    pub fn add_to_end(&self, key: impl Into<String>, value: T) -> Self {
        let key = key.into();
        let mut entries = self.without(&key);
        entries.push((key, Arc::new(value)));
        Self::from_entries(entries)
    }

    /// Inserts a key immediately before `place`, or appends it when the
    /// reference key is absent. An earlier binding for `key` is removed.
    pub fn add_before(&self, place: &str, key: impl Into<String>, value: T) -> Self {
        let key = key.into();
        let mut entries = self.without(&key);
        let index = entries.iter().position(|(name, _)| name.as_str() == place).unwrap_or(entries.len());
        entries.insert(index, (key, Arc::new(value)));
        Self::from_entries(entries)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &T)> + DoubleEndedIterator {
        self.entries.iter().map(|(name, value)| (name.as_str(), value.as_ref()))
    }

    pub fn for_each(&self, mut visit: impl FnMut(&str, &T)) {
        for (key, value) in self.iter() {
            visit(key, value);
        }
    }

    /// Places `other` first. Its values override any shared keys.
    pub fn prepend(&self, other: &Self) -> Self {
        if other.is_empty() {
            return self.clone();
        }
        let mut entries = other.entries.as_ref().clone();
        entries.extend(self.subtract(other).entries.iter().cloned());
        Self::from_entries(entries)
    }

    /// Places `other` last. Its values override any shared keys.
    pub fn append(&self, other: &Self) -> Self {
        if other.is_empty() {
            return self.clone();
        }
        let mut entries = self.subtract(other).entries.as_ref().clone();
        entries.extend(other.entries.iter().cloned());
        Self::from_entries(entries)
    }

    pub fn subtract<U>(&self, other: &OrderedMap<U>) -> Self {
        if !other.iter().any(|(name, _)| self.find(name).is_some()) {
            return self.clone();
        }
        Self::from_entries(self.entries.iter().filter(|(name, _)| other.find(name).is_none()).cloned().collect())
    }

    /// Copies the bindings into a standard map. Use `iter` to keep ordering.
    pub fn to_object(&self) -> HashMap<String, T>
    where
        T: Clone,
    {
        self.iter().map(|(name, value)| (name.to_owned(), value.clone())).collect()
    }

    pub fn size(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn from_entries(entries: Vec<Entry<T>>) -> Self {
        Self { entries: Arc::new(entries) }
    }

    fn find(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|(name, _)| name.as_str() == key)
    }

    fn without(&self, key: &str) -> Vec<Entry<T>> {
        self.entries.iter().filter(|(name, _)| name.as_str() != key).cloned().collect()
    }
}
