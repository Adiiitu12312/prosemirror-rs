//! Port of prosemirror-example-setup@1.2.3/src/keymap.ts.

use std::collections::HashMap;
use std::sync::Arc;

use prosemirror_commands::{self as commands, DispatchFn};
use prosemirror_history::{redo, undo};
use prosemirror_inputrules::undo_input_rule;
use prosemirror_keymap::{keymap, Dispatch, KeyCommand, Keymap, KeymapError, Platform};
use prosemirror_model::Schema;
use prosemirror_schema_list::{lift_list_item, sink_list_item, split_list_item, wrap_in_list};
use prosemirror_state::EditorState;
use serde_json::json;

pub type MapKeys = HashMap<String, Option<String>>;

pub fn build_keymap<V>(
    schema: &Schema, map_keys: &MapKeys, platform: Platform,
) -> Result<Keymap<V>, KeymapError> {
    let mac = matches!(platform, Platform::MacOs | Platform::Ios);
    let mut keys = Vec::new();
    let mut bind = |name: &str, command: KeyCommand<V>| {
        match map_keys.get(name) {
            Some(Some(mapped)) if !mapped.is_empty() => keys.push((mapped.clone(), command)),
            Some(None) => {}
            _ => keys.push((name.to_owned(), command)),
        }
    };
    bind("Mod-z", direct(|state, dispatch, _| undo(state, dispatch)));
    let redo = direct(|state, dispatch, _| redo(state, dispatch));
    bind("Shift-Mod-z", Arc::clone(&redo));
    bind("Backspace", direct(|state, dispatch, _| undo_input_rule(state, dispatch)));
    if !mac {
        bind("Mod-y", redo);
    }
    bind("Alt-ArrowUp", adapt(commands::join_up));
    bind("Alt-ArrowDown", adapt(commands::join_down));
    bind("Mod-BracketLeft", adapt(commands::lift));
    bind("Escape", adapt(commands::select_parent_node));
    if schema.mark_type("strong").is_some() {
        let strong = adapt(commands::toggle_mark("strong", None));
        bind("Mod-b", Arc::clone(&strong));
        bind("Mod-B", strong);
    }
    if schema.mark_type("em").is_some() {
        let em = adapt(commands::toggle_mark("em", None));
        bind("Mod-i", Arc::clone(&em));
        bind("Mod-I", em);
    }
    if schema.mark_type("code").is_some() {
        bind("Mod-\u{60}", adapt(commands::toggle_mark("code", None)));
    }
    for (name, key) in [("bullet_list", "Shift-Ctrl-8"), ("ordered_list", "Shift-Ctrl-9")] {
        if schema.node_type(name).is_some() {
            bind(key, adapt(wrap_in_list(name, None)));
        }
    }
    if schema.node_type("blockquote").is_some() {
        bind("Ctrl->", adapt(commands::wrap_in("blockquote", None)));
    }
    if schema.node_type("hard_break").is_some() {
        let line_break = direct(|state: &EditorState, dispatch: Dispatch<'_>, _| {
            commands::exit_code(state, Some(&mut *dispatch)) || insert_node(state, dispatch, "hard_break")
        });
        bind("Mod-Enter", Arc::clone(&line_break));
        bind("Shift-Enter", Arc::clone(&line_break));
        if mac {
            bind("Ctrl-Enter", line_break);
        }
    }
    if schema.node_type("list_item").is_some() {
        bind("Enter", adapt(split_list_item("list_item", None)));
        bind("Mod-[", adapt(lift_list_item("list_item")));
        bind("Mod-]", adapt(sink_list_item("list_item")));
    }
    if schema.node_type("paragraph").is_some() {
        bind("Shift-Ctrl-0", adapt(commands::set_block_type("paragraph", None)));
    }
    if schema.node_type("code_block").is_some() {
        bind("Shift-Ctrl-\\", adapt(commands::set_block_type("code_block", None)));
    }
    if schema.node_type("heading").is_some() {
        for level in 1..=6 {
            let attrs = Some(json!({"level": level}));
            bind(&format!("Shift-Ctrl-{level}"), adapt(commands::set_block_type("heading", attrs)));
        }
    }
    if schema.node_type("horizontal_rule").is_some() {
        bind("Mod-_", direct(|state, dispatch, _| insert_node(state, dispatch, "horizontal_rule")));
    }
    keymap(keys, platform)
}

pub fn build_base_keymap<V>(platform: Platform) -> Keymap<V> {
    let mac = matches!(platform, Platform::MacOs | Platform::Ios);
    let mut keys = vec!["Enter", "Mod-Enter", "Backspace", "Mod-Backspace", "Shift-Backspace", "Delete", "Mod-Delete", "Mod-a"];
    if mac {
        keys.extend(["Ctrl-h", "Alt-Backspace", "Ctrl-d", "Ctrl-Alt-Backspace", "Alt-Delete", "Alt-d", "Ctrl-a", "Ctrl-e"]);
    }
    keymap(keys.into_iter().map(|key| {
        let command = if mac { commands::mac_base_keymap(key) } else { commands::pc_base_keymap(key) };
        (key, adapt(command.expect("known base binding")))
    }), platform).expect("valid base bindings")
}

fn adapt<V>(run: impl for<'a> Fn(&EditorState, DispatchFn<'a>) -> bool + Send + Sync + 'static) -> KeyCommand<V> {
    direct(move |state: &EditorState, dispatch: Dispatch<'_>, _: &V| run(state, Some(dispatch)))
}

fn direct<V>(run: impl Fn(&EditorState, Dispatch<'_>, &V) -> bool + Send + Sync + 'static) -> KeyCommand<V> {
    Arc::new(run)
}

fn insert_node(state: &EditorState, dispatch: Dispatch<'_>, name: &str) -> bool {
    let Some(kind) = state.schema.node_type(name) else { return false };
    let mut tr = state.tr();
    tr.replace_selection_with(kind.create(None, None, None), true).scroll_into_view();
    dispatch(tr);
    true
}
