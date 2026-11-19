//! Port of prosemirror-example-setup@1.2.3/src/menu.ts.

use std::collections::HashMap;
use std::sync::Arc;

use prosemirror_commands::toggle_mark;
use prosemirror_menu::{
    block_type_item, icons, join_up_item, lift_item, redo_item, select_parent_node_item, undo_item,
    wrap_item, Dropdown, DropdownOptions, DropdownSubmenu, MenuElement, MenuItem, MenuItemSpec,
    MenuPrompt, MenuPromptField,
};
use prosemirror_model::Schema;
use prosemirror_schema_list::wrap_in_list;
use prosemirror_state::{EditorState, Selection};
use serde_json::{json, Value};

pub struct MenuItems {
    pub items: HashMap<String, MenuItem>,
    pub insert_menu: Dropdown,
    pub type_menu: Dropdown,
    pub inline_menu: Vec<Vec<MenuElement>>,
    pub block_menu: Vec<Vec<MenuElement>>,
    pub full_menu: Vec<Vec<MenuElement>>,
}

pub fn build_menu_items(schema: &Schema) -> MenuItems {
    let mut items = HashMap::new();
    for (name, mark, title) in [
        ("toggleStrong", "strong", "Toggle strong style"),
        ("toggleEm", "em", "Toggle emphasis"),
        ("toggleCode", "code", "Toggle code font"),
    ] {
        if schema.mark_type(mark).is_some() {
            items.insert(name.to_owned(), mark_item(mark, title));
        }
    }
    if schema.mark_type("link").is_some() {
        items.insert("toggleLink".to_owned(), link_item());
    }
    if schema.node_type("image").is_some() {
        items.insert("insertImage".to_owned(), image_item());
    }
    for (node, name, icon, title) in [
        ("bullet_list", "wrapBulletList", "bulletList", "Wrap in bullet list"),
        ("ordered_list", "wrapOrderedList", "orderedList", "Wrap in ordered list"),
    ] {
        if schema.node_type(node).is_some() {
            items.insert(name.to_owned(), MenuItem::new(MenuItemSpec {
                title: Some(title.into()),
                icon: icons().get(icon).cloned(),
                select: Some(Arc::new(move |state| wrap_in_list(node, None)(state, None))),
                ..MenuItemSpec::command(wrap_in_list(node, None))
            }));
        }
    }
    if schema.node_type("blockquote").is_some() {
        items.insert("wrapBlockQuote".to_owned(), wrap_item("blockquote", None, MenuItemSpec {
            title: Some("Wrap in block quote".into()), icon: icons().get("blockquote").cloned(),
            ..Default::default()
        }));
    }
    for (node, name, label, title) in [
        ("paragraph", "makeParagraph", "Plain", "Change to paragraph"),
        ("code_block", "makeCodeBlock", "Code", "Change to code block"),
    ] {
        if schema.node_type(node).is_some() {
            items.insert(name.to_owned(), block_type_item(node, None, MenuItemSpec {
                title: Some(title.into()), label: Some(label.into()), ..Default::default()
            }));
        }
    }
    if schema.node_type("heading").is_some() {
        for level in 1..=10 {
            items.insert(format!("makeHead{level}"), block_type_item(
                "heading", Some(json!({"level":level})),
                MenuItemSpec {
                    title: Some(format!("Change to heading {level}").into()),
                    label: Some(format!("Level {level}")), ..Default::default()
                },
            ));
        }
    }
    if schema.node_type("horizontal_rule").is_some() {
        items.insert("insertHorizontalRule".to_owned(), horizontal_rule_item());
    }

    let item_set = |keys: &[&str]| -> Vec<MenuElement> {
        keys.iter().filter_map(|key| items.get(*key).cloned().map(MenuElement::from)).collect()
    };
    let insert_menu = Dropdown::new(item_set(&["insertImage", "insertHorizontalRule"]), label("Insert"));
    let mut types = item_set(&["makeParagraph", "makeCodeBlock"]);
    if items.contains_key("makeHead1") {
        let headings = item_set(&["makeHead1", "makeHead2", "makeHead3", "makeHead4", "makeHead5", "makeHead6"]);
        types.push(DropdownSubmenu::new(headings, "Heading").into());
    }
    let type_menu = Dropdown::new(types, label("Type..."));
    let inline_menu = vec![item_set(&["toggleStrong", "toggleEm", "toggleCode", "toggleLink"])];
    let mut blocks = item_set(&["wrapBulletList", "wrapOrderedList", "wrapBlockQuote"]);
    blocks.extend([join_up_item().into(), lift_item().into(), select_parent_node_item().into()]);
    let block_menu = vec![blocks];
    let mut full_menu = inline_menu.clone();
    full_menu.push(vec![insert_menu.clone().into(), type_menu.clone().into()]);
    full_menu.push(vec![undo_item().into(), redo_item().into()]);
    full_menu.extend(block_menu.clone());
    MenuItems { items, insert_menu, type_menu, inline_menu, block_menu, full_menu }
}

fn label(label: &str) -> DropdownOptions {
    DropdownOptions { label: Some(label.to_owned()), ..Default::default() }
}

fn mark_item(mark: &'static str, title: &str) -> MenuItem {
    MenuItem::new(MenuItemSpec {
        title: Some(title.into()),
        icon: icons().get(mark).cloned(),
        select: Some(Arc::new(move |state| toggle_mark(mark, None)(state, None))),
        active: Some(Arc::new(move |state| mark_active(state, mark))),
        ..MenuItemSpec::command(toggle_mark(mark, None))
    })
}

fn mark_active(state: &EditorState, mark: &str) -> bool {
    if state.selection.empty_in(&state.doc) {
        state.stored_marks.clone().or_else(|| state.doc.resolve(state.selection.from()).ok().map(|pos| pos.marks()))
            .is_some_and(|marks| marks.iter().any(|value| value.type_name == mark))
    } else {
        state.doc.range_has_mark(state.selection.from(), state.selection.to(&state.doc), mark)
    }
}

fn link_item() -> MenuItem {
    MenuItem::new(MenuItemSpec {
        title: Some("Add or remove link".into()),
        icon: icons().get("link").cloned(),
        active: Some(Arc::new(|state| mark_active(state, "link"))),
        enable: Some(Arc::new(|state| !state.selection.empty_in(&state.doc))),
        ..MenuItemSpec::new(|state, host, _| {
            if mark_active(state, "link") {
                return toggle_mark("link", None)(state, Some(&mut |tr| host.dispatch(tr)));
            }
            let prompt = MenuPrompt {
                title: "Create a link".into(),
                fields: vec![field("href", "Link target", "", true), field("title", "Title", "", false)],
            };
            if let Some(attrs) = host.prompt(&prompt).filter(|value| required_text(value, "href")) {
                toggle_mark("link", Some(attrs))(state, Some(&mut |tr| host.dispatch(tr)));
                host.focus_editor();
            }
            true
        })
    })
}

fn image_item() -> MenuItem {
    MenuItem::new(MenuItemSpec {
        title: Some("Insert image".into()),
        label: Some("Image".into()),
        enable: Some(Arc::new(|state| can_insert(state, "image"))),
        ..MenuItemSpec::new(|state, host, _| {
            let Some(kind) = state.schema.node_type("image") else { return false };
            let from = state.selection.from();
            let to = state.selection.to(&state.doc);
            let attrs = if matches!(state.selection, Selection::Node { .. }) {
                state.doc.node_at(from).filter(|node| node.type_name() == "image").and_then(|node| node.attrs())
            } else {
                None
            };
            let attr = |name: &str| attrs.and_then(|attrs| attrs[name].as_str()).unwrap_or_default();
            let description = if attrs.is_some() {
                attr("alt").to_owned()
            } else {
                state.doc.text_between_with(from, to, Some(" "), None)
            };
            let prompt = MenuPrompt {
                title: "Insert image".into(),
                fields: vec![
                    field("src", "Location", attr("src"), true),
                    field("title", "Title", attr("title"), false),
                    field("alt", "Description", &description, false),
                ],
            };
            if let Some(attrs) = host.prompt(&prompt).filter(|value| required_text(value, "src")) {
                if let Ok(Some(node)) = kind.create_and_fill(Some(attrs), None, None) {
                    let mut tr = state.tr();
                    tr.replace_selection_with(node, true);
                    host.dispatch(tr);
                    host.focus_editor();
                }
            }
            true
        })
    })
}

fn horizontal_rule_item() -> MenuItem {
    MenuItem::new(MenuItemSpec {
        title: Some("Insert horizontal rule".into()),
        label: Some("Horizontal rule".into()),
        enable: Some(Arc::new(|state| can_insert(state, "horizontal_rule"))),
        ..MenuItemSpec::new(|state, host, _| {
            let Some(kind) = state.schema.node_type("horizontal_rule") else { return false };
            let mut tr = state.tr();
            tr.replace_selection_with(kind.create(None, None, None), true);
            host.dispatch(tr);
            true
        })
    })
}

fn can_insert(state: &EditorState, kind: &str) -> bool {
    state.doc.resolve(state.selection.from()).is_ok_and(|from| {
        (0..=from.depth).rev().any(|depth| {
            let index = from.index(depth);
            from.node(depth).can_replace_with(index, index, kind, &[])
        })
    })
}

fn field(name: &str, label: &str, value: &str, required: bool) -> MenuPromptField {
    MenuPromptField { name: name.into(), label: label.into(), value: value.into(), required }
}

fn required_text(value: &Value, name: &str) -> bool {
    value[name].as_str().is_some_and(|text| !text.trim().is_empty())
}
