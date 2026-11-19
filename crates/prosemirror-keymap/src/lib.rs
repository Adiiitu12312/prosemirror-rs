//! Keyboard binding and dispatch rules from prosemirror-keymap 1.2.3.

mod key_name;

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use prosemirror_state::{EditorState, Transaction};

pub type Dispatch<'a> = &'a mut dyn FnMut(Transaction);
pub type KeyCommand<V = ()> =
    Arc<dyn Fn(&EditorState, Dispatch<'_>, &V) -> bool + Send + Sync>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    MacOs,
    Ios,
    Windows,
    Other,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "ios") {
            Self::Ios
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Other
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Modifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub meta: bool,
    pub shift: bool,
}

impl Modifiers {
    fn apply(self, key: &str) -> String {
        let mut name = String::new();
        for (enabled, prefix) in [
            (self.shift, "Shift-"),
            (self.meta, "Meta-"),
            (self.ctrl, "Ctrl-"),
            (self.alt, "Alt-"),
        ] {
            if enabled {
                name.push_str(prefix);
            }
        }
        name.push_str(key);
        name
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KeyEvent<'a> {
    pub key: &'a str,
    pub key_code: Option<u16>,
    pub modifiers: Modifiers,
    /// Set by a browser adapter when the event uses the old Internet Explorer key format.
    pub legacy_ie: bool,
}

impl<'a> KeyEvent<'a> {
    pub fn new(key: &'a str) -> Self {
        Self { key, ..Self::default() }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeymapError {
    pub modifier: String,
}

impl fmt::Display for KeymapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unrecognized modifier name: {}", self.modifier)
    }
}

impl std::error::Error for KeymapError {}

pub fn normalize_key_name(name: &str, platform: Platform) -> Result<String, KeymapError> {
    let separator = name.char_indices().rev().find_map(|(index, ch)| {
        (ch == '-' && index + 1 < name.len()).then_some(index)
    });
    let (modifiers, key) = separator.map_or((None, name), |index| {
        (Some(&name[..index]), &name[index + 1..])
    });
    let mut flags = Modifiers::default();
    for part in modifiers.into_iter().flat_map(|modifiers| modifiers.split('-')) {
        match part.to_ascii_lowercase().as_str() {
            "cmd" | "meta" | "m" => flags.meta = true,
            "alt" | "a" => flags.alt = true,
            "ctrl" | "control" | "c" => flags.ctrl = true,
            "shift" | "s" => flags.shift = true,
            "mod" if matches!(platform, Platform::MacOs | Platform::Ios) => flags.meta = true,
            "mod" => flags.ctrl = true,
            _ => return Err(KeymapError { modifier: part.to_owned() }),
        }
    }
    Ok(flags.apply(if key == "Space" { " " } else { key }))
}

/// A keydown hook for an editor. `V` is the view or native integration passed to commands.
pub struct Keymap<V = ()> {
    platform: Platform,
    bindings: HashMap<String, KeyCommand<V>>,
}

pub fn keymap<V, K: AsRef<str>>(
    bindings: impl IntoIterator<Item = (K, KeyCommand<V>)>,
    platform: Platform,
) -> Result<Keymap<V>, KeymapError> {
    let mut normalized = HashMap::new();
    for (key, command) in bindings {
        normalized.insert(normalize_key_name(key.as_ref(), platform)?, command);
    }
    Ok(Keymap { platform, bindings: normalized })
}

pub fn keydown_handler<V, K: AsRef<str>>(
    bindings: impl IntoIterator<Item = (K, KeyCommand<V>)>,
    platform: Platform,
) -> Result<impl Fn(&EditorState, Dispatch<'_>, &V, KeyEvent<'_>) -> bool, KeymapError> {
    let map = keymap(bindings, platform)?;
    Ok(move |state, dispatch, view, event| map.handle_keydown(state, dispatch, view, event))
}

impl<V> Keymap<V> {
    pub fn handle_keydown(
        &self,
        state: &EditorState,
        dispatch: Dispatch<'_>,
        view: &V,
        event: KeyEvent<'_>,
    ) -> bool {
        let name = key_name::key_name(event, self.platform);
        let flags = event.modifiers;
        let mut run = |key: &str, flags: Modifiers| {
            self.bindings.get(&flags.apply(key)).is_some_and(|command| command(state, dispatch, view))
        };
        if run(name, flags) {
            return true;
        }
        if name == " " || name.encode_utf16().count() != 1 {
            return false;
        }
        if flags.shift && run(name, Modifiers { shift: false, ..flags }) {
            return true;
        }
        // Windows uses Ctrl+Alt for AltGr, which often produces a text character.
        if !(flags.alt || flags.meta || flags.ctrl)
            || self.platform == Platform::Windows && flags.ctrl && flags.alt
        {
            return false;
        }
        event.key_code.and_then(key_name::base_name)
            .is_some_and(|base| base != name && run(base, flags))
    }
}

/// Invoke handlers in editor order and stop at the first command that handles the event.
pub fn dispatch_keymaps<V>(
    maps: &[Keymap<V>],
    state: &EditorState,
    dispatch: Dispatch<'_>,
    view: &V,
    event: KeyEvent<'_>,
) -> bool {
    maps.iter().any(|map| map.handle_keydown(state, dispatch, view, event))
}
