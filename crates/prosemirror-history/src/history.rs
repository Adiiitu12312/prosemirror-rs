use std::sync::Arc;

use prosemirror_model::{ModelError, Schema};
use prosemirror_state::{EditorState, Plugin, PluginKey, Selection, SelectionBookmark, StateField, Transaction};
use prosemirror_transform::{Mapping, Step, StepMap, Transform};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HISTORY_KEY: &str = "history$";
const CLOSE_HISTORY_KEY: &str = "closeHistory$";
const DEPTH_OVERFLOW: usize = 20;
const MAX_EMPTY_ITEMS: usize = 500;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryItem {
    pub map: StepMap,
    pub step: Option<Step>,
    pub selection: Option<SelectionBookmark>,
    pub mirror_offset: Option<usize>,
}

impl HistoryItem {
    fn map_only(map: StepMap) -> Self {
        Self {
            map,
            step: None,
            selection: None,
            mirror_offset: None,
        }
    }

    fn merge(&self, other: &Self) -> Option<Self> {
        if other.selection.is_some() {
            return None;
        }
        let step = other.step.as_ref()?.merge(self.step.as_ref()?)?;
        Some(Self {
            map: step.get_map().invert(),
            step: Some(step),
            selection: self.selection.clone(),
            mirror_offset: None,
        })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(try_from = "BranchJson")]
pub struct Branch {
    pub items: Vec<HistoryItem>,
    pub event_count: usize,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BranchJson {
    Grouped {
        items: Vec<HistoryItem>,
        event_count: usize,
    },
    Legacy(Vec<LegacyHistoryItem>),
}

#[derive(Deserialize)]
struct LegacyHistoryItem {
    step: Step,
    inverted: Step,
    selection: prosemirror_state::Selection,
}

impl TryFrom<BranchJson> for Branch {
    type Error = ModelError;

    fn try_from(branch: BranchJson) -> Result<Self, Self::Error> {
        Ok(match branch {
            BranchJson::Grouped { items, event_count } => Self { items, event_count },
            BranchJson::Legacy(items) => Self {
                event_count: items.len(),
                items: items
                    .into_iter()
                    .map(|item| {
                        if matches!(item.step, Step::Custom(_)) {
                            return Err(ModelError::Serialization(
                                "Legacy history items cannot contain custom step maps".into(),
                            ));
                        }
                        Ok(HistoryItem {
                            map: item.step.get_map(),
                            step: Some(item.inverted),
                            selection: Some(item.selection.bookmark()),
                            mirror_offset: None,
                        })
                    })
                    .collect::<Result<_, _>>()?,
            },
        })
    }
}

struct PoppedEvent {
    remaining: Branch,
    transaction: Transaction,
    selection: SelectionBookmark,
}

impl Branch {
    fn pop_event(&self, state: &EditorState, preserve_items: bool) -> Option<PoppedEvent> {
        let end = self.items.iter().rposition(|item| item.selection.is_some())?;
        let mut remap = preserve_items.then(|| self.remapping(end, self.items.len()));
        let mut map_from = remap.as_ref().map_or(0, |remap| remap.maps.len());
        let mut transaction = state.tr();
        let mut add_before = Vec::new();
        let mut add_after = Vec::new();

        for index in (end..self.items.len()).rev() {
            let item = &self.items[index];
            let Some(step) = &item.step else {
                if remap.is_none() {
                    let mapping = self.remapping(end, index + 1);
                    map_from = mapping.maps.len();
                    remap = Some(mapping);
                }
                map_from -= 1;
                add_before.push(item.clone());
                continue;
            };

            if let Some(mapping) = &mut remap {
                add_before.push(HistoryItem::map_only(item.map.clone()));
                let mapped = step.map(&mapping.slice(map_from, mapping.maps.len()));
                map_from -= 1;
                if let Some(step) = mapped {
                    if transaction.step(step).is_ok() {
                        let map = transaction.mapping().maps.last().expect("step map").clone();
                        let mut item = HistoryItem::map_only(map.clone());
                        item.mirror_offset = Some(add_after.len() + add_before.len());
                        add_after.push(item);
                        mapping.append_map(map, Some(map_from));
                    }
                }
            } else {
                let _ = transaction.step(step.clone());
            }

            if let Some(selection) = &item.selection {
                let selection = if let Some(mapping) = &remap {
                    selection.map(&mapping.slice(map_from, mapping.maps.len()))
                } else {
                    selection.clone()
                };
                let mut items = self.items[..end].to_vec();
                items.extend(add_before.into_iter().rev());
                items.extend(add_after);
                return Some(PoppedEvent {
                    remaining: Self {
                        items,
                        event_count: self.event_count - 1,
                    },
                    transaction,
                    selection,
                });
            }
        }
        None
    }

    fn add_transform(
        &mut self,
        transform: &Transform,
        mut selection: Option<SelectionBookmark>,
        options: HistoryOptions,
        preserve_items: bool,
    ) {
        let mut new_items = Vec::<HistoryItem>::new();
        for index in 0..transform.steps.len() {
            let item = HistoryItem {
                map: transform.mapping.maps[index].clone(),
                step: Some(transform.steps[index].invert(&transform.docs[index])),
                selection: selection.take(),
                mirror_offset: None,
            };
            if item.selection.is_some() {
                self.event_count += 1;
            }
            let previous = new_items.last().or_else(|| self.items.last());
            let merged = (!preserve_items)
                .then(|| previous.and_then(|previous| previous.merge(&item)))
                .flatten();
            if let Some(merged) = merged {
                if index == 0 {
                    self.items.pop();
                } else {
                    new_items.pop();
                }
                new_items.push(merged);
            } else {
                new_items.push(item);
            }
        }
        let overflow = self.event_count.saturating_sub(options.depth);
        if overflow > DEPTH_OVERFLOW {
            let cut = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.selection.is_some())
                .nth(overflow)
                .map_or(self.items.len(), |(index, _)| index);
            self.items.drain(..cut);
            self.event_count -= overflow;
        }
        self.items.extend(new_items);
    }

    fn remapping(&self, from: usize, to: usize) -> Mapping {
        let mut maps = Mapping::new();
        for (index, item) in self.items.iter().enumerate().take(to).skip(from) {
            let mirror = item
                .mirror_offset
                .and_then(|offset| index.checked_sub(offset))
                .filter(|mirror| *mirror >= from)
                .map(|mirror| mirror - from);
            maps.append_map(item.map.clone(), mirror);
        }
        maps
    }

    fn add_maps(&mut self, maps: &[StepMap]) {
        if self.event_count > 0 {
            self.items.extend(maps.iter().cloned().map(HistoryItem::map_only));
        }
    }

    fn rebase(&mut self, transform: &Transform, rebased_count: usize) {
        if self.event_count == 0 {
            return;
        }
        let original_len = self.items.len();
        let rebased_count = original_len.min(rebased_count);
        let start = original_len - rebased_count;
        let tail = self.items.split_off(start);
        self.event_count -= tail.iter().filter(|item| item.selection.is_some()).count();
        let mapping = &transform.mapping;
        let mut new_until = transform.steps.len();
        let mut rebased = Vec::new();
        for (offset, item) in tail.into_iter().enumerate() {
            let original = rebased_count - offset - 1;
            let Some(pos) = mapping.get_mirror(original) else {
                continue;
            };
            new_until = new_until.min(pos);
            let map = mapping.maps[pos].clone();
            if item.step.is_some() {
                let selection = item
                    .selection
                    .map(|bookmark| bookmark.map(&mapping.slice(original + 1, pos)));
                self.event_count += selection.is_some() as usize;
                rebased.push(HistoryItem {
                    map,
                    step: Some(transform.steps[pos].invert(&transform.docs[pos])),
                    selection,
                    mirror_offset: None,
                });
            } else {
                rebased.push(HistoryItem::map_only(map));
            }
        }
        self.items.extend(
            (rebased_count..new_until).map(|index| HistoryItem::map_only(mapping.maps[index].clone())),
        );
        let rebased_len = rebased.len();
        self.items.extend(rebased);
        if self.items.iter().filter(|item| item.step.is_none()).count() > MAX_EMPTY_ITEMS {
            self.compress(original_len.saturating_sub(rebased_len));
        }
    }

    pub fn compress(&mut self, up_to: usize) {
        let up_to = up_to.min(self.items.len());
        let mut remap = self.remapping(0, up_to);
        let mut map_from = remap.maps.len();
        let mut items = Vec::<HistoryItem>::new();
        let mut events = 0;
        for (index, item) in self.items.iter().enumerate().rev() {
            if index >= up_to {
                events += item.selection.is_some() as usize;
                items.push(item.clone());
                continue;
            }
            let step = item
                .step
                .as_ref()
                .and_then(|step| step.map(&remap.slice(map_from, remap.maps.len())));
            map_from -= 1;
            let Some(step) = step else {
                continue;
            };
            let map = step.get_map();
            remap.append_map(map.clone(), Some(map_from));
            let selection = item
                .selection
                .as_ref()
                .map(|bookmark| bookmark.map(&remap.slice(map_from, remap.maps.len())));
            events += selection.is_some() as usize;
            let new_item = HistoryItem {
                map: map.invert(),
                step: Some(step),
                selection,
                mirror_offset: None,
            };
            if let Some(merged) = items.last().and_then(|previous| previous.merge(&new_item)) {
                *items.last_mut().expect("last merged item") = merged;
            } else {
                items.push(new_item);
            }
        }
        items.reverse();
        self.items = items;
        self.event_count = events;
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct HistoryOptions {
    pub depth: usize,
    pub new_group_delay: u64,
}

impl Default for HistoryOptions {
    fn default() -> Self {
        Self {
            depth: 100,
            new_group_delay: 500,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryState {
    pub done: Branch,
    pub undone: Branch,
    prev_ranges: Option<Vec<(usize, usize)>>,
    prev_time: u64,
    prev_composition: Option<i64>,
}

impl Default for HistoryState {
    fn default() -> Self {
        Self {
            done: Branch::default(),
            undone: Branch::default(),
            prev_ranges: None,
            prev_time: 0,
            prev_composition: Some(-1),
        }
    }
}

impl HistoryState {
    fn close_event(&mut self) {
        self.prev_ranges = None;
        self.prev_time = 0;
        self.prev_composition = Some(-1);
    }

    fn record(
        mut self,
        tr: &Transaction,
        old_selection: SelectionBookmark,
        options: HistoryOptions,
        preserve_items: bool,
    ) -> Self {
        if let Some(history) = tr.get_meta(HISTORY_KEY).and_then(read_meta) {
            return history.state;
        }
        if tr.get_meta(CLOSE_HISTORY_KEY).and_then(Value::as_bool) == Some(true) {
            self.close_event();
        }
        if !tr.doc_changed() {
            return self;
        }

        let appended = tr.appended_transaction();
        let maps = &tr.mapping().maps;
        if let Some(history) = appended.and_then(|tr| tr.get_meta(HISTORY_KEY)).and_then(read_meta) {
            if history.redo {
                self.done.add_transform(&tr.transform, None, options, preserve_items);
                self.prev_ranges = Some(ranges_for(maps));
            } else {
                self.undone.add_transform(&tr.transform, None, options, preserve_items);
                self.prev_ranges = None;
            }
        } else if add_to_history(tr) && appended.is_none_or(add_to_history) {
            let composition = tr.get_meta("composition").and_then(Value::as_i64);
            let new_group = self.prev_time == 0
                || (appended.is_none()
                    && self.prev_composition != composition
                    && (self.prev_time < tr.time.saturating_sub(options.new_group_delay)
                        || !is_adjacent_to(&maps[0], self.prev_ranges.as_deref())));
            self.done.add_transform(
                &tr.transform,
                new_group.then_some(old_selection),
                options,
                preserve_items,
            );
            self.undone = Branch::default();
            self.prev_ranges = if appended.is_some() {
                map_ranges(self.prev_ranges.take(), tr.mapping())
            } else {
                Some(ranges_for(maps))
            };
            self.prev_time = tr.time;
            self.prev_composition = composition.or(self.prev_composition);
        } else {
            if let Some(count) = tr.get_meta("rebased").and_then(Value::as_u64).filter(|count| *count != 0) {
                self.done.rebase(&tr.transform, count as usize);
                self.undone.rebase(&tr.transform, count as usize);
            } else {
                self.done.add_maps(maps);
                self.undone.add_maps(maps);
            }
            self.prev_ranges = map_ranges(self.prev_ranges.take(), tr.mapping());
        }
        self
    }
}

#[derive(Deserialize, Serialize)]
struct HistoryMeta {
    redo: bool,
    state: HistoryState,
}

fn read_meta(value: &Value) -> Option<HistoryMeta> {
    serde_json::from_value(value.clone()).ok()
}

fn add_to_history(tr: &Transaction) -> bool {
    tr.get_meta("addToHistory").and_then(Value::as_bool) != Some(false)
}

fn is_adjacent_to(map: &StepMap, ranges: Option<&[(usize, usize)]>) -> bool {
    let mut adjacent = false;
    if let Some(ranges) = ranges {
        map.for_each(|start, end, _, _| {
            adjacent |= ranges.iter().any(|&(from, to)| start <= to && end >= from);
        });
    }
    adjacent
}

fn ranges_for(maps: &[StepMap]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    for map in maps.iter().rev() {
        map.for_each(|_, _, from, to| ranges.push((from, to)));
        if !ranges.is_empty() {
            break;
        }
    }
    ranges
}

fn map_ranges(ranges: Option<Vec<(usize, usize)>>, mapping: &Mapping) -> Option<Vec<(usize, usize)>> {
    ranges.map(|ranges| {
        ranges
            .into_iter()
            .filter_map(|(from, to)| {
                let from = mapping.map(from, 1);
                let to = mapping.map(to, -1);
                (from <= to).then_some((from, to))
            })
            .collect()
    })
}

fn must_preserve_items(state: &EditorState) -> bool {
    state.plugins.iter().any(|plugin| {
        plugin.spec.get("historyPreserveItems").and_then(Value::as_bool) == Some(true)
    })
}

pub struct HistoryField {
    pub depth: usize,
}

impl Default for HistoryField {
    fn default() -> Self {
        Self {
            depth: HistoryOptions::default().depth,
        }
    }
}

impl StateField for HistoryField {
    fn init(&self, _config: &Value) -> Value {
        serde_json::to_value(HistoryState::default()).expect("serializable history")
    }

    fn apply(&self, tr: &Transaction, value: &Value) -> Value {
        let options = HistoryOptions { depth: self.depth, ..HistoryOptions::default() };
        let history = match tr.before().schema() {
            Some(schema) => deserialize_history(value, schema),
            None => serde_json::from_value::<HistoryState>(value.clone()).ok().filter(|history| {
                history.done.items.iter().chain(&history.undone.items)
                    .all(|item| !matches!(item.step, Some(Step::Custom(_))))
            }),
        }.unwrap_or_default();
        record(history, tr, tr.initial_selection().bookmark(), options, false)
    }

    fn apply_with_state(
        &self,
        tr: &Transaction,
        value: &Value,
        old_state: &EditorState,
        _new_state: &EditorState,
    ) -> Value {
        record(
            deserialize_history(value, &old_state.schema).unwrap_or_default(),
            tr,
            old_state.selection.bookmark(),
            history_options(old_state),
            must_preserve_items(old_state),
        )
    }
}

fn record(
    history: HistoryState,
    tr: &Transaction,
    selection: SelectionBookmark,
    options: HistoryOptions,
    preserve_items: bool,
) -> Value {
    serde_json::to_value(history.record(tr, selection, options, preserve_items))
        .expect("serializable history")
}

fn history_options(state: &EditorState) -> HistoryOptions {
    state
        .plugins
        .iter()
        .find(|plugin| plugin.key.name == HISTORY_KEY)
        .and_then(|plugin| plugin.spec.get("config"))
        .and_then(|options| serde_json::from_value(options.clone()).ok())
        .unwrap_or_default()
}

pub fn history_plugin(depth: usize) -> Plugin {
    history(HistoryOptions { depth, ..HistoryOptions::default() })
}

pub fn history(mut options: HistoryOptions) -> Plugin {
    if options.depth == 0 {
        options.depth = HistoryOptions::default().depth;
    }
    if options.new_group_delay == 0 {
        options.new_group_delay = HistoryOptions::default().new_group_delay;
    }
    let mut plugin = Plugin::new(
        PluginKey { name: HISTORY_KEY.into() },
        Some(Arc::new(HistoryField { depth: options.depth })),
    );
    plugin.spec.insert("config".into(), serde_json::to_value(options).expect("history options"));
    plugin
}

fn read_history(state: &EditorState) -> Option<HistoryState> {
    let value = state.get_plugin_state(HISTORY_KEY)?;
    deserialize_history(value, &state.schema)
}

fn deserialize_history(value: &Value, schema: &Schema) -> Option<HistoryState> {
    let mut history: HistoryState = serde_json::from_value(value.clone()).ok()?;
    for item in history.done.items.iter_mut().chain(&mut history.undone.items) {
        if let Some(step) = &mut item.step {
            let json = serde_json::to_value(&*step).ok()?;
            *step = Step::from_json(schema, &json).ok()?;
        }
    }
    Some(history)
}

pub fn close_history(tr: &mut Transaction) -> &mut Transaction {
    tr.set_meta(CLOSE_HISTORY_KEY, Value::Bool(true))
}

/// Returns whether the transaction was generated by the history plugin.
pub fn is_history_transaction(tr: &Transaction) -> bool {
    tr.get_meta(HISTORY_KEY).is_some_and(|meta| !meta.is_null())
}

fn selection_from_changes(state: &EditorState, tr: &Transaction) -> Selection {
    for (step, map) in tr.steps().iter().zip(&tr.mapping().maps) {
        let mut end = None;
        map.for_each(|_, to, _, _| end = Some(to));
        if end.is_none() {
            if let Step::AddMark { to, .. } | Step::RemoveMark { to, .. } = step {
                end = Some(*to);
            }
        }
        if let Some(pos) = end.and_then(|end| state.doc.resolve(end).ok()) {
            return Selection::near(&pos, 1);
        }
    }
    state.selection.clone()
}

fn history_transaction(state: &EditorState, redo: bool, scroll: bool) -> Option<Transaction> {
    let mut history = read_history(state)?;
    let preserve_items = must_preserve_items(state);
    let options = history_options(state);
    let (branch, opposite) = if redo {
        (&mut history.undone, &mut history.done)
    } else {
        (&mut history.done, &mut history.undone)
    };
    let popped = branch.pop_event(state, preserve_items)?;
    *branch = popped.remaining;
    let mut tr = popped.transaction;
    let bookmark = selection_from_changes(state, &tr).bookmark();
    opposite.add_transform(&tr.transform, Some(bookmark), options, preserve_items);
    let selection = popped.selection.resolve(tr.doc());
    history.close_event();
    tr.set_selection(selection);
    tr.set_meta(
        HISTORY_KEY,
        serde_json::to_value(HistoryMeta { redo, state: history }).expect("serializable history"),
    );
    if scroll {
        tr.scroll_into_view();
    }
    Some(tr)
}

fn command(
    state: &EditorState,
    redo: bool,
    scroll: bool,
    mut dispatch: impl FnMut(Transaction),
) -> bool {
    let Some(tr) = history_transaction(state, redo, scroll) else {
        return false;
    };
    dispatch(tr);
    true
}

pub fn undo(state: &EditorState, dispatch: impl FnMut(Transaction)) -> bool {
    command(state, false, true, dispatch)
}

pub fn redo(state: &EditorState, dispatch: impl FnMut(Transaction)) -> bool {
    command(state, true, true, dispatch)
}

pub fn undo_no_scroll(state: &EditorState, dispatch: impl FnMut(Transaction)) -> bool {
    command(state, false, false, dispatch)
}

pub fn redo_no_scroll(state: &EditorState, dispatch: impl FnMut(Transaction)) -> bool {
    command(state, true, false, dispatch)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryInput {
    Undo,
    Redo,
}

/// Handle the host's undo or redo input when the editor is editable.
pub fn history_input(
    state: &EditorState,
    editable: bool,
    input: HistoryInput,
    dispatch: impl FnMut(Transaction),
) -> bool {
    editable && command(state, input == HistoryInput::Redo, true, dispatch)
}

pub fn undo_depth(state: &EditorState) -> usize {
    read_history(state).map_or(0, |history| history.done.event_count)
}

pub fn redo_depth(state: &EditorState) -> usize {
    read_history(state).map_or(0, |history| history.undone.event_count)
}
