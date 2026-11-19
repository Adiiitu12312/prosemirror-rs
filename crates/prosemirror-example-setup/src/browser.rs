//! Browser example setup from prosemirror-example-setup 1.2.3.

pub mod prompt;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use prosemirror_commands::toggle_mark;
use prosemirror_history::history;
use prosemirror_keymap::KeymapError;
use prosemirror_menu::{browser as menu, MenuElement, MenuItem};
use prosemirror_state::{Plugin, PluginKey};
use prosemirror_view::browser::{BrowserView, BrowserViewProps, PluginViewFactory};
use serde_json::{json, Value};
use wasm_bindgen::JsValue;

use crate::{build_base_keymap, build_input_rules, build_keymap, build_menu_items, ExampleSetupOptions};
use crate::menu::{image_prompt, link_prompt, mark_active};
use prompt::{open_menu_prompt, Prompt};

/// Styles for the example editor and its prompts.
pub const CSS: &str = include_str!("../style/style.css");

/// Core state plugins and the corresponding browser editor props.
pub struct BrowserExampleSetup {
    pub plugins: Vec<Plugin>,
    pub props: BrowserViewProps,
}

pub fn example_setup(options: ExampleSetupOptions<'_>) -> Result<BrowserExampleSetup, KeymapError> {
    let mut state_plugins = vec![build_input_rules(options.schema)];
    let mut view_plugins = vec![
        Rc::new(build_keymap(options.schema, &options.map_keys, options.platform)?.into_browser_plugin()),
        Rc::new(build_base_keymap(options.platform).into_browser_plugin()),
        Rc::new(prosemirror_dropcursor::browser_plugin(Default::default())),
        Rc::new(prosemirror_gapcursor::browser_plugin(options.platform, options.gap_cursor_widget)),
    ];
    if options.menu_bar {
        let items = build_menu_items(options.schema);
        let (link, image, content) = if let Some(content) = options.menu_content {
            (None, None, content)
        } else {
            (items.items.get("toggleLink").cloned(), items.items.get("insertImage").cloned(), items.full_menu)
        };
        view_plugins.push(Rc::new(BrowserViewProps {
            plugin_views: vec![example_menu(content, options.floating_menu, link, image)],
            ..BrowserViewProps::default()
        }));
    }
    if let Some(options) = options.history {
        state_plugins.push(history(options));
    }
    state_plugins.push(Plugin::new(PluginKey::new("example-setup-style"), None)
        .with_prop("attributes", |_| json!({"class": "ProseMirror-example-setup-style"})));
    Ok(BrowserExampleSetup {
        plugins: state_plugins,
        props: BrowserViewProps { plugins: view_plugins, ..BrowserViewProps::default() },
    })
}

fn example_menu(
    content: Vec<Vec<MenuElement>>, floating: bool, link: Option<MenuItem>, image: Option<MenuItem>,
) -> PluginViewFactory {
    Rc::new(move |view| {
        let prompt = RefCell::<Option<Prompt>>::new(None);
        let link = link.clone();
        let image = image.clone();
        let activate: menu::BrowserMenuActivation = Rc::new(move |view, _, item, _| {
            let state = view.state();
            let (fields, command) = if same_action(link.as_ref(), item) && !mark_active(&state, "link") {
                (link_prompt(), PromptCommand::Link)
            } else if same_action(image.as_ref(), item) {
                (image_prompt(&state), PromptCommand::Image)
            } else {
                return false;
            };
            let Some(document) = view.dom().owner_document() else { return false };
            prompt.borrow_mut().take();
            let weak = view.downgrade();
            match open_menu_prompt(&document, fields, move |attrs| {
                if let Some(view) = weak.upgrade().filter(|view| !view.is_destroyed()) {
                    if let Err(error) = command.run(&view, attrs) {
                        web_sys::console::error_1(&error);
                    }
                }
            }) {
                Ok(opened) => { *prompt.borrow_mut() = Some(opened); true }
                Err(error) => { web_sys::console::error_1(&error); false }
            }
        });
        menu::menu_bar(content.clone(), menu::BrowserMenuOptions {
            floating, activate: Some(activate), ..menu::BrowserMenuOptions::default()
        })(view)
    })
}

fn same_action(expected: Option<&MenuItem>, item: &MenuItem) -> bool {
    expected.and_then(|item| item.spec.run.as_ref()).zip(item.spec.run.as_ref())
        .is_some_and(|(a, b)| Arc::ptr_eq(a, b))
}

#[derive(Clone, Copy)]
enum PromptCommand {
    Link,
    Image,
}

impl PromptCommand {
    fn run(self, view: &BrowserView, attrs: Value) -> Result<(), JsValue> {
        let state = view.state();
        match self {
            Self::Link => {
                let mut event = Ok(false);
                toggle_mark("link", Some(attrs))(&state, Some(&mut |tr| event = view.dispatch(tr)));
                event?;
            }
            Self::Image => {
                if let Some(kind) = state.schema.node_type("image") {
                    if let Ok(Some(node)) = kind.create_and_fill(Some(attrs), None, None) {
                        let mut tr = state.tr();
                        tr.replace_selection_with(node, true);
                        view.dispatch(tr)?;
                    }
                }
            }
        }
        view.focus()
    }
}
