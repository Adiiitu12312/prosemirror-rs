use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use prosemirror_model::{Fragment, Mark, Node, Slice};
use prosemirror_transform::{Mapping, MarkFilter, Step, StepResult, Transform};
use crate::plugin::ErasedValue;
use crate::selection::Selection;

#[derive(Clone, Debug)]
pub struct Transaction {
    pub transform: Transform,
    pub selection: Selection,
    pub stored_marks: Option<Vec<Mark>>,
    pub meta: HashMap<String, Value>,
    typed_meta: HashMap<String, ErasedValue>,
    pub time: u64,
    initial_selection: Selection,
    selection_origin: Selection,
    selection_index: usize,
    selection_set: bool,
    stored_marks_set: bool,
    scrolled_into_view: bool,
    appended_transaction: Option<Arc<Transaction>>,
}

impl Transaction {
    pub fn new(doc: Node, selection: Selection) -> Self {
        Self {
            transform: Transform::new(doc),
            selection: selection.clone(),
            stored_marks: None,
            meta: HashMap::new(),
            typed_meta: HashMap::new(),
            time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_millis() as u64),
            initial_selection: selection.clone(),
            selection_origin: selection,
            selection_index: 0,
            selection_set: false,
            stored_marks_set: false,
            scrolled_into_view: false,
            appended_transaction: None,
        }
    }

    pub fn doc(&self) -> &Node {
        &self.transform.doc
    }

    pub fn before(&self) -> &Node {
        self.transform.before()
    }

    pub fn initial_selection(&self) -> &Selection {
        &self.initial_selection
    }

    pub fn steps(&self) -> &[Step] {
        &self.transform.steps
    }

    pub fn mapping(&self) -> &Mapping {
        &self.transform.mapping
    }

    pub fn doc_changed(&self) -> bool {
        self.transform.doc_changed()
    }

    pub fn selection(&self) -> Selection {
        if self.selection_index == self.steps().len() {
            return self.selection.clone();
        }
        let mapping = self.mapping().slice(self.selection_index, self.steps().len());
        self.selection_origin.map(self.doc(), &mapping)
    }

    pub fn set_selection(&mut self, selection: Selection) -> &mut Self {
        assert!(
            selection.anchor() <= self.doc().content().size()
                && selection.head(self.doc()) <= self.doc().content().size(),
            "Selection must point at the current document"
        );
        self.selection_origin = selection.clone();
        self.selection_index = self.steps().len();
        self.selection = selection;
        self.selection_set = true;
        self.stored_marks = None;
        self.stored_marks_set = false;
        self
    }

    pub fn selection_set(&self) -> bool {
        self.selection_set
    }

    pub fn set_mapped_selection(&mut self, selection: Selection) -> &mut Self {
        self.set_selection(selection);
        self.selection_set = false;
        self
    }

    pub fn set_stored_marks(&mut self, marks: Option<Vec<Mark>>) -> &mut Self {
        self.stored_marks = marks;
        self.stored_marks_set = true;
        self
    }

    pub fn stored_marks_set(&self) -> bool {
        self.stored_marks_set
    }

    pub fn ensure_marks(&mut self, marks: Vec<Mark>) -> &mut Self {
        let current = self
            .stored_marks
            .clone()
            .unwrap_or_else(|| self.marks_at(self.selection().from()));
        if !Mark::same_set(&current, &marks) {
            self.set_stored_marks(Some(marks));
        }
        self
    }

    pub fn add_stored_mark(&mut self, mark: Mark) -> &mut Self {
        let marks = self.current_marks();
        self.ensure_marks(Mark::add_to_set(&marks, mark))
    }

    pub fn remove_stored_mark(&mut self, mark: &Mark) -> &mut Self {
        let mut marks = self.current_marks();
        if let Some(index) = marks.iter().position(|item| item == mark) {
            marks.remove(index);
        }
        self.ensure_marks(marks)
    }

    pub fn remove_stored_mark_type(&mut self, type_name: &str) -> &mut Self {
        let marks = self.current_marks();
        self.ensure_marks(Mark::remove_from_set(&marks, type_name))
    }

    pub fn set_time(&mut self, time: u64) -> &mut Self {
        self.time = time;
        self
    }

    pub fn set_meta(&mut self, key: impl Into<String>, value: Value) -> &mut Self {
        let key = key.into();
        self.typed_meta.remove(&key);
        self.meta.insert(key, value);
        self
    }

    pub fn get_meta(&self, key: impl AsRef<str>) -> Option<&Value> {
        self.meta.get(key.as_ref())
    }

    pub fn set_typed_meta<T: Any + Send + Sync>(
        &mut self,
        key: impl Into<String>,
        value: T,
    ) -> &mut Self {
        let key = key.into();
        self.meta.remove(&key);
        self.typed_meta.insert(key, Arc::new(value));
        self
    }

    pub fn get_typed_meta<T: Any + Send + Sync>(&self, key: impl AsRef<str>) -> Option<&T> {
        self.typed_meta.get(key.as_ref())?.downcast_ref()
    }

    pub fn is_generic(&self) -> bool {
        self.meta.is_empty() && self.typed_meta.is_empty() && self.appended_transaction.is_none()
    }

    pub fn appended_transaction(&self) -> Option<&Transaction> {
        self.appended_transaction.as_deref()
    }

    pub fn set_appended_transaction(&mut self, root: Arc<Transaction>) {
        self.appended_transaction = Some(root);
    }

    pub fn scroll_into_view(&mut self) -> &mut Self {
        self.scrolled_into_view = true;
        self
    }

    pub fn scrolled_into_view(&self) -> bool {
        self.scrolled_into_view
    }

    pub fn step(&mut self, step: Step) -> Result<(), String> {
        match self.maybe_step(step) {
            StepResult::Ok(_) => Ok(()),
            StepResult::Failed(error) => Err(error),
        }
    }

    pub fn maybe_step(&mut self, step: Step) -> StepResult {
        let start = self.steps().len();
        let result = self.transform.maybe_step(step);
        self.finish_steps(start);
        result
    }

    pub fn apply_transform(&mut self, edit: impl FnOnce(&mut Transform)) -> &mut Self {
        let start = self.steps().len();
        edit(&mut self.transform);
        self.finish_steps(start);
        self
    }

    pub fn replace(&mut self, from: usize, to: usize, slice: Slice) -> &mut Self {
        self.apply_transform(|transform| {
            transform.replace(from, to, slice);
        })
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
        self.apply_transform(|transform| {
            transform.replace_range(from, to, slice);
        })
    }

    pub fn replace_range_with(&mut self, from: usize, to: usize, node: Node) -> &mut Self {
        self.apply_transform(|transform| {
            transform.replace_range_with(from, to, node);
        })
    }

    pub fn delete_range(&mut self, from: usize, to: usize) -> &mut Self {
        self.apply_transform(|transform| {
            transform.delete_range(from, to);
        })
    }

    pub fn replace_selection(&mut self, slice: Slice) -> &mut Self {
        let selection = self.selection();
        let empty = slice.content.child_count() == 0;
        if empty && selection == Selection::All {
            self.delete(0, self.doc().content().size());
            let at_start = Selection::at_start(self.doc());
            if at_start != self.selection() {
                self.set_selection(at_start);
            }
            return self;
        }

        let from = selection.from();
        let to = selection.to(self.doc());
        let preserved = if empty && matches!(selection, Selection::Text { .. }) {
            self.doc()
                .resolve(from)
                .ok()
                .zip(self.doc().resolve(to).ok())
                .and_then(|(from, to)| from.marks_across(&to))
        } else {
            None
        };
        let mut last = slice.content.children.last();
        let mut last_parent = None;
        for _ in 0..slice.open_end {
            last_parent = last;
            last = last.and_then(|node| node.content().children.last());
        }
        let inline = last.map_or_else(
            || last_parent.is_some_and(|node| node.is_textblock()),
            |node| node.is_inline(),
        );
        let map_from = self.steps().len();
        self.replace_range(from, to, slice);
        self.selection_to_insertion_end(map_from, if inline { -1 } else { 1 });
        if let Some(marks) = preserved {
            self.ensure_marks(marks);
        }
        self
    }

    pub fn replace_selection_with(&mut self, mut node: Node, inherit_marks: bool) -> &mut Self {
        let selection = self.selection();
        let from = selection.from();
        let to = selection.to(self.doc());
        if inherit_marks {
            let marks = self.stored_marks.clone().unwrap_or_else(|| self.marks_for_range(from, to));
            node = node.with_marks(marks);
        }
        let bias = if node.is_inline() { -1 } else { 1 };
        let map_from = self.steps().len();
        self.replace_range_with(from, to, node);
        self.selection_to_insertion_end(map_from, bias);
        self
    }

    pub fn delete_selection(&mut self) -> &mut Self {
        self.replace_selection(Slice::empty())
    }

    pub fn insert_text_at_selection(&mut self, text: &str) -> &mut Self {
        if text.is_empty() {
            self.delete_selection()
        } else {
            let text = self.text_node(text, Vec::new());
            self.replace_selection_with(text, true)
        }
    }

    pub fn insert_text(&mut self, text: &str, from: usize, to: usize) -> &mut Self {
        if text.is_empty() {
            return self.delete_range(from, to);
        }
        let marks = self.stored_marks.clone().unwrap_or_else(|| self.marks_for_range(from, to));
        let text = self.text_node(text, marks);
        self.replace_range_with(from, to, text);
        let selection = self.selection();
        if !selection.empty_in(self.doc()) {
            if let Ok(to) = self.doc().resolve(selection.to(self.doc())) {
                self.set_selection(Selection::near(&to, 1));
            }
        }
        self
    }

    pub fn add_mark(&mut self, from: usize, to: usize, mark: Mark) -> &mut Self {
        self.apply_transform(|transform| {
            transform.add_mark(from, to, mark);
        })
    }

    pub fn remove_mark(&mut self, from: usize, to: usize, mark: Mark) -> &mut Self {
        self.apply_transform(|transform| {
            transform.remove_mark(from, to, mark);
        })
    }

    pub fn remove_marks(&mut self, from: usize, to: usize, filter: MarkFilter<'_>) -> &mut Self {
        self.apply_transform(|transform| {
            transform.remove_marks(from, to, filter);
        })
    }

    fn finish_steps(&mut self, before: usize) {
        if self.steps().len() != before {
            self.selection = self.selection();
            self.stored_marks = None;
            self.stored_marks_set = false;
        }
    }

    fn current_marks(&self) -> Vec<Mark> {
        self.stored_marks
            .clone()
            .unwrap_or_else(|| self.marks_at(self.selection().head(self.doc())))
    }

    fn marks_at(&self, pos: usize) -> Vec<Mark> {
        self.doc().resolve(pos).map(|pos| pos.marks()).unwrap_or_default()
    }

    fn text_node(&self, text: &str, marks: Vec<Mark>) -> Node {
        match self.doc().schema() {
            Some(schema) => schema.text(text, Some(marks)),
            None => Node::new_text(text, marks),
        }
    }

    fn marks_for_range(&self, from: usize, to: usize) -> Vec<Mark> {
        if from == to {
            return self.marks_at(from);
        }
        self.doc()
            .resolve(from)
            .ok()
            .zip(self.doc().resolve(to).ok())
            .and_then(|(from, to)| from.marks_across(&to))
            .unwrap_or_default()
    }

    fn selection_to_insertion_end(&mut self, start: usize, bias: i32) {
        let Some(last) = self.steps().len().checked_sub(1) else {
            return;
        };
        if last < start || !matches!(self.steps()[last], Step::Replace { .. } | Step::ReplaceAround { .. }) {
            return;
        }
        let mut end = None;
        self.mapping().maps[last].for_each(|_, _, _, new_to| {
            end.get_or_insert(new_to);
        });
        if let Some(pos) = end.and_then(|end| self.doc().resolve(end).ok()) {
            self.set_selection(Selection::near(&pos, bias));
        }
    }
}
