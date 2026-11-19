//! Port of prosemirror-collab 1.3.1, src/collab.ts.

mod storage;

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::Arc;

use prosemirror_model::ModelError;
use prosemirror_state::{
    EditorState, EditorStateConfig, Plugin, PluginKey, Selection, StateField, Transaction,
    TypedStateField,
};
use prosemirror_transform::{Step, StepResult, Transform};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

use storage::StoredCollab;

pub const COLLAB_KEY: &str = "collab$";
const ORIGINS_META: &str = "collabOrigins$";
type Origins = Arc<[Arc<Transaction>]>;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ClientId {
    Number(i64),
    String(String),
}

impl ClientId {
    fn matches(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(number), Self::String(text)) | (Self::String(text), Self::Number(number)) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    *number == 0
                } else {
                    trimmed.parse::<f64>().is_ok_and(|value| value == *number as f64)
                }
            }
            _ => self == other,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CollabConfig {
    pub version: u64,
    pub client_id: Option<ClientId>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ReceiveOptions {
    pub map_selection_backward: bool,
}

#[derive(Debug, Error)]
pub enum CollabError {
    #[error("Collaboration plugin is missing")]
    MissingPlugin,
    #[error("Collaboration plugin state could not be decoded")]
    InvalidState,
    #[error("Received {steps} steps and {clients} client identifiers")]
    InvalidBatch { steps: usize, clients: usize },
    #[error("The collaboration version overflowed")]
    VersionOverflow,
    #[error("Collaboration step failed: {0}")]
    Step(String),
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug)]
pub struct Rebaseable<T = Transaction> {
    pub step: Step,
    pub inverted: Step,
    pub origin: Arc<T>,
}

pub fn rebase_steps<T>(
    steps: &[Rebaseable<T>],
    over: &[Step],
    transform: &mut Transform,
) -> Result<Vec<Rebaseable<T>>, CollabError> {
    for item in steps.iter().rev() {
        transform.step(item.inverted.clone()).map_err(CollabError::Step)?;
    }
    for step in over {
        transform.step(step.clone()).map_err(CollabError::Step)?;
    }

    let mut map_from = steps.len();
    let mut rebased = Vec::new();
    for item in steps {
        let mapped = item.step.map(&transform.mapping.slice(map_from, transform.steps.len()));
        map_from -= 1;
        if let Some(step) = mapped {
            if matches!(transform.maybe_step(step.clone()), StepResult::Ok(_)) {
                let index = transform.steps.len() - 1;
                transform.mapping.set_mirror(map_from, index);
                rebased.push(Rebaseable {
                    inverted: step.invert(&transform.docs[index]),
                    step,
                    origin: Arc::clone(&item.origin),
                });
            }
        }
    }
    Ok(rebased)
}

struct CollabField {
    version: u64,
}

impl StateField for CollabField {
    fn init(&self, _: &Value) -> Value {
        StoredCollab::new(self.version).into_value()
    }

    fn apply(&self, tr: &Transaction, value: &Value) -> Value {
        if let Some(state) = tr.get_meta(COLLAB_KEY) {
            return state.clone();
        }
        if !tr.doc_changed() {
            return value.clone();
        }
        let mut state: StoredCollab = serde_json::from_value(value.clone())
            .expect("valid collaboration plugin state");
        state.append(tr);
        state.into_value()
    }
}

struct OriginField;

impl OriginField {
    fn restored(state: &EditorState) -> Origins {
        stored(state).and_then(|local| local.unconfirmed_after(0, &state.schema, None))
            .map(|pending| pending.into_iter().map(|item| item.origin).collect())
            .unwrap_or_default()
    }
}

impl TypedStateField for OriginField {
    type Value = Origins;

    fn init(&self, _: &EditorStateConfig, state: &EditorState) -> Self::Value {
        Self::restored(state)
    }

    fn apply(
        &self, tr: &Transaction, value: &Origins, old: &EditorState, new: &EditorState,
    ) -> Self::Value {
        self.apply_shared(&Arc::new(tr.clone()), value, old, new)
    }

    fn apply_shared(
        &self, tr: &Arc<Transaction>, value: &Origins, _: &EditorState, new: &EditorState,
    ) -> Self::Value {
        if let Some(origins) = tr.get_typed_meta::<Origins>(ORIGINS_META) {
            Arc::clone(origins)
        } else if tr.get_meta(COLLAB_KEY).is_some() {
            Self::restored(new)
        } else if tr.doc_changed() {
            value.iter().cloned().chain(std::iter::repeat_n(Arc::clone(tr), tr.steps().len())).collect()
        } else {
            Arc::clone(value)
        }
    }
}

pub fn collab(config: CollabConfig) -> Plugin {
    let client = config.client_id.unwrap_or_else(|| {
        let random = RandomState::new().hash_one(std::process::id());
        ClientId::Number((random % u64::from(u32::MAX)) as i64)
    });
    let mut plugin = Plugin::new(
        PluginKey { name: COLLAB_KEY.into() },
        Some(Arc::new(CollabField { version: config.version })),
    ).with_typed_state_field(OriginField);
    plugin.spec.insert("historyPreserveItems".into(), Value::Bool(true));
    plugin.spec.insert("config".into(), json!({
        "version": config.version,
        "clientID": client,
    }));
    plugin
}

fn client_id(state: &EditorState) -> Result<ClientId, CollabError> {
    let key = PluginKey { name: COLLAB_KEY.into() };
    let plugin = key.get(state).ok_or(CollabError::MissingPlugin)?;
    let id = plugin.spec.get("config")
        .and_then(|config| config.get("clientID"))
        .ok_or(CollabError::InvalidState)?;
    Ok(serde_json::from_value(id.clone())?)
}

fn stored(state: &EditorState) -> Result<StoredCollab, CollabError> {
    let state = state.get_plugin_state(COLLAB_KEY).ok_or(CollabError::MissingPlugin)?;
    Ok(serde_json::from_value(state.clone())?)
}

fn live_origins(state: &EditorState) -> Option<&[Arc<Transaction>]> {
    state.get_typed_plugin_state::<Origins>(COLLAB_KEY).map(Arc::as_ref)
}

pub fn receive_transaction(
    state: &EditorState,
    steps: &[Step],
    client_ids: &[ClientId],
    options: ReceiveOptions,
) -> Result<Transaction, CollabError> {
    if steps.len() != client_ids.len() {
        return Err(CollabError::InvalidBatch { steps: steps.len(), clients: client_ids.len() });
    }
    let local = stored(state)?;
    let version = local.version.checked_add(steps.len() as u64).ok_or(CollabError::VersionOverflow)?;
    let id = client_id(state)?;
    let ours = client_ids.iter().take_while(|client| client.matches(&id)).count();
    let mut tr = state.tr();
    let remote = &steps[ours..];
    let unconfirmed = local.unconfirmed_after(ours, &state.schema, live_origins(state))?;

    let unconfirmed = if remote.is_empty() {
        unconfirmed
    } else {
        let mut outcome = Ok(Vec::new());
        tr.apply_transform(|transform| {
            outcome = rebase_steps(&unconfirmed, remote, transform);
        });
        let outcome = outcome?;
        if options.map_selection_backward {
            if let Selection::Text { anchor, head } = state.selection {
                let anchor = tr.doc().resolve(tr.mapping().map(anchor, -1))?;
                let head = tr.doc().resolve(tr.mapping().map(head, -1))?;
                tr.set_mapped_selection(Selection::between(&anchor, &head, -1));
            }
        }
        tr.set_meta("rebased", Value::from(unconfirmed.len()))
            .set_meta("addToHistory", Value::Bool(false));
        outcome
    };
    let origins = unconfirmed.iter().map(|item| Arc::clone(&item.origin)).collect::<Origins>();
    let updated = StoredCollab::from_unconfirmed(version, &unconfirmed);
    tr.set_meta(COLLAB_KEY, updated.into_value()).set_typed_meta(ORIGINS_META, origins);
    Ok(tr)
}

#[derive(Debug)]
pub struct SendableSteps {
    pub version: u64,
    pub steps: Vec<Step>,
    pub client_id: ClientId,
    pub origins: Vec<Arc<Transaction>>,
}

pub fn sendable_steps(state: &EditorState) -> Result<Option<SendableSteps>, CollabError> {
    let local = stored(state)?;
    let unconfirmed = local.unconfirmed_after(0, &state.schema, live_origins(state))?;
    if unconfirmed.is_empty() {
        return Ok(None);
    }
    let (steps, origins) = unconfirmed.into_iter().map(|item| (item.step, item.origin)).unzip();
    Ok(Some(SendableSteps { version: local.version, steps, client_id: client_id(state)?, origins }))
}

pub fn get_version(state: &EditorState) -> Result<u64, CollabError> {
    let value = state.get_plugin_state(COLLAB_KEY).ok_or(CollabError::MissingPlugin)?;
    value.get("version").and_then(Value::as_u64).ok_or(CollabError::InvalidState)
}
