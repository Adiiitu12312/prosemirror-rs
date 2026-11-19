use prosemirror_example_setup::{build_input_rules, build_keymap};
use prosemirror_keymap::{KeyEvent, Modifiers, Platform};
use prosemirror_state::{EditorState, EditorStateConfig, Plugin, Selection};
use prosemirror_test_builder::{doc, p, test_schema};

fn editor(text: &str, plugin: Option<Plugin>) -> EditorState {
    let doc = doc!(p!(text)).node;
    EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc),
        selection: Some(Selection::cursor(text.len() + 1)),
        plugins: plugin.into_iter().collect(),
        stored_marks: None,
    })
}

#[test]
fn wraps_numbered_lists_with_the_typed_start_value() {
    let state = editor("3.", Some(build_input_rules(test_schema())));
    let tr = state.handle_text_input(3, 3, " ", false).unwrap();
    let doc = tr.doc();
    let list = doc.child(0);
    assert_eq!(list.type_name(), "ordered_list");
    assert_eq!(list.attrs().unwrap()["order"], 3);
}

#[test]
fn uses_the_number_of_hashes_as_the_heading_level() {
    let state = editor("###", Some(build_input_rules(test_schema())));
    let tr = state.handle_text_input(4, 4, " ", false).unwrap();
    assert_eq!(tr.doc().child(0).type_name(), "heading");
    assert_eq!(tr.doc().child(0).attrs().unwrap()["level"], 3);
}

#[test]
fn can_suppress_and_remap_keys() {
    let keys = [
        ("Mod-i".to_string(), Some("Mod-j".to_string())),
        ("Mod-b".to_string(), None),
    ].into();
    let keymap = build_keymap(test_schema(), &keys, Platform::Other).unwrap();
    let state = editor("abc", None);
    let mut next = None;
    let mut dispatch = |tr| { next = Some(state.apply(&tr)); };
    for key in ["i", "b", "j"] {
        let event = KeyEvent { key, modifiers: Modifiers { ctrl: true, ..Default::default() }, ..Default::default() };
        assert_eq!(keymap.handle_keydown(&state, &mut dispatch, &(), event), key == "j");
    }
    assert_eq!(next.unwrap().stored_marks.unwrap()[0].type_name, "em");
}
