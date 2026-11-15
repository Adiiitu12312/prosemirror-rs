use prosemirror_model::Node;
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_test_builder::{build_doc, p, text, test_schema};

#[test]
fn test_state_creation_and_transaction() {
    let doc = build_doc(|ctx| {
        ctx.wrap_block("doc", |inner| {
            vec![p(vec![text("Original state content")])(inner)]
        })
    }).node;

    let state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc),
        selection: Some(Selection::cursor(1)),
        stored_marks: None,
        plugins: Vec::new(),
    });

    let mut tr = state.tr();
    tr.insert_text("Prefix: ", 1, 1);
    let next_state = state.apply(&tr);

    assert_eq!(next_state.doc.text_between(1, 31), "Prefix: Original state content");
    assert_eq!(next_state.selection.from(), 9); // Cursor moved after inserted prefix
}

#[test]
fn test_state_json_roundtrip() {
    let doc = build_doc(|ctx| {
        ctx.wrap_block("doc", |inner| {
            vec![p(vec![text("Serialize me")])(inner)]
        })
    }).node;

    let state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc),
        selection: Some(Selection::text(1, 5)),
        stored_marks: None,
        plugins: Vec::new(),
    });

    let json = state.to_json();
    let restored = EditorState::from_json(test_schema(), &json).expect("deserialize state");

    assert_eq!(state.doc, restored.doc);
    assert_eq!(state.selection, restored.selection);
}
