use std::sync::Arc;

use prosemirror_commands::DispatchFn;
use prosemirror_model::DomNode;
use prosemirror_state::{EditorState, Transaction};
use serde_json::Value;

use crate::{IconSpec, MenuError};

/// The zero-based group and item indices of a control. Subsequent
/// indices identify controls inside a dropdown or submenu.
pub type MenuPath = Vec<usize>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum MenuFocus {
    Editor,
    Control(MenuPath),
    #[default]
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuPrompt {
    pub title: String,
    pub fields: Vec<MenuPromptField>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuPromptField {
    pub name: String,
    pub label: String,
    pub value: String,
    pub required: bool,
}

/// Editor and widget operations supplied by the native UI.
pub trait MenuHost {
    fn dispatch(&mut self, transaction: Transaction);

    fn focus(&self) -> MenuFocus {
        MenuFocus::Other
    }

    fn focus_editor(&mut self) {}

    fn focus_control(&mut self, _path: &MenuPath) {}

    fn translate(&self, text: &str) -> String {
        text.to_owned()
    }

    /// Present a host input prompt and return an attribute object.
    /// Returning `None` represents cancellation.
    fn prompt(&mut self, _prompt: &MenuPrompt) -> Option<Value> {
        None
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuEventSource {
    #[default]
    Pointer,
    Keyboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKey {
    ArrowDown,
    ArrowUp,
    ArrowRight,
    ArrowLeft,
    Home,
    End,
    Escape,
    Enter,
    Space,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuEvent {
    pub source: MenuEventSource,
    pub key: Option<MenuKey>,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl MenuEvent {
    pub fn keyboard(key: MenuKey) -> Self {
        Self { source: MenuEventSource::Keyboard, key: Some(key), ..Self::default() }
    }
}

pub type MenuAction = Arc<dyn Fn(&EditorState, &mut dyn MenuHost, &MenuEvent) -> bool + Send + Sync>;
pub type MenuPredicate = Arc<dyn Fn(&EditorState) -> bool + Send + Sync>;
pub type MenuRender = Arc<dyn Fn(&EditorState, &dyn MenuHost) -> DomNode + Send + Sync>;
pub type MenuTitleText = Arc<dyn Fn(&EditorState) -> String + Send + Sync>;

#[derive(Clone)]
pub enum MenuTitle {
    Text(String),
    Dynamic(MenuTitleText),
}

impl From<&str> for MenuTitle {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for MenuTitle {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl MenuTitle {
    pub fn dynamic(resolve: impl Fn(&EditorState) -> String + Send + Sync + 'static) -> Self {
        Self::Dynamic(Arc::new(resolve))
    }

    pub(crate) fn text(&self, state: &EditorState, host: &dyn MenuHost) -> String {
        match self {
            Self::Text(text) => host.translate(text),
            Self::Dynamic(resolve) => host.translate(&resolve(state)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuDisplay {
    Label(String),
    Icon(IconSpec),
    Custom(DomNode),
}

#[derive(Clone, Default)]
pub struct MenuItemSpec {
    pub run: Option<MenuAction>,
    pub select: Option<MenuPredicate>,
    pub enable: Option<MenuPredicate>,
    pub active: Option<MenuPredicate>,
    pub render: Option<MenuRender>,
    pub icon: Option<IconSpec>,
    pub label: Option<String>,
    pub title: Option<MenuTitle>,
    pub class: Option<String>,
    pub css: Option<String>,
}

impl MenuItemSpec {
    pub fn new(action: impl Fn(&EditorState, &mut dyn MenuHost, &MenuEvent) -> bool + Send + Sync + 'static) -> Self {
        Self { run: Some(Arc::new(action)), ..Self::default() }
    }

    pub fn command(command: impl for<'a> Fn(&EditorState, DispatchFn<'a>) -> bool + Send + Sync + 'static) -> Self {
        Self::new(move |state, host, _| command(state, Some(&mut |transaction| host.dispatch(transaction))))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuItemState {
    pub visible: bool,
    pub enabled: bool,
    pub active: bool,
}

#[derive(Clone)]
pub struct MenuItem {
    pub spec: MenuItemSpec,
}

impl MenuItem {
    pub fn new(spec: MenuItemSpec) -> Self {
        Self { spec }
    }

    pub fn state(&self, state: &EditorState) -> MenuItemState {
        let visible = self.spec.select.as_ref().is_none_or(|select| select(state));
        let enabled = visible && self.spec.enable.as_ref().is_none_or(|enable| enable(state));
        let active = enabled && self.spec.active.as_ref().is_some_and(|active| active(state));
        MenuItemState { visible, enabled, active }
    }

    pub fn display(&self, state: &EditorState, host: &dyn MenuHost) -> Result<MenuDisplay, MenuError> {
        if let Some(render) = &self.spec.render {
            Ok(MenuDisplay::Custom(render(state, host)))
        } else if let Some(icon) = &self.spec.icon {
            Ok(MenuDisplay::Icon(icon.clone()))
        } else if let Some(label) = self.spec.label.as_ref().filter(|label| !label.is_empty()) {
            Ok(MenuDisplay::Label(host.translate(label)))
        } else {
            Err(MenuError::MissingDisplay)
        }
    }

    /// Activates the item and restores editor focus when the action leaves
    /// its own control focused.
    pub fn activate(&self, path: &MenuPath, state: &EditorState, host: &mut dyn MenuHost, event: &MenuEvent) -> bool {
        let item_state = self.state(state);
        let Some(run) = self.spec.run.as_ref().filter(|_| item_state.enabled) else { return false };
        let control = MenuFocus::Control(path.clone());
        let focused = host.focus();
        let should_restore = focused == MenuFocus::Editor || focused == control;
        let ran = run(state, host, event);
        if should_restore && host.focus() == control {
            host.focus_editor();
        }
        ran
    }
}

#[derive(Clone, Default)]
pub struct DropdownOptions {
    pub label: Option<String>,
    pub title: Option<String>,
    pub class: Option<String>,
    pub css: Option<String>,
}

#[derive(Clone)]
pub struct Dropdown {
    pub content: Vec<MenuElement>,
    pub options: DropdownOptions,
}

impl Dropdown {
    pub fn new(content: Vec<MenuElement>, options: DropdownOptions) -> Self {
        Self { content, options }
    }
}

#[derive(Clone)]
pub struct DropdownSubmenu {
    pub content: Vec<MenuElement>,
    pub label: String,
}

impl DropdownSubmenu {
    pub fn new(content: Vec<MenuElement>, label: impl Into<String>) -> Self {
        Self { content, label: label.into() }
    }
}

#[derive(Clone)]
pub enum MenuElement {
    Item(MenuItem),
    Dropdown(Dropdown),
    Submenu(DropdownSubmenu),
}

impl MenuElement {
    pub(crate) fn children(&self) -> Option<&[Self]> {
        match self {
            Self::Item(_) => None,
            Self::Dropdown(dropdown) => Some(&dropdown.content),
            Self::Submenu(submenu) => Some(&submenu.content),
        }
    }

    pub fn visible(&self, state: &EditorState) -> bool {
        match self {
            Self::Item(item) => item.spec.select.as_ref().is_none_or(|select| select(state)),
            Self::Dropdown(dropdown) => dropdown.content.iter().any(|child| child.visible(state)),
            Self::Submenu(submenu) => submenu.content.iter().any(|child| child.visible(state)),
        }
    }
}

impl From<MenuItem> for MenuElement {
    fn from(item: MenuItem) -> Self { Self::Item(item) }
}

impl From<Dropdown> for MenuElement {
    fn from(dropdown: Dropdown) -> Self { Self::Dropdown(dropdown) }
}

impl From<DropdownSubmenu> for MenuElement {
    fn from(submenu: DropdownSubmenu) -> Self { Self::Submenu(submenu) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusDirection {
    Forwards,
    Backwards,
}

/// Finds the next visible control, wrapping around the ends of the scope.
/// A missing start index searches from the relevant edge of the scope.
pub fn find_focusable_index(visible: &[bool], start: Option<usize>, direction: FocusDirection) -> Option<usize> {
    if visible.is_empty() {
        return None;
    }
    let length = visible.len();
    let mut index = match (start.filter(|start| *start < length), direction) {
        (Some(start), FocusDirection::Forwards) => (start + 1) % length,
        (Some(0) | None, FocusDirection::Backwards) => length - 1,
        (Some(start), FocusDirection::Backwards) => start - 1,
        (None, FocusDirection::Forwards) => 0,
    };
    for _ in 0..length {
        if visible[index] {
            return Some(index);
        }
        index = match direction {
            FocusDirection::Forwards => (index + 1) % length,
            FocusDirection::Backwards if index == 0 => length - 1,
            FocusDirection::Backwards => index - 1,
        };
    }
    None
}

pub fn keyboard_move_focus(
    visible: &[bool], current: Option<usize>, key: MenuKey, orientation: Orientation,
) -> Option<usize> {
    match (key, orientation) {
        (MenuKey::ArrowRight, Orientation::Horizontal) | (MenuKey::ArrowDown, Orientation::Vertical) =>
            find_focusable_index(visible, current, FocusDirection::Forwards),
        (MenuKey::ArrowLeft, Orientation::Horizontal) | (MenuKey::ArrowUp, Orientation::Vertical) =>
            find_focusable_index(visible, current, FocusDirection::Backwards),
        (MenuKey::Home, _) => find_focusable_index(visible, None, FocusDirection::Forwards),
        (MenuKey::End, _) => find_focusable_index(visible, None, FocusDirection::Backwards),
        _ => None,
    }
}
