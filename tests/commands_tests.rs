use prosemirror_commands::{chain_commands, delete_selection, join_backward, split_block, toggle_mark};
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

    let mut tr_opt = None;
    let did_delete = delete_selection(&state, Some(&mut |tr| {
        tr_opt = Some(tr);
    }));

    assert!(did_delete);
    if let Some(tr) = tr_opt {
        state = state.apply(&tr);
    }
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

    let mut tr_opt = None;
    let did_split = split_block(&state, Some(&mut |tr| {
        tr_opt = Some(tr);
    }));

    assert!(did_split);
    if let Some(tr) = tr_opt {
        state = state.apply(&tr);
    }
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
    let mut tr_opt = None;
    toggle_em(&state, Some(&mut |tr| {
        tr_opt = Some(tr);
    }));
    if let Some(tr) = tr_opt {
        state = state.apply(&tr);
    }

    let em = Mark::new("em", None);
    assert!(em.is_in_set(state.doc.child(0).child(0).marks()));

    // Turn mark off
    let mut tr_off_opt = None;
    toggle_em(&state, Some(&mut |tr| {
        tr_off_opt = Some(tr);
    }));
    if let Some(tr) = tr_off_opt {
        state = state.apply(&tr);
    }
    assert!(!em.is_in_set(state.doc.child(0).child(0).marks()));
}

#[test]
fn test_chain_commands() {
    let doc = build_doc(|ctx| {
        ctx.wrap_block("doc", |inner| {
            vec![p(vec![text("Chain test")])(inner)]
        })
    }).node;

    let state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc),
        selection: Some(Selection::cursor(1)), // Empty selection
        stored_marks: None,
        plugins: Vec::new(),
    });

    // First command will return false (delete_selection needs non-empty selection)
    // Second command (join_backward) will handle it
    let chained = chain_commands(vec![
        Box::new(|s, d| delete_selection(s, d)),
        Box::new(|s, d| join_backward(s, d)),
    ]);

    let mut executed = false;
    let res = chained(&state, Some(&mut |_tr| {
        executed = true;
    }));
    assert!(res);
    assert!(executed);
}
