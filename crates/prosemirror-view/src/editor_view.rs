//! The editor state and input lifecycle from prosemirror-view's EditorView,
//! with rendering, coordinate queries, and focus supplied by a native host.

use std::sync::Arc;

use prosemirror_model::{DomNode, ModelError, Slice};
use prosemirror_state::{EditorState, Transaction};
use serde_json::json;

use crate::{
    parse_from_clipboard, serialize_for_clipboard, ClipboardData, ClipboardOptions, DecorationGroup,
    DecorationSet, ImeHandler, ImeState, LayoutRect, ViewLayout,
};

pub type DecorationProvider = Arc<dyn Fn(&EditorState) -> Option<DecorationSet> + Send + Sync>;
pub type Editable = Arc<dyn Fn(&EditorState) -> bool + Send + Sync>;
pub type DispatchTransaction = Box<dyn FnMut(&EditorState, Transaction) -> Option<EditorState>>;

pub struct EditorProps {
    pub width: f32,
    pub editable: Option<Editable>,
    pub decorations: Vec<DecorationProvider>,
    pub dispatch_transaction: Option<DispatchTransaction>,
}

impl Default for EditorProps {
    fn default() -> Self {
        Self { width: 800.0, editable: None, decorations: Vec::new(), dispatch_transaction: None }
    }
}

pub struct ViewFrame<'a> {
    pub state: &'a EditorState,
    pub layout: &'a ViewLayout,
    pub decorations: &'a DecorationGroup,
    pub editable: bool,
}

pub trait ViewHost {
    fn render(&mut self, frame: ViewFrame<'_>);
    fn has_focus(&self) -> bool;
    fn focus(&mut self);

    fn scroll_to_selection(&mut self, _rect: LayoutRect) {}
    fn coords_at_pos(&self, _pos: usize, _side: i32) -> Option<LayoutRect> { None }
    fn pos_at_coords(&self, _x: f32, _y: f32) -> Option<usize> { None }
    fn destroy(&mut self) {}
}

pub struct EditorView<H: ViewHost> {
    state: EditorState,
    props: EditorProps,
    host: Option<H>,
    layout: ViewLayout,
    decorations: DecorationGroup,
    ime: ImeHandler,
    composition: u64,
}

impl<H: ViewHost> EditorView<H> {
    pub fn new(host: H, state: EditorState, props: EditorProps) -> Self {
        let layout = ViewLayout::build(&state, props.width);
        let decorations = decorations(&state, &props);
        let mut view = Self {
            state, props, layout, decorations,
            host: Some(host), ime: ImeHandler::default(), composition: 0,
        };
        view.render();
        view
    }

    pub fn state(&self) -> &EditorState {
        &self.state
    }

    pub fn props(&self) -> &EditorProps {
        &self.props
    }

    pub fn editable(&self) -> bool {
        self.host.is_some() && self.props.editable.as_ref().is_none_or(|editable| editable(&self.state))
    }

    pub fn composing(&self) -> bool {
        matches!(self.ime.state, ImeState::Composing { .. })
    }

    pub fn has_focus(&self) -> bool {
        self.host.as_ref().is_some_and(ViewHost::has_focus)
    }

    pub fn focus(&mut self) {
        if let Some(host) = &mut self.host {
            host.focus();
        }
    }

    pub fn update(&mut self, state: EditorState, props: EditorProps) {
        self.props = props;
        self.update_state(state);
    }

    pub fn update_state(&mut self, state: EditorState) {
        let scroll = state.scroll_to_selection > self.state.scroll_to_selection;
        if state.stored_marks.is_some() && self.composing() {
            self.ime.state = ImeState::Idle;
        }
        self.state = state;
        self.layout = ViewLayout::build(&self.state, self.props.width);
        self.decorations = decorations(&self.state, &self.props);
        self.render();
        if scroll {
            let head = self.state.selection.head(&self.state.doc);
            if let Some(rect) = self.coords_at_pos(head, 1) {
                if let Some(host) = &mut self.host {
                    host.scroll_to_selection(rect);
                }
            }
        }
    }

    pub fn dispatch(&mut self, transaction: Transaction) -> bool {
        if self.host.is_none() || transaction.before() != &self.state.doc {
            return false;
        }
        if let Some(dispatch) = &mut self.props.dispatch_transaction {
            let Some(state) = dispatch(&self.state, transaction) else { return false };
            self.update_state(state);
            return true;
        }
        let applied = self.state.apply_transaction(&transaction);
        if applied.transactions.is_empty() {
            return false;
        }
        self.update_state(applied.state);
        true
    }

    pub fn insert_text(&mut self, text: &str) -> bool {
        if !self.editable() {
            return false;
        }
        let from = self.state.selection.from();
        let to = self.state.selection.to(&self.state.doc);
        let tr = self.state.handle_text_input(from, to, text, self.composing()).unwrap_or_else(|| {
            let mut tr = self.state.tr();
            tr.insert_text_at_selection(text).scroll_into_view();
            tr
        });
        self.dispatch(tr)
    }

    pub fn composition_start(&mut self) {
        if self.editable() {
            self.composition = self.composition.wrapping_add(1);
            self.ime.handle_composition_start(&self.state);
        }
    }

    pub fn composition_update(&mut self, text: &str) -> bool {
        if !self.editable() || !self.composing() {
            return false;
        }
        let previous = self.ime.state.clone();
        let mut tr = self.ime.handle_composition_update(&self.state, text);
        tr.set_meta("composition", json!(self.composition));
        if self.dispatch(tr) {
            true
        } else {
            self.ime.state = previous;
            false
        }
    }

    pub fn composition_end(&mut self, text: &str) -> bool {
        if !self.editable() || !self.composing() {
            return false;
        }
        let previous = self.ime.state.clone();
        let mut tr = self.ime.handle_composition_end(&self.state, text);
        tr.set_meta("composition", json!(self.composition));
        if !self.dispatch(tr) {
            self.ime.state = previous;
            return false;
        }
        if let Some(tr) = self.state.handle_composition_end() {
            self.dispatch(tr);
        }
        true
    }

    pub fn pos_at_coords(&self, x: f32, y: f32) -> Option<usize> {
        let host = self.host.as_ref()?;
        let pos = host.pos_at_coords(x, y).unwrap_or_else(|| self.layout.coords_to_pos(x, y));
        (pos <= self.state.doc.content().size()).then_some(pos)
    }

    pub fn coords_at_pos(&self, pos: usize, side: i32) -> Option<LayoutRect> {
        let host = self.host.as_ref()?;
        if pos > self.state.doc.content().size() {
            return None;
        }
        Some(host.coords_at_pos(pos, side).unwrap_or_else(|| self.layout.pos_to_coords(pos)))
    }

    pub fn paste(
        &mut self, text: &str, dom: Option<DomNode>, plain_text: bool, options: &ClipboardOptions<'_>,
    ) -> Result<bool, ModelError> {
        if !self.editable() {
            return Ok(false);
        }
        let head = self.state.doc.resolve(self.state.selection.from())?;
        let Some(slice) = parse_from_clipboard(&self.state.schema, text, dom, plain_text, &head, options)?
            else { return Ok(false) };
        let mut tr = self.state.tr();
        tr.replace_selection(slice).scroll_into_view().set_meta("paste", json!(true)).set_meta("uiEvent", json!("paste"));
        Ok(self.dispatch(tr))
    }

    pub fn serialize_for_clipboard(&self, slice: Slice, options: &ClipboardOptions<'_>) -> Result<ClipboardData, ModelError> {
        serialize_for_clipboard(&self.state.schema, slice, options)
    }

    pub fn destroy(&mut self) {
        if let Some(mut host) = self.host.take() {
            host.destroy();
        }
        self.ime.state = ImeState::Idle;
    }

    pub fn is_destroyed(&self) -> bool {
        self.host.is_none()
    }

    fn render(&mut self) {
        let editable = self.editable();
        if let Some(host) = &mut self.host {
            host.render(ViewFrame { state: &self.state, layout: &self.layout, decorations: &self.decorations, editable });
        }
    }
}

impl<H: ViewHost> Drop for EditorView<H> {
    fn drop(&mut self) {
        self.destroy();
    }
}

fn decorations(state: &EditorState, props: &EditorProps) -> DecorationGroup {
    DecorationGroup {
        members: props.decorations.iter().filter_map(|provider| provider(state)).filter(|set| !set.is_empty()).collect(),
    }
}
