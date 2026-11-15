use prosemirror_history::{history_plugin, redo, redo_depth, undo, undo_depth};
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_test_builder::{build_doc, p, text, test_schema};

#[test]
fn test_history_undo_redo_cycle() {
    let doc = build_doc(|ctx| {
        ctx.wrap_block("doc", |inner| {
            vec![p(vec![text("Start")])(inner)]
        })
    }).node;

    let mut state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc),
        selection: Some(Selection::cursor(6)),
        stored_marks: None,
        plugins: vec![history_plugin(50)],
    });

    assert_eq!(undo_depth(&state), 0);
    assert_eq!(redo_depth(&state), 0);

    // Perform Edit
    let mut tr = state.tr();
    tr.insert_text(" middle", 6, 6);
    state = state.apply(&tr);

    assert_eq!(undo_depth(&state), 1);
    assert_eq!(redo_depth(&state), 0);
    assert_eq!(state.doc.text_between(1, 13), "Start middle");

    // Perform Undo
    let mut undo_tr = None;
    let did_undo = undo(&state, |t| {
        undo_tr = Some(t);
    });
    assert!(did_undo);
    if let Some(t) = undo_tr {
        state = state.apply(&t);
    }

    assert_eq!(undo_depth(&state), 0);
    assert_eq!(redo_depth(&state), 1);
    assert_eq!(state.doc.text_between(1, 6), "Start");

    // Perform Redo
    let mut redo_tr = None;
    let did_redo = redo(&state, |t| {
        redo_tr = Some(t);
    });
    assert!(did_redo);
    if let Some(t) = redo_tr {
        state = state.apply(&t);
    }

    assert_eq!(undo_depth(&state), 1);
    assert_eq!(redo_depth(&state), 0);
    assert_eq!(state.doc.text_between(1, 13), "Start middle");
}
