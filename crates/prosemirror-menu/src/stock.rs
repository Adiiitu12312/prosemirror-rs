use std::sync::Arc;

use prosemirror_commands::{self as commands, Command};
use prosemirror_history as history;
use prosemirror_state::{EditorState, Selection};
use serde_json::Value;

use crate::{icons, MenuItem, MenuItemSpec};

pub fn join_up_item() -> MenuItem {
    selectable_command(commands::join_up, "join", "Join with above block")
}

pub fn lift_item() -> MenuItem {
    selectable_command(commands::lift, "lift", "Lift out of enclosing block")
}

pub fn select_parent_node_item() -> MenuItem {
    selectable_command(commands::select_parent_node, "selectParentNode", "Select parent node")
}

pub fn undo_item() -> MenuItem {
    MenuItem::new(MenuItemSpec {
        title: Some("Undo last change".into()),
        icon: icons().get("undo").cloned(),
        enable: Some(Arc::new(|state| history::undo_depth(state) != 0)),
        ..MenuItemSpec::new(|state, host, _| history::undo(state, |tr| host.dispatch(tr)))
    })
}

pub fn redo_item() -> MenuItem {
    MenuItem::new(MenuItemSpec {
        title: Some("Redo last undone change".into()),
        icon: icons().get("redo").cloned(),
        enable: Some(Arc::new(|state| history::redo_depth(state) != 0)),
        ..MenuItemSpec::new(|state, host, _| history::redo(state, |tr| host.dispatch(tr)))
    })
}

fn selectable_command(command: Command, icon: &str, title: &str) -> MenuItem {
    MenuItem::new(MenuItemSpec {
        title: Some(title.into()),
        icon: icons().get(icon).cloned(),
        select: Some(Arc::new(move |state| command(state, None))),
        ..MenuItemSpec::command(command)
    })
}

/// Builds a command for wrapping the selection in a node type.
pub fn wrap_item(type_name: impl Into<String>, attrs: Option<Value>, mut options: MenuItemSpec) -> MenuItem {
    let type_name = type_name.into();
    if options.select.is_none() {
        let type_name = type_name.clone();
        let attrs = attrs.clone();
        options.select = Some(Arc::new(move |state| commands::wrap_in(&type_name, attrs.clone())(state, None)));
    }
    if options.run.is_none() {
        options.run = MenuItemSpec::command(move |state, dispatch| {
            commands::wrap_in(&type_name, attrs.clone())(state, dispatch)
        }).run;
    }
    MenuItem::new(options)
}

/// Builds a command that changes the type of the selected textblock.
pub fn block_type_item(type_name: impl Into<String>, attrs: Option<Value>, mut options: MenuItemSpec) -> MenuItem {
    let type_name = type_name.into();
    if options.enable.is_none() {
        let type_name = type_name.clone();
        let attrs = attrs.clone();
        options.enable = Some(Arc::new(move |state| commands::set_block_type(&type_name, attrs.clone())(state, None)));
    }
    if options.active.is_none() {
        let type_name = type_name.clone();
        let attrs = attrs.clone();
        options.active = Some(Arc::new(move |state| selected_markup(&type_name, attrs.as_ref(), state)));
    }
    if options.run.is_none() {
        options.run = MenuItemSpec::command(move |state, dispatch| {
            commands::set_block_type(&type_name, attrs.clone())(state, dispatch)
        }).run;
    }
    MenuItem::new(options)
}

fn selected_markup(type_name: &str, attrs: Option<&Value>, state: &EditorState) -> bool {
    let Some(kind) = state.schema.node_type(type_name) else { return false };
    let Ok(markup) = kind.try_create(attrs.cloned(), None, None) else { return false };
    let Ok(from) = state.doc.resolve(state.selection.from()) else { return false };
    if matches!(state.selection, Selection::Node { .. }) {
        return from.node_after().is_some_and(|node| node.same_markup(&markup));
    }
    state.selection.to(&state.doc) <= from.end(from.depth) && from.parent().same_markup(&markup)
}
