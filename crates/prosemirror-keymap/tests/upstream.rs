//! Upstream test/test-keymap.ts at 176de4152dca0e473f74504e8995d7a281392eb5.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use prosemirror_keymap::{dispatch_keymaps, keydown_handler, keymap, normalize_key_name, KeyCommand, KeyEvent, Keymap, Modifiers, Platform};
use prosemirror_schema_basic::schema;
use prosemirror_state::{EditorState, EditorStateConfig};

fn state() -> EditorState {
    EditorState::create(EditorStateConfig {
        schema: schema().clone(),
        doc: None,
        selection: None,
        stored_marks: None,
        plugins: Vec::new(),
    })
}

fn counter() -> (KeyCommand, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let command = Arc::new(move |_: &EditorState, _: prosemirror_keymap::Dispatch<'_>, _: &()| {
        observed.fetch_add(1, Ordering::Relaxed);
        true
    });
    (command, count)
}

fn click(map: &Keymap, key: &str, modifiers: Modifiers, code: Option<u16>) -> bool {
    let event = KeyEvent { key, key_code: code, modifiers, legacy_ie: false };
    map.handle_keydown(&state(), &mut |_| {}, &(), event)
}

#[test]
fn calls_the_correct_handler() {
    let (a, a_count) = counter();
    let (b, b_count) = counter();
    let map = keymap([("KeyA", a), ("KeyB", b)], Platform::Other).unwrap();
    assert!(click(&map, "KeyA", Modifiers::default(), None));
    assert_eq!(a_count.load(Ordering::Relaxed), 1);
    assert_eq!(b_count.load(Ordering::Relaxed), 0);
}

#[test]
fn distinguishes_modifiers_and_space_aliases() {
    let (space, space_count) = counter();
    let (control, control_count) = counter();
    let (shift_control, shift_control_count) = counter();
    let (alt, alt_count) = counter();
    let map = keymap([
        ("Space", space), ("Control-Space", control),
        ("s-c-Space", shift_control), ("alt-Space", alt),
    ], Platform::Other).unwrap();
    assert!(click(&map, " ", Modifiers { ctrl: true, ..Modifiers::default() }, None));
    assert!(click(&map, " ", Modifiers { ctrl: true, shift: true, ..Modifiers::default() }, None));
    assert_eq!(space_count.load(Ordering::Relaxed), 0);
    assert_eq!(control_count.load(Ordering::Relaxed), 1);
    assert_eq!(shift_control_count.load(Ordering::Relaxed), 1);
    assert_eq!(alt_count.load(Ordering::Relaxed), 0);
}

#[test]
fn passes_state_dispatch_and_view() {
    struct FakeView {
        state: EditorState,
    }
    let view = FakeView { state: state() };
    let command: KeyCommand<FakeView> = Arc::new(|state: &EditorState, dispatch: prosemirror_keymap::Dispatch<'_>, view: &FakeView| {
        assert!(std::ptr::eq(state, &view.state));
        dispatch(state.tr());
        true
    });
    let mut called = false;
    let handler = keydown_handler([("X", command)], Platform::Other).unwrap();
    assert!(handler(&view.state, &mut |_| { called = true; }, &view, KeyEvent::new("X")));
    assert!(called);
}

#[test]
fn tries_both_shifted_key_and_base_with_shift_modifier() {
    let (percent, count) = counter();
    let (shift_five, base_count) = counter();
    let modifiers = Modifiers { ctrl: true, shift: true, ..Modifiers::default() };
    let map = keymap([("Ctrl-%", percent)], Platform::Other).unwrap();
    assert!(click(&map, "%", modifiers, Some(53)));
    assert_eq!(count.load(Ordering::Relaxed), 1);
    let map = keymap([("Ctrl-Shift-5", shift_five)], Platform::Other).unwrap();
    assert!(click(&map, "%", modifiers, Some(53)));
    assert_eq!(base_count.load(Ordering::Relaxed), 1);
}

#[test]
fn shift_alone_does_not_fall_back_to_the_keycode() {
    let (command, count) = counter();
    let modifiers = Modifiers { shift: true, ..Modifiers::default() };
    let map = keymap([("Shift-5", command)], Platform::Other).unwrap();
    assert!(!click(&map, "%", modifiers, Some(53)));
    assert_eq!(count.load(Ordering::Relaxed), 0);
}

#[test]
fn tries_keycode_when_a_modifier_is_active() {
    let (command, count) = counter();
    let map = keymap([("Shift-Alt-3", command)], Platform::Other).unwrap();
    let modifiers = Modifiers { shift: true, alt: true, ..Modifiers::default() };
    assert!(click(&map, "×", modifiers, Some(51)));
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[test]
fn tries_keycode_for_non_ascii_characters_with_mod() {
    let (command, count) = counter();
    let map = keymap([("Mod-s", command)], Platform::Other).unwrap();
    assert!(click(&map, "ы", Modifiers { ctrl: true, ..Modifiers::default() }, Some(83)));
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[test]
fn does_not_capture_altgr_text_on_windows() {
    let (command, count) = counter();
    let map = keymap([("Ctrl-Alt-e", command)], Platform::Windows).unwrap();
    let flags = Modifiers { ctrl: true, alt: true, ..Modifiers::default() };
    assert!(!click(&map, "€", flags, Some(69)));
    assert_eq!(count.load(Ordering::Relaxed), 0);
}

#[test]
fn handles_mac_shift_command_and_ios_mod() {
    let (command, count) = counter();
    let map = keymap([("Mod-S", command)], Platform::MacOs).unwrap();
    let flags = Modifiers { meta: true, shift: true, ..Modifiers::default() };
    assert!(click(&map, "s", flags, Some(83)));
    assert_eq!(count.load(Ordering::Relaxed), 1);
    assert_eq!(normalize_key_name("Mod--", Platform::Ios).unwrap(), "Meta--");
}

#[test]
fn keymaps_run_in_order_until_one_handles_the_event() {
    let (command, count) = counter();
    let decline: KeyCommand = Arc::new(|_: &EditorState, _: prosemirror_keymap::Dispatch<'_>, _: &()| false);
    let first = keymap([("Escape", decline)], Platform::Other).unwrap();
    let second = keymap([("Escape", command)], Platform::Other).unwrap();
    let (unreached, unreached_count) = counter();
    let third = keymap([("Escape", unreached)], Platform::Other).unwrap();
    assert!(dispatch_keymaps(&[first, second, third], &state(), &mut |_| {}, &(), KeyEvent::new("Esc")));
    assert_eq!(count.load(Ordering::Relaxed), 1);
    assert_eq!(unreached_count.load(Ordering::Relaxed), 0);
}

#[test]
fn rejects_unknown_modifiers() {
    let error = normalize_key_name("Hyper-a", Platform::Other).unwrap_err();
    assert_eq!(error.modifier, "Hyper");
}
