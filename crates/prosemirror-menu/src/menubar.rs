use std::collections::HashMap;

use prosemirror_state::EditorState;

use crate::{
    find_focusable_index, keyboard_move_focus, FocusDirection, MenuDisplay, MenuElement,
    MenuError, MenuEvent, MenuEventSource, MenuFocus, MenuHost, MenuItemState, MenuKey,
    MenuPath, Orientation,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuPosition {
    #[default]
    Before,
    After,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuBarOptions {
    pub position: MenuPosition,
    pub floating: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuControlKind {
    Item,
    Dropdown,
    Submenu,
}

/// A snapshot of a native menu control. Paths are stable across updates,
/// allowing a host to update existing controls without rebuilding them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuControl {
    pub path: MenuPath,
    pub kind: MenuControlKind,
    pub state: MenuItemState,
    pub display: MenuDisplay,
    pub title: Option<String>,
    pub class: Option<String>,
    pub css: Option<String>,
    pub aria_disabled: Option<bool>,
    pub aria_pressed: Option<bool>,
    pub aria_expanded: Option<bool>,
    pub tab_index: i8,
    pub children: Vec<MenuControl>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuGroup {
    pub items: Vec<MenuControl>,
    pub visible: bool,
    pub separator_before: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuSnapshot {
    pub position: MenuPosition,
    pub visible: bool,
    pub groups: Vec<MenuGroup>,
    pub floating: Option<FloatingMenu>,
    pub min_height: f64,
}

pub struct MenuBar {
    content: Vec<Vec<MenuElement>>,
    options: MenuBarOptions,
    open: Vec<MenuPath>,
    tabstops: HashMap<MenuPath, MenuPath>,
    hovered: Option<MenuPath>,
    pub layout: MenuBarLayout,
}

pub fn menu_bar(content: Vec<Vec<MenuElement>>, options: MenuBarOptions) -> MenuBar {
    MenuBar::new(content, options)
}

/// Projects each group and its visibility for a host without a persistent toolbar.
pub fn render_grouped(
    content: &[Vec<MenuElement>], state: &EditorState, host: &dyn MenuHost,
) -> Result<Vec<MenuGroup>, MenuError> {
    projection(
        content, state, host,
        |_, _, _| false,
        |_| None,
    )
}

impl MenuBar {
    pub fn new(content: Vec<Vec<MenuElement>>, options: MenuBarOptions) -> Self {
        Self { content, options, open: Vec::new(), tabstops: HashMap::new(), hovered: None, layout: MenuBarLayout::default() }
    }

    pub fn content(&self) -> &[Vec<MenuElement>] {
        &self.content
    }

    pub fn options(&self) -> MenuBarOptions {
        self.options
    }

    pub fn snapshot(&self, state: &EditorState, host: &dyn MenuHost) -> Result<MenuSnapshot, MenuError> {
        let focused = host.focus();
        let groups = projection(
            &self.content, state, host,
            |path, kind, visible| visible && self.expanded(path, kind, &focused),
            |scope| self.tabstops.get(scope).cloned(),
        )?;
        Ok(MenuSnapshot {
            position: self.options.position,
            visible: groups.iter().any(|group| group.visible),
            groups,
            floating: self.layout.floating,
            min_height: self.layout.max_height,
        })
    }

    /// Re-evaluates hidden controls after an editor state change, moving focus
    /// to the next visible control when the focused control disappeared.
    pub fn update(&mut self, state: &EditorState, host: &mut dyn MenuHost) -> Result<MenuSnapshot, MenuError> {
        self.open.retain(|path| lookup(&self.content, path).is_some_and(|item| item.visible(state)));
        if let MenuFocus::Control(path) = host.focus() {
            let scope = scope_of(&path);
            if !lookup(&self.content, &path).is_some_and(|item| item.visible(state)) {
                self.move_in_scope(scope, Some(&path), FocusDirection::Forwards, state, host);
            }
        }
        self.snapshot(state, host)
    }

    pub fn activate(
        &mut self, path: &MenuPath, state: &EditorState, host: &mut dyn MenuHost, event: MenuEvent,
    ) -> bool {
        if !self.available(path, state, &host.focus()) {
            return false;
        }
        if event.source == MenuEventSource::Pointer {
            self.outside_click(Some(path));
        }
        match lookup(&self.content, path).expect("available control exists") {
            MenuElement::Item(item) => {
                let ran = item.activate(path, state, host, &event);
                if event.source == MenuEventSource::Pointer {
                    self.open.clear();
                }
                ran
            }
            MenuElement::Dropdown(_) if self.open.contains(path) => {
                self.close(path);
                true
            }
            MenuElement::Dropdown(_) | MenuElement::Submenu(_) => {
                self.open(path, event.source == MenuEventSource::Keyboard, state, host);
                true
            }
        }
    }

    /// Routes key handling to the toolbar or active popup. Returns true
    /// when the host should consume the keyboard event.
    pub fn handle_key(
        &mut self, path: &MenuPath, key: MenuKey, state: &EditorState, host: &mut dyn MenuHost,
    ) -> bool {
        if !self.available(path, state, &host.focus()) {
            return false;
        }
        match key {
            MenuKey::Enter | MenuKey::Space => return self.activate(path, state, host, MenuEvent::keyboard(key)),
            MenuKey::ArrowRight if matches!(lookup(&self.content, path), Some(MenuElement::Submenu(_))) => {
                self.open(path, true, state, host);
                return true;
            }
            _ => (),
        }
        let scope = scope_of(path);
        let closes_popup = !scope.is_empty() && (
            key == MenuKey::Escape
            || key == MenuKey::ArrowLeft && matches!(lookup(&self.content, scope), Some(MenuElement::Submenu(_)))
        );
        if closes_popup {
            self.close(scope);
            self.hovered = None;
            let trigger = scope.to_vec();
            self.focus_control(&trigger, host);
            return true;
        }
        let (paths, visible) = scope_controls(&self.content, scope, state);
        let current = paths.iter().position(|candidate| candidate == path);
        let orientation = if scope.is_empty() { Orientation::Horizontal } else { Orientation::Vertical };
        if let Some(index) = keyboard_move_focus(&visible, current, key, orientation) {
            if paths.len() > 1 {
                self.focus_control(&paths[index], host);
            }
            true
        } else {
            false
        }
    }

    /// Called after the host settles focus. A popup closes when focus
    /// moves outside of its children.
    pub fn focus_changed(&mut self, focus: MenuFocus) {
        match focus {
            MenuFocus::Control(target) => {
                self.open.retain(|scope| target.starts_with(scope) && target.len() > scope.len());
                self.tabstops.insert(scope_of(&target).to_vec(), target);
            }
            MenuFocus::Editor | MenuFocus::Other => self.open.clear(),
        }
    }

    /// Informs the menu about an external pointer click. Popups containing
    /// the pointer target stay open. Passing None represents a click outside.
    pub fn outside_click(&mut self, target: Option<&MenuPath>) {
        self.open.retain(|scope| target.is_some_and(|target| target.starts_with(scope)));
    }

    /// Submenus are also expanded by hovering over their trigger or contents.
    pub fn hover_changed(&mut self, target: Option<MenuPath>) {
        self.hovered = target;
    }

    pub fn update_layout(&mut self, geometry: FloatingGeometry) {
        self.layout.update(self.options, geometry);
    }

    pub(crate) fn available(&self, path: &[usize], state: &EditorState, focus: &MenuFocus) -> bool {
        if !lookup(&self.content, path).is_some_and(|item| item.visible(state)) {
            return false;
        }
        for depth in 2..path.len() {
            let parent = &path[..depth];
            let Some(menu) = lookup(&self.content, parent) else { return false };
            if !self.expanded(parent, control_kind(menu), focus) {
                return false;
            }
        }
        true
    }

    fn expanded(&self, path: &[usize], kind: MenuControlKind, focus: &MenuFocus) -> bool {
        self.open.iter().any(|menu| menu.as_slice() == path)
            || kind == MenuControlKind::Submenu && (
                self.hovered.as_ref().is_some_and(|hover| hover.starts_with(path))
                || matches!(focus, MenuFocus::Control(active) if active.len() > path.len() && active.starts_with(path))
            )
    }

    fn open(&mut self, path: &MenuPath, keyboard: bool, state: &EditorState, host: &mut dyn MenuHost) {
        if !self.open.contains(path) {
            self.open.push(path.clone());
        }
        if keyboard {
            self.move_in_scope(path, None, FocusDirection::Forwards, state, host);
        }
    }

    fn close(&mut self, path: &[usize]) {
        self.open.retain(|scope| !scope.starts_with(path));
    }

    fn move_in_scope(
        &mut self, scope: &[usize], current: Option<&MenuPath>, direction: FocusDirection,
        state: &EditorState, host: &mut dyn MenuHost,
    ) {
        let (paths, visible) = scope_controls(&self.content, scope, state);
        if paths.len() <= 1 {
            return;
        }
        let current = current.and_then(|current| paths.iter().position(|path| path == current));
        if let Some(index) = find_focusable_index(&visible, current, direction) {
            self.focus_control(&paths[index], host);
        }
    }

    fn focus_control(&mut self, path: &MenuPath, host: &mut dyn MenuHost) {
        self.tabstops.insert(scope_of(path).to_vec(), path.clone());
        host.focus_control(path);
    }
}

fn scope_of(path: &[usize]) -> &[usize] {
    if path.len() <= 2 { &[] } else { &path[..path.len() - 1] }
}

pub(crate) fn lookup<'a>(groups: &'a [Vec<MenuElement>], path: &[usize]) -> Option<&'a MenuElement> {
    let (&group, rest) = path.split_first()?;
    let (&index, nested) = rest.split_first()?;
    let mut item = groups.get(group)?.get(index)?;
    for &index in nested {
        item = item.children()?.get(index)?;
    }
    Some(item)
}

fn scope_controls(groups: &[Vec<MenuElement>], scope: &[usize], state: &EditorState) -> (Vec<MenuPath>, Vec<bool>) {
    if scope.is_empty() {
        groups.iter().enumerate().flat_map(|(group, children)| {
            children.iter().enumerate().map(move |(index, item)| (vec![group, index], item.visible(state)))
        }).unzip()
    } else if let Some(children) = lookup(groups, scope).and_then(MenuElement::children) {
        children.iter().enumerate().map(|(index, item)| (child_path(scope, index), item.visible(state))).unzip()
    } else {
        (Vec::new(), Vec::new())
    }
}

fn child_path(parent: &[usize], index: usize) -> MenuPath {
    let mut path = parent.to_vec();
    path.push(index);
    path
}

fn control_kind(item: &MenuElement) -> MenuControlKind {
    match item {
        MenuElement::Item(_) => MenuControlKind::Item,
        MenuElement::Dropdown(_) => MenuControlKind::Dropdown,
        MenuElement::Submenu(_) => MenuControlKind::Submenu,
    }
}

struct Projection<'a, E, F> {
    state: &'a EditorState,
    host: &'a dyn MenuHost,
    is_expanded: E,
    tabstop: F,
}

fn projection(
    groups: &[Vec<MenuElement>], state: &EditorState, host: &dyn MenuHost,
    is_expanded: impl Fn(&[usize], MenuControlKind, bool) -> bool,
    tabstop: impl Fn(&[usize]) -> Option<MenuPath>,
) -> Result<Vec<MenuGroup>, MenuError> {
    let projection = Projection { state, host, is_expanded, tabstop };
    let mut previous_visible = false;
    let mut rendered = Vec::new();
    for (group_index, group) in groups.iter().enumerate().filter(|(_, group)| !group.is_empty()) {
        let items = group.iter().enumerate().map(|(index, item)| projection.render(item, vec![group_index, index]))
            .collect::<Result<Vec<_>, _>>()?;
        let visible = items.iter().any(|item| item.state.visible);
        rendered.push(MenuGroup { items, visible, separator_before: previous_visible && visible });
        previous_visible = visible;
    }
    apply_tabstop(rendered.iter_mut().flat_map(|group| &mut group.items), (projection.tabstop)(&[]));
    Ok(rendered)
}

impl<E, F> Projection<'_, E, F>
where
    E: Fn(&[usize], MenuControlKind, bool) -> bool,
    F: Fn(&[usize]) -> Option<MenuPath>,
{
    fn render(&self, item: &MenuElement, path: MenuPath) -> Result<MenuControl, MenuError> {
        let kind = control_kind(item);
        if let MenuElement::Item(item) = item {
            let state = item.state(self.state);
            return Ok(MenuControl {
                path, kind, state, tab_index: -1,
                display: item.display(self.state, self.host)?,
                title: item.spec.title.as_ref().map(|title| title.text(self.state, self.host)),
                class: item.spec.class.clone(),
                css: item.spec.css.clone(),
                aria_disabled: item.spec.enable.is_some().then_some(!state.enabled),
                aria_pressed: item.spec.active.is_some().then_some(state.active),
                aria_expanded: None,
                children: Vec::new(),
            });
        }
        let mut children = item.children().expect("dropdown and submenu contain controls").iter().enumerate()
            .map(|(index, item)| self.render(item, child_path(&path, index))).collect::<Result<Vec<_>, _>>()?;
        apply_tabstop(children.iter_mut(), (self.tabstop)(&path));
        let visible = children.iter().any(|child| child.state.visible);
        let expanded = (self.is_expanded)(&path, kind, visible);
        let (label, title, class, css) = match item {
            MenuElement::Dropdown(dropdown) => (
                dropdown.options.label.as_deref().unwrap_or_default(),
                dropdown.options.title.as_ref().map(|title| self.host.translate(title)),
                dropdown.options.class.clone(),
                dropdown.options.css.clone(),
            ),
            MenuElement::Submenu(submenu) => (submenu.label.as_str(), None, None, None),
            MenuElement::Item(_) => unreachable!(),
        };
        Ok(MenuControl {
            path, kind, tab_index: -1, title, class, css, children,
            state: MenuItemState { visible, enabled: visible, active: false },
            display: MenuDisplay::Label(self.host.translate(label)),
            aria_disabled: None,
            aria_pressed: None,
            aria_expanded: Some(expanded),
        })
    }
}

fn apply_tabstop<'a>(controls: impl Iterator<Item = &'a mut MenuControl>, preferred: Option<MenuPath>) {
    let mut first = None;
    for control in controls.filter(|control| control.state.visible) {
        if preferred.as_ref() == Some(&control.path) {
            control.tab_index = 0;
            return;
        }
        if first.is_none() {
            first = Some(control);
        }
    }
    if let Some(control) = first {
        control.tab_index = 0;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Rect {
    pub fn width(self) -> f64 { self.right - self.left }
    pub fn height(self) -> f64 { self.bottom - self.top }
}

/// The measured host geometry required by the floating toolbar.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FloatingGeometry {
    pub wrapper: Rect,
    pub toolbar: Rect,
    pub wrapper_offset_width: f64,
    pub wrapper_client_width: f64,
    pub toolbar_offset_height: f64,
    pub viewport_height: f64,
    pub scroll_ancestor_top: Option<f64>,
    pub ios: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatingMenu {
    pub left: f64,
    pub top: Option<f64>,
    pub width: f64,
    pub spacer_height: f64,
    pub visible: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuBarLayout {
    pub floating: Option<FloatingMenu>,
    pub max_height: f64,
    measured_width: Option<f64>,
}

impl MenuBarLayout {
    pub fn update(&mut self, options: MenuBarOptions, geometry: FloatingGeometry) {
        let top = geometry.scroll_ancestor_top.map(|top| top.max(0.0));
        let viewport_top = top.unwrap_or_default();
        let should_float = options.floating && !geometry.ios
            && geometry.wrapper.top < viewport_top
            && geometry.wrapper.bottom >= geometry.toolbar_offset_height + 10.0;

        if should_float {
            if let Some(floating) = &mut self.floating {
                let border = (geometry.wrapper_offset_width - geometry.wrapper_client_width) / 2.0;
                floating.left = geometry.wrapper.left + border;
                floating.top = top.or(floating.top);
                floating.visible = geometry.wrapper.top <= geometry.viewport_height;
            } else {
                self.floating = Some(FloatingMenu {
                    left: geometry.toolbar.left,
                    top,
                    width: geometry.toolbar.width(),
                    spacer_height: geometry.toolbar.height(),
                    visible: true,
                });
            }
        } else {
            self.floating = None;
            let width = geometry.toolbar.width();
            if self.measured_width != Some(width) {
                self.max_height = 0.0;
                self.measured_width = Some(width);
            }
            self.max_height = self.max_height.max(geometry.toolbar_offset_height);
        }
    }

    /// Returns the scroll adjustment when the active end of the selection is
    /// obscured by the toolbar. The host applies it to the wrapping scroller.
    pub fn selection_scroll_delta(&self, toolbar: Rect, selection_rects: &[Rect], inverted: bool) -> Option<f64> {
        self.floating.filter(|floating| floating.visible)?;
        let selection = if inverted { selection_rects.first()? } else { selection_rects.last()? };
        (selection.top < toolbar.bottom && selection.bottom > toolbar.top)
            .then_some(selection.top - toolbar.bottom)
    }
}

/// The native anchor for a popup below its trigger or beside a submenu.
pub fn popup_origin(kind: MenuControlKind, trigger: Rect) -> Option<(f64, f64)> {
    match kind {
        MenuControlKind::Item => None,
        MenuControlKind::Dropdown => Some((trigger.left, trigger.bottom)),
        MenuControlKind::Submenu => Some((trigger.right, trigger.top - 3.0)),
    }
}
