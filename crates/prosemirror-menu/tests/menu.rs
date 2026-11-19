use std::sync::Arc;

use prosemirror_history::{history, HistoryOptions};
use prosemirror_menu::{
    block_type_item, find_focusable_index, icons, keyboard_move_focus, menu_bar, popup_origin,
    redo_item, render_grouped, select_parent_node_item, undo_item, wrap_item, Dropdown,
    DropdownOptions, DropdownSubmenu, FloatingGeometry, FocusDirection, MenuBarOptions,
    MenuControlKind, MenuDisplay, MenuElement, MenuEvent, MenuFocus, MenuHost, MenuItem,
    MenuItemSpec, MenuKey, MenuPath, MenuPosition, MenuPrompt, MenuPromptField, MenuTitle,
    Orientation, Rect,
};
use prosemirror_state::{EditorState, EditorStateConfig, Plugin, Selection, Transaction};
use prosemirror_test_builder::test_schema;
use serde_json::{json, Value};

#[derive(Default)]
struct Host {
    focus: MenuFocus,
    transactions: Vec<Transaction>,
    prompts: Vec<MenuPrompt>,
    prompt_response: Option<Value>,
}

impl MenuHost for Host {
    fn dispatch(&mut self, tr: Transaction) {
        self.transactions.push(tr);
    }

    fn focus(&self) -> MenuFocus {
        self.focus.clone()
    }

    fn focus_editor(&mut self) {
        self.focus = MenuFocus::Editor;
    }

    fn focus_control(&mut self, path: &MenuPath) {
        self.focus = MenuFocus::Control(path.clone());
    }

    fn translate(&self, label: &str) -> String {
        format!("[{label}]")
    }

    fn prompt(&mut self, prompt: &MenuPrompt) -> Option<Value> {
        self.prompts.push(prompt.clone());
        self.prompt_response.take()
    }
}

fn editor(cursor: usize, plugins: Vec<Plugin>) -> EditorState {
    let schema = test_schema();
    let doc = schema.node_from_json(&json!({
        "type": "doc",
        "content": [{"type": "paragraph", "content": [{"type":"text", "text":"hello"}]}],
    })).unwrap();
    EditorState::create(EditorStateConfig {
        schema: schema.clone(), doc: Some(doc), selection: Some(Selection::cursor(cursor)),
        stored_marks: None, plugins,
    })
}

fn item(label: &str) -> MenuElement {
    MenuItem::new(MenuItemSpec {
        label: Some(label.to_owned()),
        ..MenuItemSpec::new(|_, _, _| true)
    }).into()
}

fn hidden_item() -> MenuElement {
    MenuItem::new(MenuItemSpec {
        label: Some("Hidden".to_owned()),
        select: Some(Arc::new(|_| false)),
        ..MenuItemSpec::default()
    }).into()
}

#[test]
fn update_surfaces_selection_enabled_active_and_translated_titles() {
    let menu = menu_bar(vec![vec![MenuItem::new(MenuItemSpec {
        label: Some("Edit".to_owned()),
        title: Some(MenuTitle::dynamic(|state| format!("Edit at {}", state.selection.from()))),
        select: Some(Arc::new(|state| state.selection.from() > 1)),
        enable: Some(Arc::new(|state| state.selection.from() > 2)),
        active: Some(Arc::new(|_| true)),
        ..MenuItemSpec::default()
    }).into()]], MenuBarOptions::default());
    let host = Host::default();
    let snapshots: Vec<_> = (1..=3).map(|pos| menu.snapshot(&editor(pos, Vec::new()), &host).unwrap()).collect();
    let controls: Vec<_> = snapshots.iter().map(|snapshot| &snapshot.groups[0].items[0]).collect();
    assert!(!controls[0].state.visible);
    assert_eq!(controls[1].aria_disabled, Some(true));
    assert_eq!(controls[1].aria_pressed, Some(false));
    assert_eq!(controls[2].aria_disabled, Some(false));
    assert_eq!(controls[2].aria_pressed, Some(true));
    assert_eq!(controls[2].display, MenuDisplay::Label("[Edit]".to_owned()));
    assert_eq!(controls[2].title.as_deref(), Some("[Edit at 3]"));
}

#[test]
fn disabled_items_do_not_dispatch_and_enabled_items_restore_editor_focus() {
    let mut menu = menu_bar(vec![vec![MenuItem::new(MenuItemSpec {
        label: Some("Insert".to_owned()),
        enable: Some(Arc::new(|state| state.selection.from() > 1)),
        ..MenuItemSpec::new(|state, host, _| {
            let mut transaction = state.tr();
            transaction.insert_text_at_selection("a");
            host.dispatch(transaction);
            true
        })
    }).into()]], MenuBarOptions::default());
    let path = vec![0, 0];
    let mut host = Host { focus: MenuFocus::Control(path.clone()), ..Host::default() };
    assert!(!menu.activate(&path, &editor(1, Vec::new()), &mut host, MenuEvent::default()));
    assert!(host.transactions.is_empty());
    assert_eq!(host.focus, MenuFocus::Control(path.clone()));
    let state = editor(2, Vec::new());
    assert!(menu.activate(&path, &state, &mut host, MenuEvent::keyboard(MenuKey::Enter)));
    assert_eq!(host.focus, MenuFocus::Editor);
    assert_eq!(state.apply(&host.transactions[0]).doc.text_content(), "haello");
}

#[test]
fn a_custom_item_can_request_host_input_and_cancel() {
    let item = MenuItem::new(MenuItemSpec {
        label: Some("Insert text".to_owned()),
        ..MenuItemSpec::new(|state, host, _| {
            let prompt = MenuPrompt {
                title: "Insert".to_owned(),
                fields: vec![MenuPromptField {
                    name: "text".to_owned(), label: "Text".to_owned(), value: String::new(), required: true,
                }],
            };
            let Some(input) = host.prompt(&prompt) else { return false };
            let mut tr = state.tr();
            tr.insert_text_at_selection(input["text"].as_str().unwrap());
            host.dispatch(tr);
            true
        })
    });
    let state = editor(1, Vec::new());
    let mut host = Host::default();
    assert!(!item.activate(&vec![0, 0], &state, &mut host, &MenuEvent::default()));
    assert!(host.transactions.is_empty());
    host.prompt_response = Some(json!({"text":"!"}));
    assert!(item.activate(&vec![0, 0], &state, &mut host, &MenuEvent::default()));
    assert_eq!(host.prompts.len(), 2);
    assert_eq!(state.apply(&host.transactions[0]).doc.text_content(), "!hello");
}

#[test]
fn grouped_controls_skip_empty_groups_and_suppress_hidden_separators() {
    let state = editor(1, Vec::new());
    let groups = render_grouped(
        &[
            vec![item("First")], Vec::new(), vec![item("Second")],
            vec![hidden_item()], vec![item("Third")],
        ],
        &state, &Host::default(),
    ).unwrap();
    assert_eq!(groups.iter().map(|group| group.separator_before).collect::<Vec<_>>(), [false, true, false, false]);
    assert_eq!(groups[1].items[0].path, vec![2, 0]);
    assert_eq!(groups[0].items[0].tab_index, 0);
    assert_eq!(groups[1].items[0].tab_index, -1);
}

#[test]
fn focus_navigation_wraps_and_skips_hidden_controls() {
    let visible = [true, false, true, false];
    assert_eq!(find_focusable_index(&visible, Some(2), FocusDirection::Forwards), Some(0));
    assert_eq!(find_focusable_index(&visible, Some(0), FocusDirection::Backwards), Some(2));
    assert_eq!(find_focusable_index(&[false, false], None, FocusDirection::Backwards), None);
    assert_eq!(find_focusable_index(&[], None, FocusDirection::Forwards), None);
    assert_eq!(keyboard_move_focus(&visible, Some(0), MenuKey::ArrowRight, Orientation::Vertical), None);
    assert_eq!(keyboard_move_focus(&visible, Some(2), MenuKey::Home, Orientation::Vertical), Some(0));
    assert_eq!(keyboard_move_focus(&visible, Some(0), MenuKey::End, Orientation::Horizontal), Some(2));
}

#[test]
fn toolbar_reassigns_focus_when_the_active_control_becomes_hidden() {
    let mut menu = menu_bar(vec![vec![
        MenuItem::new(MenuItemSpec {
            label: Some("First".to_owned()),
            select: Some(Arc::new(|state| state.selection.from() == 1)),
            ..MenuItemSpec::default()
        }).into(),
        item("Second"),
    ]], MenuBarOptions { position: MenuPosition::After, floating: false });
    let mut host = Host { focus: MenuFocus::Control(vec![0, 0]), ..Host::default() };
    let snapshot = menu.update(&editor(2, Vec::new()), &mut host).unwrap();
    assert_eq!(snapshot.position, MenuPosition::After);
    assert_eq!(host.focus, MenuFocus::Control(vec![0, 1]));
    assert_eq!(snapshot.groups[0].items[1].tab_index, 0);
}

#[test]
fn dropdown_and_nested_submenu_share_keyboard_focus_and_close_independently() {
    let submenu = DropdownSubmenu::new(vec![hidden_item(), item("Nested")], "More");
    let dropdown = Dropdown::new(vec![hidden_item(), submenu.into(), item("Last")], DropdownOptions {
        label: Some("Tools".to_owned()), ..DropdownOptions::default()
    });
    let mut menu = menu_bar(vec![vec![dropdown.into()]], MenuBarOptions::default());
    let state = editor(1, Vec::new());
    let mut host = Host::default();
    let trigger = vec![0, 0];
    let subtrigger = vec![0, 0, 1];
    let nested = vec![0, 0, 1, 1];
    assert!(!menu.activate(&nested, &state, &mut host, MenuEvent::default()));
    assert!(menu.handle_key(&trigger, MenuKey::Enter, &state, &mut host));
    assert_eq!(host.focus, MenuFocus::Control(subtrigger.clone()));
    assert!(menu.handle_key(&subtrigger, MenuKey::ArrowRight, &state, &mut host));
    assert_eq!(host.focus, MenuFocus::Control(nested.clone()));
    let opened = menu.snapshot(&state, &host).unwrap();
    let control = &opened.groups[0].items[0];
    assert_eq!(control.aria_expanded, Some(true));
    assert_eq!(control.children[1].aria_expanded, Some(true));
    assert!(menu.handle_key(&nested, MenuKey::ArrowLeft, &state, &mut host));
    assert_eq!(host.focus, MenuFocus::Control(subtrigger.clone()));
    assert!(menu.handle_key(&subtrigger, MenuKey::Escape, &state, &mut host));
    assert_eq!(host.focus, MenuFocus::Control(trigger));
    assert_eq!(menu.snapshot(&state, &host).unwrap().groups[0].items[0].aria_expanded, Some(false));
}

#[test]
fn pointer_hover_expands_submenu_and_an_outside_click_closes_dropdown() {
    let submenu = DropdownSubmenu::new(vec![item("Child")], "More");
    let dropdown = Dropdown::new(vec![submenu.into()], DropdownOptions::default());
    let mut menu = menu_bar(vec![vec![dropdown.into()]], MenuBarOptions::default());
    let state = editor(1, Vec::new());
    let mut host = Host { focus: MenuFocus::Editor, ..Host::default() };
    assert!(menu.activate(&vec![0, 0], &state, &mut host, MenuEvent::default()));
    assert_eq!(host.focus, MenuFocus::Editor);
    menu.hover_changed(Some(vec![0, 0, 0]));
    let hovered = menu.snapshot(&state, &host).unwrap();
    assert_eq!(hovered.groups[0].items[0].children[0].aria_expanded, Some(true));
    menu.hover_changed(None);
    menu.outside_click(None);
    let dismissed = menu.snapshot(&state, &host).unwrap();
    assert_eq!(dismissed.groups[0].items[0].aria_expanded, Some(false));
    assert_eq!(dismissed.groups[0].items[0].children[0].aria_expanded, Some(false));
}

#[test]
fn toolbar_floats_with_measured_scroll_geometry_and_frees_the_selection() {
    let mut menu = menu_bar(Vec::new(), MenuBarOptions { floating: true, ..MenuBarOptions::default() });
    let mut geometry = FloatingGeometry {
        wrapper: Rect { left: 20.0, top: -5.0, right: 360.0, bottom: 300.0 },
        toolbar: Rect { left: 21.0, top: -5.0, right: 350.0, bottom: 19.0 },
        wrapper_offset_width: 340.0,
        wrapper_client_width: 336.0,
        toolbar_offset_height: 24.0,
        viewport_height: 600.0,
        scroll_ancestor_top: Some(4.0), ios: false,
    };
    menu.update_layout(geometry);
    let first = menu.layout.floating.unwrap();
    assert_eq!((first.left, first.top, first.width, first.spacer_height), (21.0, Some(4.0), 329.0, 24.0));
    geometry.wrapper.left = 12.0;
    geometry.scroll_ancestor_top = Some(1.0);
    menu.update_layout(geometry);
    let fixed = menu.layout.floating.unwrap();
    assert_eq!((fixed.left, fixed.top), (14.0, Some(1.0)));
    let toolbar = Rect { top: 1.0, bottom: 25.0, ..Rect::default() };
    let rects = [Rect { top: 15.0, bottom: 35.0, ..Rect::default() }, Rect { top: 45.0, bottom: 65.0, ..Rect::default() }];
    assert_eq!(menu.layout.selection_scroll_delta(toolbar, &rects, true), Some(-10.0));
    assert_eq!(menu.layout.selection_scroll_delta(toolbar, &rects, false), None);
    geometry.ios = true;
    menu.update_layout(geometry);
    assert!(menu.layout.floating.is_none());
    assert_eq!(menu.layout.max_height, 24.0);
    assert_eq!(popup_origin(MenuControlKind::Dropdown, toolbar), Some((0.0, 25.0)));
    assert_eq!(popup_origin(MenuControlKind::Submenu, toolbar), Some((0.0, -2.0)));
}

#[test]
fn stock_items_dispatch_real_editor_commands_and_track_history() {
    let state = editor(2, vec![history(HistoryOptions::default())]);
    let mut host = Host::default();
    let path = vec![0, 0];
    let event = MenuEvent::default();
    let quote = wrap_item("blockquote", None, MenuItemSpec { label: Some("Quote".to_owned()), ..MenuItemSpec::default() });
    assert!(quote.state(&state).visible);
    assert!(quote.activate(&path, &state, &mut host, &event));
    let quoted = state.apply(&host.transactions.pop().unwrap());
    assert_eq!(quoted.doc.child(0).type_name(), "blockquote");
    assert!(select_parent_node_item().state(&quoted).visible);
    assert!(undo_item().state(&quoted).enabled);
    assert!(undo_item().activate(&path, &quoted, &mut host, &event));
    let undone = quoted.apply(&host.transactions.pop().unwrap());
    assert_eq!(undone.doc, state.doc);
    assert!(redo_item().state(&undone).enabled);
    assert!(redo_item().activate(&path, &undone, &mut host, &event));
    let redone = undone.apply(&host.transactions.pop().unwrap());
    assert_eq!(redone.doc, quoted.doc);
}

#[test]
fn stock_block_type_supports_attrs_and_explicit_availability() {
    let state = editor(1, Vec::new());
    let heading = block_type_item("heading", Some(json!({"level":2})), MenuItemSpec {
        label: Some("Heading 2".to_owned()),
        enable: Some(Arc::new(|_| true)),
        ..MenuItemSpec::default()
    });
    let mut host = Host::default();
    assert!(!heading.state(&state).active);
    assert!(heading.activate(&vec![0, 0], &state, &mut host, &MenuEvent::default()));
    let edited = state.apply(&host.transactions.pop().unwrap());
    assert_eq!(edited.doc.child(0).attrs().unwrap()["level"], 2);
    assert!(heading.state(&edited).active);
}

#[test]
fn stock_icon_contains_the_source_path_and_original_symbol_hash() {
    let glyphs = icons();
    assert_eq!(glyphs.len(), 12);
    let icon = &glyphs["join"];
    assert_eq!(icon.symbol_id().as_deref(), Some("pm-icon--3cb9c8"));
    let dom = icon.to_dom();
    assert_eq!(dom.as_element().unwrap().attr("class"), Some("ProseMirror-icon"));
    assert_eq!(dom.children()[0].as_element().unwrap().attr("viewBox"), Some("0 0 800 900"));
}
