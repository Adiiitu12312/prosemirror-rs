//! Adapt browser keyboard events to the upstream keymap matching rules.

use std::rc::Rc;

use prosemirror_view::browser::{BrowserView, BrowserViewProps, ViewEventHandler};
use web_sys::{KeyboardEvent, Navigator};

use crate::{KeyEvent, Keymap, Modifiers, Platform};

impl Platform {
    pub fn browser() -> Self {
        let Some(navigator) = web_sys::window().map(|window| window.navigator()) else {
            return Self::Other;
        };
        let platform = navigator.platform().unwrap_or_default();
        if platform.starts_with("Mac") {
            Self::MacOs
        } else if platform.starts_with("Win") {
            Self::Windows
        } else if ["iPhone", "iPad", "iPod"].contains(&platform.as_str()) {
            Self::Ios
        } else {
            Self::Other
        }
    }
}

impl Keymap<BrowserView> {
    pub fn into_browser_handler(self) -> ViewEventHandler<KeyboardEvent> {
        Rc::new(move |view, event| {
            let state = view.state();
            let key = event.key();
            let navigator = view.dom().owner_document().and_then(|doc| doc.default_view())
                .map(|window| window.navigator());
            let legacy_ie = navigator.as_ref().and_then(|navigator: &Navigator| navigator.user_agent().ok())
                .is_some_and(|agent| agent.contains("MSIE ") || agent.contains("Trident/"));
            self.handle_keydown(&state, &mut |tr| { let _ = view.dispatch(tr); }, view, KeyEvent {
                key: &key,
                key_code: u16::try_from(event.key_code()).ok().filter(|code| *code != 0),
                modifiers: Modifiers {
                    alt: event.alt_key(), ctrl: event.ctrl_key(),
                    meta: event.meta_key(), shift: event.shift_key(),
                },
                legacy_ie,
            })
        })
    }

    pub fn into_browser_plugin(self) -> BrowserViewProps {
        BrowserViewProps { handle_key_down: Some(self.into_browser_handler()), ..BrowserViewProps::default() }
    }
}
