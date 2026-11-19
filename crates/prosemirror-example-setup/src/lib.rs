//! Example editor input and command configuration from prosemirror-example-setup@1.2.3.

#[cfg(all(target_arch = "wasm32", feature = "browser"))]
pub mod browser;
pub mod inputrules;
pub mod keymap;
pub mod menu;

use prosemirror_dropcursor::DropCursor;
use prosemirror_gapcursor::{gap_cursor, GapCursorPlugin, GapCursorView};
use prosemirror_history::{history, HistoryOptions};
use prosemirror_keymap::{Keymap, KeymapError, Platform};
use prosemirror_menu::{menu_bar, MenuBar, MenuBarOptions, MenuElement};
use prosemirror_model::Schema;
use prosemirror_state::{Plugin, PluginKey};
use serde_json::json;

pub use inputrules::build_input_rules;
pub use keymap::{build_base_keymap, build_keymap, MapKeys};
pub use menu::{build_menu_items, MenuItems};

pub struct ExampleSetupOptions<'a> {
    pub schema: &'a Schema,
    pub map_keys: MapKeys,
    pub history: Option<HistoryOptions>,
    pub platform: Platform,
    pub gap_cursor_widget: u64,
    pub menu_bar: bool,
    pub floating_menu: bool,
    pub menu_content: Option<Vec<Vec<MenuElement>>>,
}

impl<'a> ExampleSetupOptions<'a> {
    pub fn new(schema: &'a Schema) -> Self {
        Self {
            schema,
            map_keys: MapKeys::new(),
            history: Some(HistoryOptions::default()),
            platform: default_platform(),
            gap_cursor_widget: 0,
            menu_bar: true,
            floating_menu: true,
            menu_content: None,
        }
    }
}

fn default_platform() -> Platform {
    #[cfg(all(target_arch = "wasm32", feature = "browser"))]
    return Platform::browser();
    #[cfg(not(all(target_arch = "wasm32", feature = "browser")))]
    Platform::current()
}

pub struct ExampleSetup<V> {
    pub plugins: Vec<Plugin>,
    pub keymaps: [Keymap<V>; 2],
    pub drop_cursor: DropCursor,
    pub gap_cursor: GapCursorPlugin<V>,
    pub menu_bar: Option<MenuBar>,
}

pub fn example_setup<V: GapCursorView>(options: ExampleSetupOptions<'_>) -> Result<ExampleSetup<V>, KeymapError> {
    let menu = options.menu_bar.then(|| {
        let content = options.menu_content.unwrap_or_else(|| build_menu_items(options.schema).full_menu);
        menu_bar(content, MenuBarOptions { floating: options.floating_menu, ..Default::default() })
    });
    let mut plugins = vec![build_input_rules(options.schema)];
    if let Some(options) = options.history {
        plugins.push(history(options));
    }
    plugins.push(Plugin::new(PluginKey::new("example-setup-style"), None)
        .with_prop("attributes", |_| json!({"class": "ProseMirror-example-setup-style"})));
    Ok(ExampleSetup {
        plugins,
        keymaps: [
            build_keymap(options.schema, &options.map_keys, options.platform)?,
            build_base_keymap(options.platform),
        ],
        drop_cursor: DropCursor::default(),
        gap_cursor: gap_cursor(options.platform, options.gap_cursor_widget),
        menu_bar: menu,
    })
}
