#![cfg(target_arch = "wasm32")]

use std::cell::Cell;
use std::rc::Rc;

use prosemirror_menu::browser::{menu_bar, BrowserMenuOptions, CSS};
use prosemirror_menu::{
    icons, Dropdown, DropdownOptions, DropdownSubmenu, MenuElement, MenuItem, MenuItemSpec,
    MenuPosition, MenuPrompt, MenuPromptField,
};
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_test_builder::{doc, p, test_schema};
use prosemirror_view::browser::{BrowserView, BrowserViewProps, Mount};
use serde_json::json;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;
use web_sys::{
    Document, Event, HtmlElement, KeyboardEvent, KeyboardEventInit, MouseEvent, MouseEventInit,
    ShadowRootInit, ShadowRootMode,
};

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> Document {
    web_sys::window().unwrap().document().unwrap()
}

fn element(tag: &str) -> HtmlElement {
    document().create_element(tag).unwrap().unchecked_into()
}

fn setup(content: Vec<Vec<MenuElement>>, options: BrowserMenuOptions) -> (BrowserView, HtmlElement, HtmlElement) {
    let space = element("div");
    let style = element("style");
    style.set_text_content(Some(CSS));
    space.append_child(&style).unwrap();
    let container = element("div");
    space.append_child(&container).unwrap();
    document().body().unwrap().append_child(&space).unwrap();
    let editor = element("div");
    container.append_child(&editor).unwrap();
    let view = BrowserView::new(Mount::Existing(editor), state(), BrowserViewProps {
        plugin_views: vec![menu_bar(content, options)],
        ..Default::default()
    }).unwrap();
    (view, space, container)
}

fn state() -> EditorState {
    EditorState::create(EditorStateConfig {
        schema: test_schema().clone(), doc: Some(doc!(p!("foo")).node),
        selection: Some(Selection::text(1, 1)), stored_marks: None, plugins: vec![],
    })
}

fn item(label: &str) -> MenuElement {
    MenuItem::new(MenuItemSpec { label: Some(label.into()), ..Default::default() }).into()
}

fn query(container: &HtmlElement, selector: &str) -> HtmlElement {
    container.query_selector(selector).unwrap().unwrap().unchecked_into()
}

fn buttons(container: &HtmlElement) -> Vec<HtmlElement> {
    let list = container.query_selector_all("button").unwrap();
    (0..list.length()).map(|index| list.item(index).unwrap().unchecked_into()).collect()
}

fn key(button: &HtmlElement, value: &str) -> bool {
    let init = KeyboardEventInit::new();
    init.set_key(value);
    init.set_bubbles(true);
    init.set_cancelable(true);
    let event = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap();
    !button.dispatch_event(&event).unwrap()
}

fn focused(button: &HtmlElement) -> bool {
    document().active_element().is_some_and(|active| button.is_same_node(Some(&active)))
}

#[wasm_bindgen_test]
fn inserts_before_or_after_and_restores_existing_mount_on_destroy() {
    for position in [MenuPosition::Before, MenuPosition::After] {
        let (view, space, container) = setup(vec![vec![item("Item")]], BrowserMenuOptions {
            position, ..Default::default()
        });
        let wrapper = query(&container, ".ProseMirror-menubar-wrapper");
        let toolbar = query(&container, "[role=toolbar]");
        let first = wrapper.first_child().unwrap();
        let editor = view.dom();
        match position {
            MenuPosition::Before => assert!(first.is_same_node(Some(&toolbar))),
            MenuPosition::After => assert!(first.is_same_node(Some(&editor))),
        }
        view.destroy();
        assert!(container.first_child().unwrap().is_same_node(Some(&editor)));
        assert!(container.query_selector("[role=toolbar]").unwrap().is_none());
        space.remove();
    }
}

#[wasm_bindgen_test]
fn updates_visibility_and_accessibility_after_dispatched_actions() {
    let action = MenuItem::new(MenuItemSpec {
        label: Some("Append".into()),
        enable: Some(std::sync::Arc::new(|state| state.doc.text_content() == "foo")),
        active: Some(std::sync::Arc::new(|_| true)),
        ..MenuItemSpec::new(|state, host, _| {
            let mut tr = state.tr();
            tr.insert_text("!", 4, 4);
            host.dispatch(tr);
            true
        })
    });
    let dynamic = MenuItem::new(MenuItemSpec {
        label: Some("After".into()), select: Some(std::sync::Arc::new(|state| state.doc.text_content().ends_with('!'))),
        ..Default::default()
    });
    let (view, space, container) = setup(vec![vec![action.into()], vec![dynamic.into()]], BrowserMenuOptions {
        translate: Some(Rc::new(|value| format!("[{value}]"))), ..Default::default()
    });
    let controls = buttons(&container);
    let separator = query(&container, "[role=separator]");
    assert_eq!(separator.style().get_property_value("display").unwrap(), "none");
    assert_eq!(controls[0].text_content().as_deref(), Some("[Append]"));
    assert_eq!(controls[0].get_attribute("aria-pressed").as_deref(), Some("true"));
    let down = MouseEventInit::new();
    down.set_bubbles(true);
    down.set_cancelable(true);
    let down = MouseEvent::new_with_mouse_event_init_dict("mousedown", &down).unwrap();
    assert!(!controls[0].dispatch_event(&down).unwrap());
    controls[0].focus().unwrap();
    controls[0].click();
    assert_eq!(view.state().doc.text_content(), "foo!");
    assert_eq!(controls[0].get_attribute("aria-disabled").as_deref(), Some("true"));
    assert_eq!(controls[0].get_attribute("aria-pressed").as_deref(), Some("false"));
    assert_eq!(controls[1].style().get_property_value("display").unwrap(), "");
    assert_eq!(separator.style().get_property_value("display").unwrap(), "");
    assert!(view.has_focus());
    controls[0].click();
    assert_eq!(view.state().doc.text_content(), "foo!");
    view.destroy();
    space.remove();
}

#[wasm_bindgen_test]
fn moves_toolbar_tab_stop_and_skips_hidden_controls() {
    let hidden = MenuItem::new(MenuItemSpec {
        label: Some("Hidden".into()), select: Some(std::sync::Arc::new(|_| false)),
        ..Default::default()
    });
    let (view, space, container) = setup(vec![vec![item("First"), hidden.into(), item("Last")]], Default::default());
    let controls = buttons(&container);
    assert_eq!((controls[0].tab_index(), controls[1].tab_index(), controls[2].tab_index()), (0, -1, -1));
    controls[0].focus().unwrap();
    assert!(key(&controls[0], "ArrowRight"));
    assert!(focused(&controls[2]));
    assert_eq!((controls[0].tab_index(), controls[2].tab_index()), (-1, 0));
    assert!(key(&controls[2], "Home"));
    assert!(focused(&controls[0]));
    assert!(key(&controls[0], "End"));
    assert!(focused(&controls[2]));
    view.destroy();
    space.remove();
}

#[wasm_bindgen_test]
fn opens_and_closes_nested_popups_and_restores_trigger_focus() {
    let submenu = DropdownSubmenu::new(vec![item("One"), item("Two")], "Nested");
    let dropdown = Dropdown::new(vec![submenu.into(), item("Sibling")], DropdownOptions {
        label: Some("Dropdown".into()), ..Default::default()
    });
    let (view, space, container) = setup(vec![vec![dropdown.into()]], Default::default());
    let trigger = query(&container, ".ProseMirror-menu-dropdown");
    assert_eq!(trigger.get_attribute("aria-expanded").as_deref(), Some("false"));
    trigger.focus().unwrap();
    assert!(key(&trigger, "Enter"));
    let nested = query(&container, ".ProseMirror-menu-submenu-label");
    assert!(focused(&nested));
    assert_eq!(trigger.get_attribute("aria-expanded").as_deref(), Some("true"));
    assert!(key(&nested, "ArrowRight"));
    let submenu = query(&container, ".ProseMirror-menu-submenu");
    let submenu_buttons = buttons(&submenu);
    assert!(focused(&submenu_buttons[0]));
    assert!(key(&submenu_buttons[0], "ArrowDown"));
    assert!(focused(&submenu_buttons[1]));
    assert!(key(&submenu_buttons[1], "ArrowLeft"));
    assert!(focused(&nested));
    assert!(key(&nested, "Escape"));
    assert!(focused(&trigger));
    assert!(container.query_selector("[role=menu]").unwrap().is_none());
    trigger.click();
    assert_eq!(trigger.get_attribute("aria-expanded").as_deref(), Some("true"));
    space.click();
    assert_eq!(trigger.get_attribute("aria-expanded").as_deref(), Some("false"));
    view.destroy();
    space.remove();
}

#[wasm_bindgen_test]
fn caches_svg_symbols_inside_the_editors_shadow_root() {
    let space = element("div");
    document().body().unwrap().append_child(&space).unwrap();
    let shadow = space.attach_shadow(&ShadowRootInit::new(ShadowRootMode::Open)).unwrap();
    let container = element("div");
    shadow.append_child(&container).unwrap();
    let icon = icons()["strong"].clone();
    let icon_item = || MenuItem::new(MenuItemSpec { icon: Some(icon.clone()), ..Default::default() }).into();
    let view = BrowserView::new(Mount::Append(container.clone().into()), state(), BrowserViewProps {
        plugin_views: vec![menu_bar(vec![vec![icon_item(), icon_item()]], Default::default())],
        ..Default::default()
    }).unwrap();
    assert_eq!(container.query_selector_all("button svg use").unwrap().length(), 2);
    let collection = shadow.query_selector("#ProseMirror-icon-collection").unwrap().unwrap();
    assert_eq!(collection.query_selector_all("symbol").unwrap().length(), 1);
    let symbol = collection.first_child().unwrap().unchecked_into::<web_sys::Element>();
    assert_eq!(symbol.id(), icon.symbol_id().unwrap());
    view.destroy();
    space.remove();
}

#[wasm_bindgen_test]
fn intercepts_a_browser_action_before_the_native_command() {
    let handled = Rc::new(Cell::new(false));
    let activated = handled.clone();
    let action = MenuItem::new(MenuItemSpec {
        label: Some("Action".into()),
        ..MenuItemSpec::new(|_, _, _| panic!("the intercepted native command ran"))
    });
    let (view, space, container) = setup(vec![vec![action.into()]], BrowserMenuOptions {
        activate: Some(Rc::new(move |view, path, item, event| {
            assert_eq!(path, &[0, 0]);
            assert_eq!(item.spec.label.as_deref(), Some("Action"));
            assert_eq!(event.key, Some(prosemirror_menu::MenuKey::Enter));
            let mut tr = view.state().tr();
            tr.insert_text_at_selection("!");
            view.dispatch(tr).unwrap();
            activated.set(true);
            true
        })),
        ..Default::default()
    });
    let button = query(&container, "button");
    button.focus().unwrap();
    assert!(key(&button, "Enter"));
    assert!(handled.get());
    assert_eq!(view.state().doc.text_content(), "!foo");
    assert!(view.has_focus());
    view.destroy();
    space.remove();
}

#[wasm_bindgen_test]
fn passes_a_host_prompt_result_to_the_native_action() {
    let action = MenuItem::new(MenuItemSpec {
        label: Some("Prompt".into()),
        ..MenuItemSpec::new(|state, host, _| {
            if let Some(answer) = host.prompt(&MenuPrompt {
                title: "Title".into(), fields: vec![MenuPromptField {
                    name: "text".into(), label: "Text".into(), value: "initial".into(), required: false,
                }],
            }) {
                let mut tr = state.tr();
                tr.insert_text_at_selection(answer["text"].as_str().unwrap());
                host.dispatch(tr);
            }
            true
        })
    });
    let (view, space, container) = setup(vec![vec![action.into()]], BrowserMenuOptions {
        prompt: Some(Rc::new(|_, prompt| {
            assert_eq!(prompt.fields[0].value, "initial");
            Some(json!({"text":"answer "}))
        })),
        ..Default::default()
    });
    query(&container, "button").click();
    assert_eq!(view.state().doc.text_content(), "answer foo");
    view.destroy();
    space.remove();
}

#[wasm_bindgen_test]
fn floats_on_a_wrapping_scroller_and_removes_its_spacer() {
    let (view, space, container) = setup(vec![vec![item("Item")]], BrowserMenuOptions {
        floating: true, ..Default::default()
    });
    container.style().set_css_text("height: 100px; overflow: auto");
    view.dom().style().set_property("height", "250px").unwrap();
    let toolbar = query(&container, "[role=toolbar]");
    let scroll = Event::new("scroll").unwrap();
    container.set_scroll_top(60);
    container.dispatch_event(&scroll).unwrap();
    assert_eq!(toolbar.style().get_property_value("position").unwrap(), "fixed");
    assert!(container.query_selector(".ProseMirror-menubar-spacer").unwrap().is_some());
    container.set_scroll_top(0);
    container.dispatch_event(&scroll).unwrap();
    assert_eq!(toolbar.style().get_property_value("position").unwrap(), "");
    assert!(container.query_selector(".ProseMirror-menubar-spacer").unwrap().is_none());
    view.destroy();
    space.remove();
}
