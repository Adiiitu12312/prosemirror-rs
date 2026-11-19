//! Persistent sequences ported from rope-sequence 1.3.4, `index.js`.
//! Copyright (C) 2016 by Marijn Haverbeke <marijn@haverbeke.berlin>.

use std::sync::Arc;

const GOOD_LEAF_SIZE: usize = 200;

/// A mostly balanced, persistent sequence of leaf arrays.
#[derive(Debug)]
pub struct RopeSequence<T>(Option<Arc<Node<T>>>);

#[derive(Debug)]
enum Node<T> {
    Leaf(Vec<T>),
    Append {
        left: RopeSequence<T>,
        right: RopeSequence<T>,
        len: usize,
        depth: usize,
    },
}

impl<T> Clone for RopeSequence<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Default for RopeSequence<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> From<Vec<T>> for RopeSequence<T> {
    fn from(values: Vec<T>) -> Self {
        if values.is_empty() { Self::new() } else { Self(Some(Arc::new(Node::Leaf(values)))) }
    }
}

impl<T> From<&Self> for RopeSequence<T> {
    fn from(rope: &Self) -> Self {
        rope.clone()
    }
}

impl<T> RopeSequence<T> {
    pub const fn new() -> Self {
        Self(None)
    }

    pub fn len(&self) -> usize {
        match self.0.as_deref() {
            Some(Node::Leaf(items)) => items.len(),
            Some(Node::Append { len, .. }) => *len,
            None => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    /// Tests whether the ropes share the same root.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(left), Some(right)) => Arc::ptr_eq(left, right),
            (None, None) => true,
            _ => false,
        }
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        match self.0.as_deref()? {
            Node::Leaf(items) => items.get(index),
            Node::Append { left, right, .. } => {
                let left_len = left.len();
                if index < left_len { left.get(index) } else { right.get(index - left_len) }
            }
        }
    }

    /// Visit the elements between two boundaries. A descending range visits in reverse.
    /// Returning `false` stops traversal immediately.
    pub fn for_each(&self, from: usize, to: usize, mut visit: impl FnMut(&T, usize) -> bool) -> bool {
        self.visit(from.min(self.len()), to.min(self.len()), 0, &mut visit)
    }

    pub fn map<U>(&self, from: usize, to: usize, mut map: impl FnMut(&T, usize) -> U) -> Vec<U> {
        let mut result = Vec::with_capacity(from.min(self.len()).abs_diff(to.min(self.len())));
        self.for_each(from, to, |item, index| {
            result.push(map(item, index));
            true
        });
        result
    }

    fn visit(&self, from: usize, to: usize, start: usize, visit: &mut impl FnMut(&T, usize) -> bool) -> bool {
        match self.0.as_deref() {
            None => true,
            Some(Node::Leaf(values)) if from <= to => (from..to).all(|i| visit(&values[i], start + i)),
            Some(Node::Leaf(values)) => (to..from).rev().all(|i| visit(&values[i], start + i)),
            Some(Node::Append { left, right, .. }) => {
                let boundary = left.len();
                if from <= to {
                    (from >= boundary || left.visit(from, to.min(boundary), start, visit))
                        && (to <= boundary || right.visit(from.saturating_sub(boundary), to - boundary, start + boundary, visit))
                } else {
                    (from <= boundary || right.visit(from - boundary, to.saturating_sub(boundary), start + boundary, visit))
                        && (to >= boundary || left.visit(from.min(boundary), to, start, visit))
                }
            }
        }
    }

    fn depth(&self) -> usize {
        if let Some(Node::Append { depth, .. }) = self.0.as_deref() { *depth } else { 0 }
    }

    fn branch(left: Self, right: Self) -> Self {
        let len = left.len() + right.len();
        let depth = left.depth().max(right.depth()) + 1;
        Self(Some(Arc::new(Node::Append { left, right, len, depth })))
    }
}

impl<T: Clone> RopeSequence<T> {
    pub fn append(&self, other: impl Into<Self>) -> Self {
        let other = other.into();
        if other.is_empty() {
            return self.clone();
        }
        if self.is_empty() {
            return other;
        }
        if other.len() < GOOD_LEAF_SIZE {
            if let Some(joined) = self.leaf_append(&other) {
                return joined;
            }
        }
        if self.len() < GOOD_LEAF_SIZE {
            if let Some(joined) = other.leaf_prepend(self) {
                return joined;
            }
        }
        self.append_inner(other)
    }

    pub fn prepend(&self, other: impl Into<Self>) -> Self {
        other.into().append(self)
    }

    pub fn slice(&self, from: usize, to: usize) -> Self {
        let to = to.min(self.len());
        if from >= to {
            return Self::new();
        }
        if from == 0 && to == self.len() {
            return self.clone();
        }
        match self.0.as_deref().expect("a nonempty slice has a root") {
            Node::Leaf(values) => Self::from(values[from..to].to_vec()),
            Node::Append { left, right, .. } => {
                let boundary = left.len();
                if to <= boundary {
                    left.slice(from, to)
                } else if from >= boundary {
                    right.slice(from - boundary, to - boundary)
                } else {
                    left.slice(from, boundary).append(right.slice(0, to - boundary))
                }
            }
        }
    }

    pub fn flatten(&self) -> Vec<T> {
        self.map(0, self.len(), |value, _| value.clone())
    }

    fn leaf_append(&self, other: &Self) -> Option<Self> {
        match self.0.as_deref()? {
            Node::Leaf(values) => {
                let len = values.len() + other.len();
                if len > GOOD_LEAF_SIZE {
                    return None;
                }
                let mut joined = Vec::with_capacity(len);
                joined.extend_from_slice(values);
                other.for_each(0, other.len(), |value, _| { joined.push(value.clone()); true });
                Some(Self::from(joined))
            }
            Node::Append { left, right, .. } => {
                right.leaf_append(other).map(|joined| Self::branch(left.clone(), joined))
            }
        }
    }

    fn leaf_prepend(&self, other: &Self) -> Option<Self> {
        match self.0.as_deref()? {
            Node::Leaf(values) => {
                let len = values.len() + other.len();
                if len > GOOD_LEAF_SIZE {
                    return None;
                }
                let mut joined = Vec::with_capacity(len);
                other.for_each(0, other.len(), |value, _| { joined.push(value.clone()); true });
                joined.extend_from_slice(values);
                Some(Self::from(joined))
            }
            Node::Append { left, right, .. } => {
                left.leaf_prepend(other).map(|joined| Self::branch(joined, right.clone()))
            }
        }
    }

    fn append_inner(&self, other: Self) -> Self {
        if let Some(Node::Append { left, right, .. }) = self.0.as_deref() {
            if left.depth() > right.depth().max(other.depth()) {
                return Self::branch(left.clone(), Self::branch(right.clone(), other));
            }
        }
        Self::branch(self.clone(), other)
    }
}
