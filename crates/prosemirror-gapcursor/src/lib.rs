//! Native editor hooks from prosemirror-gapcursor 1.4.0.
//! The host supplies textblock edges, hit testing, and a cursor decoration widget.

use std::sync::Arc;

use prosemirror_keymap::{keymap, Dispatch, KeyCommand, KeyEvent, Keymap, Platform};
use prosemirror_model::{Fragment, ResolvedPos, Slice};
use prosemirror_state::{EditorState, Selection, Transaction};
use prosemirror_view::{Decoration, DecorationSet};
use serde_json::json;

pub use prosemirror_state::{find_gap_cursor_from, valid_gap_cursor};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Arrow {
    Left,
    Right,
    Up,
    Down,
}

impl Arrow {
    fn direction(self) -> i32 {
        match self {
            Self::Left | Self::Up => -1,
            Self::Right | Self::Down => 1,
        }
    }
}

pub trait GapCursorView {
    fn editable(&self) -> bool;
    fn end_of_textblock(&self, arrow: Arrow) -> bool;
    /// Document offset of the node containing the point, if there is one.
    fn node_at_point(&self, x: f64, y: f64) -> Option<usize>;
}

pub struct GapCursorPlugin<V> {
    keys: Keymap<V>,
    widget_id: u64,
}

pub fn gap_cursor<V: GapCursorView>(platform: Platform, widget_id: u64) -> GapCursorPlugin<V> {
    let bindings = [
        ("ArrowLeft", Arrow::Left),
        ("ArrowRight", Arrow::Right),
        ("ArrowUp", Arrow::Up),
        ("ArrowDown", Arrow::Down),
    ].map(|(key, arrow)| {
        let command: KeyCommand<V> = Arc::new(move |state: &EditorState, dispatch: Dispatch<'_>, view: &V| {
            if let Some(selection) = arrow_selection(state, view, arrow) {
                dispatch_selection(state, dispatch, selection);
                true
            } else {
                false
            }
        });
        (key, command)
    });
    GapCursorPlugin {
        keys: keymap(bindings, platform).expect("valid arrow bindings"),
        widget_id,
    }
}

impl<V: GapCursorView> GapCursorPlugin<V> {
    pub fn decorations(&self, state: &EditorState) -> Option<DecorationSet> {
        let Selection::GapCursor { pos } = state.selection else { return None };
        let widget = Decoration::widget(
            pos, self.widget_id, json!({"key": "gapcursor", "class": "ProseMirror-gapcursor"}),
        );
        Some(DecorationSet::create(&state.doc, vec![widget]))
    }

    pub fn create_selection_between(
        &self,
        anchor: &ResolvedPos,
        head: &ResolvedPos,
    ) -> Option<Selection> {
        (anchor.pos == head.pos && valid_gap_cursor(head))
            .then(|| Selection::gap_cursor(head.pos))
    }

    pub fn handle_keydown(
        &self,
        state: &EditorState,
        dispatch: Dispatch<'_>,
        view: &V,
        event: KeyEvent<'_>,
    ) -> bool {
        self.keys.handle_keydown(state, dispatch, view, event)
    }

    pub fn handle_click(
        &self,
        state: &EditorState,
        dispatch: Dispatch<'_>,
        view: &V,
        pos: usize,
        x: f64,
        y: f64,
    ) -> bool {
        if !view.editable() {
            return false;
        }
        let Ok(resolved) = state.doc.resolve(pos) else { return false };
        if !valid_gap_cursor(&resolved)
            || view.node_at_point(x, y).and_then(|inside| state.doc.node_at(inside))
                .is_some_and(|node| node.is_selectable())
        {
            return false;
        }
        dispatch_selection(state, dispatch, Selection::gap_cursor(pos));
        true
    }

    /// Prepare an inline context for an IME composition. Return false to continue normal input.
    pub fn before_input(&self, state: &EditorState, dispatch: Dispatch<'_>, input_type: &str) -> bool {
        if input_type == "insertCompositionText" {
            if let Some(transaction) = composition_transaction(state) {
                dispatch(transaction);
            }
        }
        false
    }
}

fn arrow_selection<V: GapCursorView>(state: &EditorState, view: &V, arrow: Arrow) -> Option<Selection> {
    let dir = arrow.direction();
    let selection = &state.selection;
    let mut start = state.doc.resolve(if dir > 0 { selection.to(&state.doc) } else { selection.from() }).ok()?;
    let mut must_move = selection.empty_in(&state.doc);
    if matches!(selection, Selection::Text { .. }) {
        if !view.end_of_textblock(arrow) || start.depth == 0 {
            return None;
        }
        let boundary = if dir > 0 { start.after(start.depth) } else { start.before(start.depth) };
        start = state.doc.resolve(boundary).ok()?;
        must_move = false;
    }
    find_gap_cursor_from(&start, dir, must_move).map(|pos| Selection::gap_cursor(pos.pos))
}

fn dispatch_selection(state: &EditorState, dispatch: Dispatch<'_>, selection: Selection) {
    let mut transaction = state.tr();
    transaction.set_selection(selection);
    dispatch(transaction);
}

fn composition_transaction(state: &EditorState) -> Option<Transaction> {
    let Selection::GapCursor { pos } = state.selection else { return None };
    let resolved = state.doc.resolve(pos).ok()?;
    let wrappers = resolved.parent().content_match_at(resolved.index(resolved.depth))?
        .find_wrapping(&state.schema, "text")?;
    let mut fragment = Fragment::empty();
    for name in wrappers.iter().rev() {
        let wrapper = state.schema.node_type(name)?.create_and_fill(None, Some(fragment), None).ok()??;
        fragment = Fragment::from_nodes(vec![wrapper]);
    }
    let mut transaction = state.tr();
    transaction.replace(pos, pos, Slice::new(fragment, 0, 0));
    if !transaction.doc_changed() {
        return None;
    }
    let target = transaction.doc().resolve(pos + 1).ok()?;
    transaction.set_selection(Selection::near(&target, 1));
    Some(transaction)
}
