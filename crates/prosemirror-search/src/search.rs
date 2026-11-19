//! Port of prosemirror-search 1.1.0, src/search.ts.

use prosemirror_state::{EditorState, EditorStateConfig, Plugin, PluginKey, Selection, Transaction, TypedStateField};
use prosemirror_view::{Decoration, DecorationSet};
use serde_json::Value;

use crate::{SearchQuery, SearchResult};

pub const SEARCH_KEY: &str = "search$";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchRange {
    pub from: usize,
    pub to: usize,
}

impl SearchRange {
    pub fn new(from: usize, to: usize) -> Self {
        Self { from, to }
    }

    fn for_document(state: &EditorState) -> Self {
        Self::new(0, state.doc.content().size())
    }
}

#[derive(Clone, Default)]
pub struct SearchOptions {
    pub initial_query: Option<SearchQuery>,
    pub initial_range: Option<SearchRange>,
}

#[derive(Clone)]
pub struct SearchState {
    pub query: SearchQuery,
    pub range: Option<SearchRange>,
    decorations: DecorationSet,
}

impl SearchState {
    fn new(state: &EditorState, query: SearchQuery, range: Option<SearchRange>) -> Self {
        let decorations = build_match_decorations(state, &query, range);
        Self { query, range, decorations }
    }

    fn range(&self, state: &EditorState) -> SearchRange {
        self.range.unwrap_or_else(|| SearchRange::for_document(state))
    }
}

struct SearchUpdate {
    query: SearchQuery,
    range: Option<SearchRange>,
}

struct SearchField(SearchOptions);

impl TypedStateField for SearchField {
    type Value = SearchState;

    fn init(&self, _: &EditorStateConfig, state: &EditorState) -> Self::Value {
        SearchState::new(
            state,
            self.0.initial_query.clone().unwrap_or_default(),
            self.0.initial_range,
        )
    }

    fn apply(
        &self,
        tr: &Transaction,
        search: &Self::Value,
        _: &EditorState,
        state: &EditorState,
    ) -> Self::Value {
        if let Some(update) = tr.get_typed_meta::<SearchUpdate>(SEARCH_KEY) {
            return SearchState::new(state, update.query.clone(), update.range);
        }
        if tr.doc_changed() || tr.selection_set() {
            let range = search.range.and_then(|range| {
                let from = tr.mapping().map(range.from, 1);
                let to = tr.mapping().map(range.to, -1);
                (from < to).then_some(SearchRange::new(from, to))
            });
            return SearchState::new(state, search.query.clone(), range);
        }
        search.clone()
    }
}

fn build_match_decorations(
    state: &EditorState,
    query: &SearchQuery,
    range: Option<SearchRange>,
) -> DecorationSet {
    if !query.valid() {
        return DecorationSet::default();
    }
    let range = range.unwrap_or_else(|| SearchRange::for_document(state));
    let mut decorations = Vec::new();
    let mut pos = range.from;
    while let Some(next) = query.find_next(state, pos, range.to) {
        let active = next.from == state.selection.from() && next.to == state.selection.to(&state.doc);
        let class = if active { "ProseMirror-active-search-match" } else { "ProseMirror-search-match" };
        decorations.push(Decoration::inline(
            next.from,
            next.to,
            [("class".into(), class.into())].into(),
            Value::Null,
        ));
        pos = next.to.max(next.from + 1).max(pos + 1);
    }
    DecorationSet::create(&state.doc, decorations)
}

pub fn search(options: SearchOptions) -> Plugin {
    Plugin::new(PluginKey { name: SEARCH_KEY.into() }, None)
        .with_typed_state_field(SearchField(options))
}

pub fn get_search_state(state: &EditorState) -> Option<&SearchState> {
    state.get_typed_plugin_state(SEARCH_KEY)
}

pub fn get_match_highlights(state: &EditorState) -> DecorationSet {
    get_search_state(state).map_or_else(DecorationSet::default, |search| search.decorations.clone())
}

pub fn set_search_state(
    tr: &mut Transaction,
    query: SearchQuery,
    range: Option<SearchRange>,
) -> &mut Transaction {
    tr.set_typed_meta(SEARCH_KEY, SearchUpdate { query, range })
}

fn next_match(
    search: &SearchState,
    state: &EditorState,
    wrap: bool,
    from: usize,
    to: usize,
) -> Option<SearchResult> {
    let range = search.range(state);
    search.query.find_next(state, to.max(range.from), range.to).or_else(|| {
        wrap.then(|| search.query.find_next(state, range.from, from.min(range.to))).flatten()
    })
}

fn prev_match(
    search: &SearchState,
    state: &EditorState,
    wrap: bool,
    from: usize,
    to: usize,
) -> Option<SearchResult> {
    let range = search.range(state);
    search.query.find_prev(state, from.min(range.to), range.from).or_else(|| {
        wrap.then(|| search.query.find_prev(state, range.to, to.max(range.from))).flatten()
    })
}

pub type Dispatch<'a> = Option<&'a mut dyn FnMut(Transaction)>;

fn find_command(state: &EditorState, dispatch: Dispatch<'_>, wrap: bool, forward: bool) -> bool {
    let Some(search) = get_search_state(state).filter(|search| search.query.valid()) else {
        return false;
    };
    let from = state.selection.from();
    let to = state.selection.to(&state.doc);
    let found = if forward { next_match(search, state, wrap, from, to) } else { prev_match(search, state, wrap, from, to) };
    let Some(found) = found else {
        return false;
    };
    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        tr.set_selection(Selection::text(found.from, found.to)).scroll_into_view();
        dispatch(tr);
    }
    true
}

pub fn find_next(state: &EditorState, dispatch: Dispatch<'_>) -> bool {
    find_command(state, dispatch, true, true)
}

pub fn find_next_no_wrap(state: &EditorState, dispatch: Dispatch<'_>) -> bool {
    find_command(state, dispatch, false, true)
}

pub fn find_prev(state: &EditorState, dispatch: Dispatch<'_>) -> bool {
    find_command(state, dispatch, true, false)
}

pub fn find_prev_no_wrap(state: &EditorState, dispatch: Dispatch<'_>) -> bool {
    find_command(state, dispatch, false, false)
}

fn replace_command(state: &EditorState, dispatch: Dispatch<'_>, wrap: bool, move_forward: bool) -> bool {
    let Some(search) = get_search_state(state).filter(|search| search.query.valid()) else {
        return false;
    };
    let from = state.selection.from();
    let Some(next) = next_match(search, state, wrap, from, from) else {
        return false;
    };
    let Some(dispatch) = dispatch else {
        return true;
    };
    let mut tr = state.tr();
    let selection = if from == next.from && state.selection.to(&state.doc) == next.to {
        for replacement in search.query.get_replacements(state, &next).into_iter().rev() {
            tr.replace(replacement.from, replacement.to, replacement.insert);
        }
        let after = move_forward.then(|| next_match(search, state, wrap, next.from, next.to)).flatten();
        if let Some(after) = after {
            Selection::text(tr.mapping().map(after.from, 1), tr.mapping().map(after.to, -1))
        } else {
            Selection::text(next.from, tr.mapping().map(next.to, 1))
        }
    } else if !move_forward {
        return false;
    } else {
        Selection::text(next.from, next.to)
    };
    tr.set_selection(selection).scroll_into_view();
    dispatch(tr);
    true
}

pub fn replace_next(state: &EditorState, dispatch: Dispatch<'_>) -> bool {
    replace_command(state, dispatch, true, true)
}

pub fn replace_next_no_wrap(state: &EditorState, dispatch: Dispatch<'_>) -> bool {
    replace_command(state, dispatch, false, true)
}

pub fn replace_current(state: &EditorState, dispatch: Dispatch<'_>) -> bool {
    replace_command(state, dispatch, false, false)
}

pub fn replace_all(state: &EditorState, dispatch: Dispatch<'_>) -> bool {
    let Some(search) = get_search_state(state) else {
        return false;
    };
    let Some(dispatch) = dispatch else {
        return true;
    };
    let range = search.range(state);
    let mut pos = range.from;
    let mut matches = Vec::new();
    while let Some(next) = search.query.find_next(state, pos, range.to) {
        pos = next.to.max(next.from + 1).max(pos + 1);
        matches.push(next);
    }
    let mut tr = state.tr();
    for matched in matches.into_iter().rev() {
        for replacement in search.query.get_replacements(state, &matched).into_iter().rev() {
            tr.replace(replacement.from, replacement.to, replacement.insert);
        }
    }
    dispatch(tr);
    true
}
