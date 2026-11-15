use std::collections::HashMap;
use serde_json::Value;
use prosemirror_model::{Fragment, Mark, Node, Slice};
use prosemirror_transform::{Mapping, Step, Transform};
use crate::selection::Selection;

#[derive(Clone, Debug)]
pub struct Transaction {
    pub transform: Transform,
    pub selection: Selection,
    pub stored_marks: Option<Vec<Mark>>,
    pub meta: HashMap<String, Value>,
    pub time: u64,
}

impl Transaction {
    pub fn new(doc: Node, selection: Selection) -> Self {
        Self {
            transform: Transform::new(doc),
            selection,
            stored_marks: None,
            meta: HashMap::new(),
            time: 0,
        }
    }

    pub fn doc(&self) -> &Node {
        &self.transform.doc
    }

    pub fn steps(&self) -> &[Step] {
        &self.transform.steps
    }

    pub fn mapping(&self) -> &Mapping {
        &self.transform.mapping
    }

    pub fn doc_changed(&self) -> bool {
        !self.transform.steps.is_empty()
    }

    pub fn set_selection(&mut self, selection: Selection) -> &mut Self {
        self.selection = selection;
        self
    }

    pub fn set_stored_marks(&mut self, marks: Option<Vec<Mark>>) -> &mut Self {
        self.stored_marks = marks;
        self
    }

    pub fn set_meta(&mut self, key: impl Into<String>, value: Value) -> &mut Self {
        self.meta.insert(key.into(), value);
        self
    }

    pub fn get_meta(&self, key: &str) -> Option<&Value> {
        self.meta.get(key)
    }

    pub fn step(&mut self, step: Step) -> Result<(), String> {
        self.transform.step(step)?;
        self.selection = self.selection.map(&self.transform.doc, &self.transform.mapping);
        Ok(())
    }

    pub fn replace(&mut self, from: usize, to: usize, slice: Slice) -> &mut Self {
        let step = Step::replace(from, to, slice);
        let _ = self.step(step);
        self
    }

    pub fn replace_with(&mut self, from: usize, to: usize, content: Fragment) -> &mut Self {
        self.replace(from, to, Slice::new(content, 0, 0))
    }

    pub fn delete(&mut self, from: usize, to: usize) -> &mut Self {
        self.replace(from, to, Slice::empty())
    }

    pub fn insert(&mut self, pos: usize, content: Fragment) -> &mut Self {
        self.replace_with(pos, pos, content)
    }

    pub fn insert_text(&mut self, text: &str, from: usize, to: usize) -> &mut Self {
        let node = Node::new_text(text, self.stored_marks.clone().unwrap_or_default());
        self.replace_with(from, to, Fragment::from_nodes(vec![node]))
    }

    pub fn add_mark(&mut self, from: usize, to: usize, mark: Mark) -> &mut Self {
        let step = Step::add_mark(from, to, mark);
        let _ = self.step(step);
        self
    }

    pub fn remove_mark(&mut self, from: usize, to: usize, mark: Mark) -> &mut Self {
        let step = Step::remove_mark(from, to, mark);
        let _ = self.step(step);
        self
    }
}
