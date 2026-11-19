//! Native editor props and lifecycle hooks.

use std::{collections::HashMap, rc::Rc};

use prosemirror_model::{Node, Slice};
use prosemirror_state::{EditorState, PluginKey, Transaction};

use crate::{DecorationGroup, Event, KeyInput, PointerInput};

pub type Attributes = HashMap<String, String>;
pub type EditorAttributes = Rc<dyn Fn(&EditorState) -> Attributes>;
pub type Decorations = Rc<dyn Fn(&EditorState) -> DecorationGroup>;
pub type Editable = Rc<dyn Fn(&EditorState) -> bool>;
pub type KeyHandler = Rc<dyn Fn(&EditorState, &KeyInput) -> EventAction>;
pub type TextHandler = Rc<dyn Fn(&EditorState, usize, usize, &str) -> EventAction>;
pub type PasteHandler = Rc<dyn Fn(&EditorState, &Slice, bool) -> EventAction>;
pub type DropHandler = Rc<dyn Fn(&EditorState, &Slice, usize, bool) -> EventAction>;
pub type ClickHandler = Rc<dyn Fn(&EditorState, usize, &PointerInput) -> EventAction>;
pub type PasteTransform = Rc<dyn Fn(&EditorState, Slice, bool) -> Slice>;
pub type DispatchTransaction = Rc<dyn Fn(&EditorState, Transaction) -> Option<EditorState>>;
pub type MakeNodeView = Rc<dyn Fn(&Node, usize, &DecorationGroup) -> Box<dyn NodeView>>;
pub type MakePluginView = Rc<dyn Fn(&EditorState) -> Box<dyn PluginView>>;

#[derive(Default)]
pub enum EventAction {
    #[default]
    Continue,
    Prevent,
    Dispatch(Box<Transaction>),
}

impl EventAction {
    pub fn transaction(transaction: Transaction) -> Self {
        Self::Dispatch(Box::new(transaction))
    }
}

/// Describes native changes reported by a host for a custom view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMutation {
    Layout,
    Selection,
    Content,
}

/// The native controller associated with a rendered document node.
pub trait NodeView {
    /// Accept a new node or request recreation of the view.
    fn update(&mut self, _node: &Node, _pos: usize, _decorations: &DecorationGroup) -> bool { false }
    fn select(&mut self) {}
    fn deselect(&mut self) {}
    fn stop_event(&self, _event: &Event) -> bool { false }
    fn ignore_mutation(&self, _mutation: ViewMutation) -> bool { false }
    fn destroy(&mut self) {}
}

pub trait PluginView {
    fn update(&mut self, _state: &EditorState, _previous: &EditorState) {}
    fn destroy(&mut self) {}
}

#[derive(Clone, Default)]
pub struct PluginProps {
    pub editable: Option<Editable>,
    pub attributes: Option<EditorAttributes>,
    pub decorations: Option<Decorations>,
    pub node_views: HashMap<String, MakeNodeView>,
    pub plugin_view: Option<MakePluginView>,
    pub handle_key_down: Option<KeyHandler>,
    pub handle_key_up: Option<KeyHandler>,
    pub handle_text_input: Option<TextHandler>,
    pub handle_paste: Option<PasteHandler>,
    pub handle_drop: Option<DropHandler>,
    pub handle_click: Option<ClickHandler>,
    pub handle_double_click: Option<ClickHandler>,
    pub handle_triple_click: Option<ClickHandler>,
    pub transform_pasted: Option<PasteTransform>,
}

#[derive(Clone, Default)]
pub struct EditorProps {
    pub direct: PluginProps,
    pub view_plugins: Vec<Rc<PluginProps>>,
    pub state_plugins: HashMap<PluginKey, Rc<PluginProps>>,
    pub dispatch_transaction: Option<DispatchTransaction>,
}

impl EditorProps {
    /// Yield direct props, view plugins, and attached state plugins in precedence order.
    pub fn providers<'a>(&'a self, state: &'a EditorState) -> impl Iterator<Item = &'a PluginProps> + 'a {
        std::iter::once(&self.direct)
            .chain(self.view_plugins.iter().map(Rc::as_ref))
            .chain(state.plugins.iter().filter_map(|plugin| self.state_plugins.get(&plugin.key).map(Rc::as_ref)))
    }
}
