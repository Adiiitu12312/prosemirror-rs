use prosemirror_model::{ContentMatch, Fragment, Mark, Node, NodeRange, NodeType, Slice};
use serde_json::Value;

use crate::{mark, replace, structure};
use crate::{Mapping, Step, StepResult};

#[derive(Clone, Debug)]
pub struct Transform {
    pub doc: Node,
    pub steps: Vec<Step>,
    pub docs: Vec<Node>,
    pub mapping: Mapping,
}

impl Transform {
    pub fn new(doc: Node) -> Self {
        Self {
            doc,
            steps: Vec::new(),
            docs: Vec::new(),
            mapping: Mapping::new(),
        }
    }

    pub fn before(&self) -> &Node {
        self.docs.first().unwrap_or(&self.doc)
    }

    pub fn doc_changed(&self) -> bool {
        !self.steps.is_empty()
    }

    pub fn step(&mut self, step: Step) -> Result<(), String> {
        match self.maybe_step(step) {
            StepResult::Ok(_) => Ok(()),
            StepResult::Failed(error) => Err(error),
        }
    }

    pub fn maybe_step(&mut self, step: Step) -> StepResult {
        let result = step.apply(&self.doc);
        if let StepResult::Ok(doc) = &result {
            self.docs.push(std::mem::replace(&mut self.doc, doc.clone()));
            self.mapping.append_map(step.get_map(), None);
            self.steps.push(step);
        }
        result
    }

    pub fn replace(&mut self, from: usize, to: usize, slice: Slice) -> &mut Self {
        if let Some(step) = replace::replace_step(&self.doc, from, to, slice) {
            self.maybe_step(step);
        }
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

    pub fn replace_range(&mut self, from: usize, to: usize, slice: Slice) -> &mut Self {
        replace::replace_range(self, from, to, slice);
        self
    }

    pub fn replace_range_with(&mut self, from: usize, to: usize, node: Node) -> &mut Self {
        replace::replace_range_with(self, from, to, node);
        self
    }

    pub fn delete_range(&mut self, from: usize, to: usize) -> &mut Self {
        replace::delete_range(self, from, to);
        self
    }

    pub fn add_mark(&mut self, from: usize, to: usize, mark: Mark) -> &mut Self {
        mark::add_mark(self, from, to, mark);
        self
    }

    pub fn remove_mark(&mut self, from: usize, to: usize, mark: Mark) -> &mut Self {
        self.remove_marks(from, to, mark::MarkFilter::Exact(&mark))
    }

    pub fn remove_marks(&mut self, from: usize, to: usize, filter: mark::MarkFilter<'_>) -> &mut Self {
        mark::remove_mark(self, from, to, filter);
        self
    }

    pub fn clear_incompatible(
        &mut self, pos: usize, parent_type: &NodeType, matched: Option<ContentMatch>,
    ) -> &mut Self {
        mark::clear_incompatible(self, pos, parent_type, matched, true);
        self
    }

    pub fn split(&mut self, pos: usize, depth: usize) -> &mut Self {
        self.split_with_types(pos, depth, None)
    }

    pub fn split_with_types(
        &mut self, pos: usize, depth: usize, types_after: Option<&[Option<Node>]>,
    ) -> &mut Self {
        structure::split(self, pos, depth, types_after);
        self
    }

    pub fn join(&mut self, pos: usize, depth: usize) -> &mut Self {
        structure::join(self, pos, depth);
        self
    }

    pub fn lift(&mut self, range: &NodeRange, target: usize) -> &mut Self {
        structure::lift(self, range, target);
        self
    }

    pub fn wrap(&mut self, range: &NodeRange, wrappers: &[Node]) -> &mut Self {
        structure::wrap(self, range, wrappers);
        self
    }

    pub fn set_block_type(
        &mut self, from: usize, to: usize, type_name: &str, attrs: Option<Value>,
    ) -> &mut Self {
        self.set_block_type_with(from, to, type_name, |_| attrs.clone())
    }

    pub fn set_block_type_with(
        &mut self, from: usize, to: usize, type_name: &str, attrs: impl FnMut(&Node) -> Option<Value>,
    ) -> &mut Self {
        structure::set_block_type(self, from, to, type_name, attrs);
        self
    }

    pub fn set_node_markup(
        &mut self, pos: usize, type_name: Option<&str>, attrs: Option<Value>, marks: Option<Vec<Mark>>,
    ) -> &mut Self {
        structure::set_node_markup(self, pos, type_name, attrs, marks);
        self
    }

    pub fn set_node_attribute(&mut self, pos: usize, name: impl Into<String>, value: Value) -> &mut Self {
        self.maybe_step(Step::attr(pos, name, value));
        self
    }

    pub fn set_doc_attribute(&mut self, name: impl Into<String>, value: Value) -> &mut Self {
        self.maybe_step(Step::doc_attr(name, value));
        self
    }

    pub fn add_node_mark(&mut self, pos: usize, mark: Mark) -> &mut Self {
        self.maybe_step(Step::add_node_mark(pos, mark));
        self
    }

    pub fn remove_node_mark(&mut self, pos: usize, mark: Mark) -> &mut Self {
        self.maybe_step(Step::remove_node_mark(pos, mark));
        self
    }

    pub fn remove_node_mark_type(&mut self, pos: usize, type_name: &str) -> &mut Self {
        if let Some(mark) = crate::step::node_at(&self.doc, pos)
            .and_then(|node| node.marks().iter().find(|mark| mark.type_name == type_name)).cloned()
        {
            self.remove_node_mark(pos, mark);
        }
        self
    }
}
