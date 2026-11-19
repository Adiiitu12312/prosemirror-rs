//! Key name compatibility with w3c-keyname 2.2.8.
//! Copyright (C) 2016 by Marijn Haverbeke and others.

use crate::{KeyEvent, Platform};

fn ascii_at(source: &'static str, digit: u16) -> &'static str {
    let index = usize::from(digit);
    &source[index..index + 1]
}

pub(crate) fn base_name(code: u16) -> Option<&'static str> {
    const FUNCTION_KEYS: [&str; 24] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
        "F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
    ];
    Some(match code {
        8 => "Backspace",
        9 => "Tab",
        10 | 13 => "Enter",
        12 | 144 => "NumLock",
        16 | 160 | 161 => "Shift",
        17 | 162 | 163 => "Control",
        18 | 164 | 165 => "Alt",
        20 => "CapsLock",
        27 => "Escape",
        32 => " ",
        33 => "PageUp",
        34 => "PageDown",
        35 => "End",
        36 => "Home",
        37 => "ArrowLeft",
        38 => "ArrowUp",
        39 => "ArrowRight",
        40 => "ArrowDown",
        44 => "PrintScreen",
        45 => "Insert",
        46 => "Delete",
        48..=57 => ascii_at("0123456789", code - 48),
        59 | 186 => ";",
        61 | 187 => "=",
        65..=90 => ascii_at("abcdefghijklmnopqrstuvwxyz", code - 65),
        91 | 92 => "Meta",
        96..=105 => ascii_at("0123456789", code - 96),
        106 => "*",
        107 => "+",
        108 | 188 => ",",
        109 | 173 | 189 => "-",
        110 | 190 => ".",
        111 | 191 => "/",
        112..=135 => FUNCTION_KEYS[usize::from(code - 112)],
        145 => "ScrollLock",
        192 => "`",
        219 => "[",
        220 => "\\",
        221 => "]",
        222 => "'",
        _ => return None,
    })
}

fn shifted_name(code: u16) -> Option<&'static str> {
    Some(match code {
        48..=57 => ascii_at(")!@#$%^&*(", code - 48),
        59 | 186 => ":",
        61 | 187 => "+",
        65..=90 => ascii_at("ABCDEFGHIJKLMNOPQRSTUVWXYZ", code - 65),
        173 | 189 => "_",
        188 => "<",
        190 => ">",
        191 => "?",
        192 => "~",
        219 => "{",
        220 => "|",
        221 => "}",
        222 => "\"",
        _ => return base_name(code),
    })
}

pub(crate) fn key_name(event: KeyEvent<'_>, platform: Platform) -> &str {
    let flags = event.modifiers;
    let ignore = (platform == Platform::MacOs && flags.meta && flags.shift && !flags.ctrl && !flags.alt)
        || (event.legacy_ie && flags.shift && event.key.encode_utf16().count() == 1)
        || event.key == "Unidentified";
    let key = if !ignore && !event.key.is_empty() {
        event.key
    } else {
        let lookup = if flags.shift { shifted_name } else { base_name };
        event.key_code.and_then(lookup)
            .unwrap_or(if event.key.is_empty() { "Unidentified" } else { event.key })
    };
    match key {
        "Esc" => "Escape",
        "Del" => "Delete",
        "Left" => "ArrowLeft",
        "Up" => "ArrowUp",
        "Right" => "ArrowRight",
        "Down" => "ArrowDown",
        _ => key,
    }
}
