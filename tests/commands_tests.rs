use prosemirror_commands::{delete_selection, join_backward, split_block, toggle_mark};
use prosemirror_model::Mark;
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_test_builder::{build_doc, p, text, test_schema};

#[test]
fn test_delete_selection_command() {
    let doc = build_doc(|ctx| {
        ctx.wrap_block("doc", |inner| {
            vec![p(vec![text("abcdef")])(inner)]
        })
    }).node;

    let mut state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc),
        selection: Some(Selection::text(2, 5)), // "bcd"
        stored_marks: None,
        plugins: Vec::new(),
    });

    let did_delete = delete_selection(&state, Some(&mut |tr| {
        state = state.apply(&tr);
    }));

    assert!(did_delete);
    assert_eq!(state.doc.text_between(1, 4), "aef");
}

#[test]
fn test_split_block_command() {
    let doc = build_doc(|ctx| {
        ctx.wrap_block("doc", |inner| {
            vec![p(vec![text("LineOneLineTwo")])(inner)]
        })
    }).node;

    let mut state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc),
        selection: Some(Selection::cursor(8)), // Split after "LineOne"
        stored_marks: None,
        plugins: Vec::new(),
    });

    let did_split = split_block(&state, Some(&mut |tr| {
        state = state.apply(&tr);
    }));

    assert!(did_split);
    assert_eq!(state.doc.child_count(), 2);
    assert_eq!(state.doc.child(0).text_between(0, 7), "LineOne");
    assert_eq!(state.doc.child(1).text_between(0, 7), "LineTwo");
}

#[test]
fn test_toggle_mark_command() {
    let doc = build_doc(|ctx| {
        ctx.wrap_block("doc", |inner| {
            vec![p(vec![text("Plain text")])(inner)]
        })
    }).node;

    let mut state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc),
        selection: Some(Selection::text(1, 6)), // "Plain"
        stored_marks: None,
        plugins: Vec::new(),
    });

    let toggle_em = toggle_mark("em", None);

    // Turn mark on
    toggle_em(&state, Some(&mut |tr| {
        state = state.apply(&tr);
    }));

    let em = Mark::new("em", None);
    assert!(em.is_in_set(state.doc.child(0).child(0).marks()));

    // Turn mark off
    toggle_em(&state, Some(&mut |tr| {
        state = state.apply(&tr);
    }));
    assert!(!em.is_in_set(state.doc.child(0).child(0).marks()));
}
