use std::collections::HashMap;
use std::sync::Arc;

use prosemirror_model::Schema;
use prosemirror_state::{Selection, Transaction};
use prosemirror_transform::{Mapping, Step};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CollabError, Rebaseable};

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct StoredCollab {
    pub(crate) version: u64,
    origins: Vec<StoredOrigin>,
    unconfirmed: Vec<StoredStep>,
}

#[derive(Deserialize, Serialize)]
struct StoredStep {
    step: Value,
    inverted: Value,
    origin: usize,
}

impl StoredStep {
    fn new(step: &Step, inverted: &Step, origin: usize) -> Self {
        Self {
            step: as_json(step),
            inverted: as_json(inverted),
            origin,
        }
    }
}

#[derive(Deserialize, Serialize)]
struct StoredOrigin {
    before: Value,
    steps: Vec<Value>,
    mapping: Mapping,
    initial_selection: Selection,
    selection: Selection,
    selection_set: bool,
    stored_marks: Option<Vec<Value>>,
    stored_marks_set: bool,
    time: u64,
    meta: HashMap<String, Value>,
    scroll: bool,
    appended: Option<Box<Self>>,
}

impl StoredOrigin {
    fn new(tr: &Transaction) -> Self {
        Self {
            before: as_json(tr.before()),
            steps: tr.steps().iter().map(as_json).collect(),
            mapping: tr.mapping().clone(),
            initial_selection: tr.initial_selection().clone(),
            selection: tr.selection(),
            selection_set: tr.selection_set(),
            stored_marks: tr.stored_marks.as_ref().map(|marks| marks.iter().map(as_json).collect()),
            stored_marks_set: tr.stored_marks_set(),
            time: tr.time,
            meta: tr.meta.clone(),
            scroll: tr.scrolled_into_view(),
            appended: tr.appended_transaction().map(|transaction| Box::new(Self::new(transaction))),
        }
    }

    fn restore(&self, schema: &Schema) -> Result<Transaction, CollabError> {
        let before = schema.node_from_json(&self.before)?;
        let mut transaction = Transaction::new(before, self.initial_selection.clone());
        for step in &self.steps {
            transaction.step(Step::from_json(schema, step)?).map_err(CollabError::Step)?;
        }
        transaction.transform.mapping = self.mapping.clone();
        if self.selection_set {
            transaction.set_selection(self.selection.clone());
        } else {
            transaction.set_mapped_selection(self.selection.clone());
        }
        let marks = self.stored_marks.as_ref().map(|marks| {
            marks.iter().map(|mark| schema.mark_from_json(mark)).collect::<Result<Vec<_>, _>>()
        }).transpose()?;
        if self.stored_marks_set {
            transaction.set_stored_marks(marks);
        } else {
            transaction.stored_marks = marks;
        }
        transaction.set_time(self.time);
        transaction.meta = self.meta.clone();
        if self.scroll {
            transaction.scroll_into_view();
        }
        if let Some(appended) = &self.appended {
            transaction.set_appended_transaction(Arc::new(appended.restore(schema)?));
        }
        Ok(transaction)
    }
}

impl StoredCollab {
    pub(crate) fn new(version: u64) -> Self {
        Self { version, ..Self::default() }
    }

    pub(crate) fn append(&mut self, tr: &Transaction) {
        let origin = self.origins.len();
        self.origins.push(StoredOrigin::new(tr));
        self.unconfirmed.extend(tr.steps().iter().zip(&tr.transform.docs).map(|(step, doc)| {
            StoredStep::new(step, &step.invert(doc), origin)
        }));
    }

    pub(crate) fn from_unconfirmed(version: u64, steps: &[Rebaseable]) -> Self {
        let mut stored = Self::new(version);
        let mut origins = HashMap::new();
        for item in steps {
            let origin = *origins.entry(Arc::as_ptr(&item.origin)).or_insert_with(|| {
                let index = stored.origins.len();
                stored.origins.push(StoredOrigin::new(&item.origin));
                index
            });
            stored.unconfirmed.push(StoredStep::new(&item.step, &item.inverted, origin));
        }
        stored
    }

    pub(crate) fn unconfirmed_after(
        &self,
        skip: usize,
        schema: &Schema,
        live_origins: Option<&[Arc<Transaction>]>,
    ) -> Result<Vec<Rebaseable>, CollabError> {
        let mut origins = HashMap::new();
        let mut steps = Vec::new();
        let live_origins = live_origins.filter(|origins| origins.len() == self.unconfirmed.len());
        for (index, item) in self.unconfirmed.iter().enumerate().skip(skip) {
            let origin = if let Some(live) = live_origins {
                Arc::clone(&live[index])
            } else {
                match origins.entry(item.origin) {
                    std::collections::hash_map::Entry::Occupied(entry) => Arc::clone(entry.get()),
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        let stored = self.origins.get(item.origin).ok_or(CollabError::InvalidState)?;
                        Arc::clone(entry.insert(Arc::new(stored.restore(schema)?)))
                    }
                }
            };
            steps.push(Rebaseable {
                step: Step::from_json(schema, &item.step)?,
                inverted: Step::from_json(schema, &item.inverted)?,
                origin,
            });
        }
        Ok(steps)
    }

    pub(crate) fn into_value(self) -> Value {
        as_json(&self)
    }
}

fn as_json(value: &impl Serialize) -> Value {
    serde_json::to_value(value).expect("serializable collaboration state")
}
