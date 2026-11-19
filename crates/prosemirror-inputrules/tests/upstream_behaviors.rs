// Expectations verified against the published prosemirror-inputrules 1.5.0 package.
use prosemirror_inputrules::{
    ellipsis, em_dash, input_rules, smart_quotes, textblock_type_input_rule, undo_input_rule,
    wrapping_input_rule, Attrs, InCode, InputRule, InputRuleOptions,
};
use prosemirror_model::{Fragment, MarkSpec, Node, NodeSpec, Schema};
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_test_builder::{blockquote, doc, p, test_schema};
use serde_json::json;

fn create(doc: Node, rules: Vec<InputRule>) -> EditorState {
    let end = doc.content().size() - 1;
    EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc),
        selection: Some(Selection::cursor(end)),
        stored_marks: None,
        plugins: vec![input_rules(rules)],
    })
}

fn typed(state: &EditorState, text: &str) -> Option<EditorState> {
    let from = state.selection.from();
    let to = state.selection.to(&state.doc);
    let tr = state.handle_text_input(from, to, text, false)?;
    Some(state.apply(&tr))
}

fn replace(pattern: &str, flags: &str, replacement: &str) -> InputRule {
    InputRule::replace(pattern, flags, replacement, InputRuleOptions::default()).unwrap()
}

fn code_block(text: &str) -> Node {
    test_schema().node(
        "code_block",
        None,
        Some(Fragment::from_nodes(vec![test_schema().text(text, None)])),
        None,
    ).unwrap()
}

#[test]
fn replaces_an_em_dash_and_undoes_to_literal_input() {
    let state = create(doc!(p!("a-")).node, vec![em_dash()]);
    let state = typed(&state, "-").unwrap();
    assert_eq!(state.doc, doc!(p!("a—")).node);
    let mut undone = None;
    assert!(undo_input_rule(&state, |tr| undone = Some(state.apply(&tr))));
    assert_eq!(undone.unwrap().doc, doc!(p!("a--")).node);
}

#[test]
fn replaces_ellipsis() {
    let state = create(doc!(p!("..")).node, vec![ellipsis()]);
    assert_eq!(typed(&state, ".").unwrap().doc, doc!(p!("…")).node);
}

#[test]
fn smart_quotes_choose_opening_and_closing_context() {
    for (prefix, input, expected) in [
        ("hello ", "\"", "hello “"),
        ("hello", "\"", "hello”"),
        ("hello ", "'", "hello ‘"),
        ("it", "'", "it’"),
    ] {
        let state = create(doc!(p!(prefix)).node, smart_quotes().into());
        assert_eq!(typed(&state, input).unwrap().doc, doc!(p!(expected)).node);
    }
}

#[test]
fn honors_code_node_options() {
    let state = create(doc!(code_block("-")).node, vec![em_dash()]);
    assert!(typed(&state, "-").is_none());

    let only_code = InputRuleOptions { in_code: InCode::Only, ..Default::default() };
    let rule = InputRule::replace(r"--$", "", "X", only_code).unwrap();
    let state = create(doc!(code_block("-")).node, vec![rule.clone()]);
    assert_eq!(typed(&state, "-").unwrap().doc, doc!(code_block("X")).node);
    let state = create(doc!(p!("-")).node, vec![rule]);
    assert!(typed(&state, "-").is_none());
}

#[test]
fn honors_explicit_code_mark_options() {
    let schema = Schema::new(
        vec![
            ("doc".into(), NodeSpec { content: Some("paragraph+".into()), ..Default::default() }),
            ("paragraph".into(), NodeSpec { content: Some("text*".into()), ..Default::default() }),
            ("text".into(), NodeSpec { inline: true, ..Default::default() }),
        ],
        vec![("literal".into(), MarkSpec { code: true, ..Default::default() })],
    );
    let marked = |text| {
        let mark = schema.mark_type("literal").unwrap().create(None);
        let paragraph = schema.node(
            "paragraph", None, Some(Fragment::from_nodes(vec![schema.text(text, Some(vec![mark]))])), None,
        ).unwrap();
        schema.node("doc", None, Some(Fragment::from_nodes(vec![paragraph])), None).unwrap()
    };
    let state = create(marked("-"), vec![em_dash()]);
    assert!(typed(&state, "-").is_none());
    let state = create(marked("-"), vec![replace(r"--$", "", "X")]);
    assert_eq!(typed(&state, "-").unwrap().doc, marked("X"));
}

#[test]
fn preserves_text_outside_first_capture_even_for_multi_character_input() {
    for (pattern, input, expected) in [(r"a(b)c$", "abc", "aXc"), (r"(a)1a$", "a1a", "a1X")] {
        let state = create(doc!(p!()).node, vec![replace(pattern, "", "X")]);
        assert_eq!(typed(&state, input).unwrap().doc, doc!(p!(expected)).node);
    }
}

#[test]
fn matches_a_500_code_unit_prefix_window() {
    let prefix = "x".repeat(501);
    let state = create(doc!(p!(prefix)).node, vec![replace(r"^x{500}!$", "", "Z")]);
    assert_eq!(typed(&state, "!").unwrap().doc, doc!(p!("xZ")).node);
}

#[test]
fn handles_ecmascript_backreferences_and_utf16_positions() {
    let state = create(doc!(p!("😀A")).node, vec![replace(r"([a-z])\1$", "i", "X")]);
    assert_eq!(typed(&state, "a").unwrap().doc, doc!(p!("😀Xa")).node);
}

#[test]
fn custom_handlers_see_named_captures_and_global_match_position() {
    let rule = InputRule::new(r"(?<ending>a)$", "g", InputRuleOptions::default(), |state, matched, from, to| {
        assert_eq!(matched.index(), 1);
        assert_eq!(matched.named_group("ending").as_deref(), Some("a"));
        assert_eq!((from, to), (2, 3));
        let mut tr = state.tr();
        tr.insert_text("X", from, to);
        Some(tr)
    }).unwrap();
    let state = create(doc!(p!("ba")).node, vec![rule]);
    assert_eq!(typed(&state, "").unwrap().doc, doc!(p!("bX")).node);
    assert!(typed(&state, "").is_none());
    assert_eq!(typed(&state, "").unwrap().doc, doc!(p!("bX")).node);
}

#[test]
fn skips_when_match_is_shorter_than_input() {
    let state = create(doc!(p!()).node, vec![em_dash()]);
    assert!(typed(&state, " x--").is_none());
}

#[test]
fn wraps_textblock_and_joins_compatible_previous_wrapper() {
    let rule = wrapping_input_rule(r"^> $", "", "blockquote", Attrs::default(), None).unwrap();
    let state = create(doc!(blockquote!(p!("a")), p!(">")).node, vec![rule]);
    let state = typed(&state, " ").unwrap();
    assert_eq!(state.doc, doc!(blockquote!(p!("a"), p!())).node);
}

#[test]
fn converts_textblock_type_using_capture_attributes() {
    let rule = textblock_type_input_rule(r"^(#+) $", "", "heading", Attrs::dynamic(|matched| {
        Some(json!({"level": matched.group(1)?.len()}))
    })).unwrap();
    let state = create(doc!(p!("##")).node, vec![rule]);
    let state = typed(&state, " ").unwrap();
    assert_eq!(state.doc.child(0).type_name(), "heading");
    assert_eq!(state.doc.child(0).attrs(), Some(&json!({"level": 2})));
}

#[test]
fn treats_inline_leaf_nodes_as_replacement_characters() {
    let image = || test_schema().node("image", Some(json!({"src": "img.png"})), None, None).unwrap();
    let rule = replace(r"\ufffc(#)$", "", "X");
    let state = create(doc!(p!(image())).node, vec![rule]);
    let state = typed(&state, "#").unwrap();
    assert_eq!(state.doc, doc!(p!(image(), "X")).node);
}

#[test]
fn applies_composition_rules_after_commit() {
    let state = create(doc!(p!("--")).node, vec![em_dash()]);
    assert!(state.handle_text_input(3, 3, "", true).is_none());
    let tr = state.handle_composition_end().unwrap();
    assert_eq!(state.apply(&tr).doc, doc!(p!("—")).node);
}

#[test]
fn undoability_is_cleared_only_by_selection_and_document_edits() {
    let state = create(doc!(p!("-")).node, vec![em_dash()]);
    let state = typed(&state, "-").unwrap();
    let mut tr = state.tr();
    tr.set_meta("other", json!(true));
    let state = state.apply(&tr);
    assert!(undo_input_rule(&state, |_| {}));
    let mut tr = state.tr();
    tr.set_selection(Selection::cursor(1));
    let state = state.apply(&tr);
    assert!(!undo_input_rule(&state, |_| {}));

    let options = InputRuleOptions { undoable: false, ..Default::default() };
    let rule = InputRule::replace(r"--$", "", "X", options).unwrap();
    let state = create(doc!(p!("-")).node, vec![rule]);
    let state = typed(&state, "-").unwrap();
    assert!(!undo_input_rule(&state, |_| {}));
}
