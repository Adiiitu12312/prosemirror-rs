// Ported from prosemirror-history 1.4.1, test/test-history.ts.
use prosemirror_history::{
    close_history, history, redo, redo_depth, undo, undo_depth, HistoryOptions, HistoryState, HISTORY_KEY,
};
use prosemirror_model::{Fragment, Node, Slice};
use prosemirror_state::{EditorState, EditorStateConfig, Plugin, Selection, Transaction};
use prosemirror_test_builder::{doc, p, test_schema};
use prosemirror_transform::Step;
use serde_json::{json, Value};

fn state(doc: Option<Node>, options: HistoryOptions, mut plugins: Vec<Plugin>) -> EditorState {
    plugins.insert(0, history(options));
    EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc,
        selection: None,
        stored_marks: None,
        plugins,
    })
}

fn default_state() -> EditorState {
    state(None, HistoryOptions::default(), vec![])
}

fn edit(state: &mut EditorState, f: impl FnOnce(&mut Transaction)) {
    let mut tr = state.tr();
    f(&mut tr);
    *state = state.apply(&tr);
}

fn type_text(state: &mut EditorState, text: &str) {
    edit(state, |tr| { tr.insert_text_at_selection(text); });
}

fn close(state: &mut EditorState) {
    edit(state, |tr| { close_history(tr); });
}

fn undo_once(state: &mut EditorState) {
    let mut result = None;
    undo(state, |tr| result = Some(tr));
    if let Some(tr) = result {
        *state = state.apply(&tr);
    }
}

fn redo_once(state: &mut EditorState) {
    let mut result = None;
    redo(state, |tr| result = Some(tr));
    if let Some(tr) = result {
        *state = state.apply(&tr);
    }
}

fn text(text: &str) -> Fragment {
    Fragment::from_nodes(vec![test_schema().text(text, None)])
}

fn split(tr: &mut Transaction, pos: usize) {
    tr.apply_transform(|transform| { transform.split(pos, 1); });
}

fn compress(state: &mut EditorState) {
    let value = state.get_plugin_state(HISTORY_KEY).unwrap();
    let mut history: HistoryState = serde_json::from_value(value.clone()).unwrap();
    history.done.compress(history.done.items.len());
    state.plugin_states.insert(HISTORY_KEY.into(), serde_json::to_value(history).unwrap());
}

#[test]
fn enables_undo() {
    let mut state = default_state();
    type_text(&mut state, "a");
    type_text(&mut state, "b");
    assert_eq!(state.doc, doc!(p!("ab")).node);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!()).node);
}

#[test]
fn enables_redo() {
    let mut state = default_state();
    type_text(&mut state, "a");
    type_text(&mut state, "b");
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!()).node);
    redo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("ab")).node);
}

#[test]
fn tracks_multiple_levels_of_history() {
    let mut state = default_state();
    type_text(&mut state, "a");
    type_text(&mut state, "b");
    edit(&mut state, |tr| { tr.insert_text("c", 1, 1); });
    assert_eq!(state.doc, doc!(p!("cab")).node);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("ab")).node);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!()).node);
    redo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("ab")).node);
    redo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("cab")).node);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("ab")).node);
}

#[test]
fn starts_new_event_when_group_delay_elapses() {
    let options = HistoryOptions { new_group_delay: 1000, ..HistoryOptions::default() };
    let mut state = state(None, options, vec![]);
    edit(&mut state, |tr| { tr.insert_text_at_selection("a").set_time(1000); });
    edit(&mut state, |tr| { tr.insert_text_at_selection("b").set_time(1600); });
    assert_eq!(undo_depth(&state), 1);
    edit(&mut state, |tr| { tr.insert_text_at_selection("c").set_time(2700); });
    assert_eq!(undo_depth(&state), 2);
    undo_once(&mut state);
    edit(&mut state, |tr| { tr.insert_text_at_selection("d").set_time(2800); });
    assert_eq!(undo_depth(&state), 2);
}

#[test]
fn starts_new_event_for_non_adjacent_changes() {
    let options = HistoryOptions { new_group_delay: 1000, ..HistoryOptions::default() };
    let mut state = state(Some(doc!(p!("abc")).node), options, vec![]);
    edit(&mut state, |tr| { tr.insert_text("x", 1, 1); });
    edit(&mut state, |tr| { tr.insert_text("y", 5, 5); });
    assert_eq!(undo_depth(&state), 2);
}

#[test]
fn checks_adjacency_when_transactions_include_non_replacement_steps() {
    let options = HistoryOptions { new_group_delay: 1000, ..HistoryOptions::default() };
    let mut state = state(Some(doc!(p!()).node), options, vec![]);
    let em = test_schema().mark_type("em").unwrap().create(None);
    edit(&mut state, |tr| { tr.insert_text("x", 1, 1).add_mark(1, 2, em.clone()); });
    edit(&mut state, |tr| { tr.insert_text("y", 2, 2).add_mark(2, 3, em); });
    assert_eq!(undo_depth(&state), 1);
}

#[test]
fn allows_changes_outside_history() {
    let mut state = default_state();
    type_text(&mut state, "hello");
    edit(&mut state, |tr| { tr.insert_text("oops", 1, 1).set_meta("addToHistory", json!(false)); });
    edit(&mut state, |tr| { tr.insert_text("!", 10, 10).set_meta("addToHistory", json!(false)); });
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("oops!")).node);
}

#[test]
fn handles_undo_without_adding_redo_item() {
    let mut state = default_state();
    type_text(&mut state, "foo");
    edit(&mut state, |tr| { tr.replace_with(1, 4, text("bar")).set_meta("addToHistory", json!(false)); });
    undo_once(&mut state);
    redo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("bar")).node);
}

fn unsynced_complex(do_compress: bool) {
    let mut state = default_state();
    type_text(&mut state, "hello");
    close(&mut state);
    type_text(&mut state, "!");
    edit(&mut state, |tr| { tr.insert_text("....", 1, 1).set_meta("addToHistory", json!(false)); });
    edit(&mut state, |tr| split(tr, 3));
    assert_eq!(state.doc, doc!(p!(".."), p!("..hello!")).node);
    edit(&mut state, |tr| {
        split(tr, 2);
        tr.set_meta("addToHistory", json!(false));
    });
    if do_compress {
        compress(&mut state);
    }
    undo_once(&mut state);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("."), p!("...hello")).node);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("."), p!("...")).node);
}

#[test]
fn handles_complex_editing_sequences() {
    unsynced_complex(false);
}

#[test]
fn handles_complex_editing_sequences_with_compression() {
    unsynced_complex(true);
}

#[test]
fn supports_overlapping_edits() {
    let mut state = default_state();
    type_text(&mut state, "hello");
    close(&mut state);
    edit(&mut state, |tr| { tr.delete(1, 6); });
    assert_eq!(state.doc, doc!(p!()).node);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("hello")).node);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!()).node);
}

#[test]
fn supports_overlapping_uncollapsed_edits() {
    let mut state = default_state();
    edit(&mut state, |tr| { tr.insert_text("h", 1, 1).set_meta("addToHistory", json!(false)); });
    type_text(&mut state, "ello");
    close(&mut state);
    edit(&mut state, |tr| { tr.delete(1, 6); });
    assert_eq!(state.doc, doc!(p!()).node);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("hello")).node);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("h")).node);
}

#[test]
fn supports_overlapping_unsynced_deletes() {
    let mut state = default_state();
    type_text(&mut state, "hi");
    close(&mut state);
    type_text(&mut state, "hello");
    edit(&mut state, |tr| { tr.delete(1, 8).set_meta("addToHistory", json!(false)); });
    assert_eq!(state.doc, doc!(p!()).node);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!()).node);
}

#[test]
fn goes_back_and_forth_through_history_multiple_times() {
    let mut state = default_state();
    type_text(&mut state, "one");
    type_text(&mut state, " two");
    close(&mut state);
    type_text(&mut state, " three");
    edit(&mut state, |tr| { tr.insert_text("zero ", 1, 1); });
    close(&mut state);
    edit(&mut state, |tr| split(tr, 1));
    edit(&mut state, |tr| { tr.set_selection(Selection::cursor(1)); });
    type_text(&mut state, "top");
    for _ in 0..3 {
        for _ in 0..4 { undo_once(&mut state); }
        assert_eq!(state.doc, doc!(p!()).node);
        for _ in 0..4 { redo_once(&mut state); }
        assert_eq!(state.doc, doc!(p!("top"), p!("zero one two three")).node);
    }
}

#[test]
fn supports_untracked_changes_next_to_tracked_changes() {
    let mut state = default_state();
    type_text(&mut state, "o");
    edit(&mut state, |tr| split(tr, 1));
    edit(&mut state, |tr| { tr.insert_text("zzz", 4, 4).set_meta("addToHistory", json!(false)); });
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("zzz")).node);
}

#[test]
fn goes_back_and_forth_with_untracked_changes_and_compression() {
    let mut state = default_state();
    type_text(&mut state, "one");
    type_text(&mut state, " two");
    close(&mut state);
    let head = state.selection.head(&state.doc);
    edit(&mut state, |tr| { tr.insert_text("xxx", head, head).set_meta("addToHistory", json!(false)); });
    type_text(&mut state, " three");
    edit(&mut state, |tr| { tr.insert_text("zero ", 1, 1); });
    close(&mut state);
    edit(&mut state, |tr| split(tr, 1));
    edit(&mut state, |tr| { tr.set_selection(Selection::cursor(1)); });
    type_text(&mut state, "top");
    edit(&mut state, |tr| { tr.insert_text("yyy", 1, 1).set_meta("addToHistory", json!(false)); });
    for iteration in 0..3 {
        if iteration == 2 { compress(&mut state); }
        for _ in 0..4 { undo_once(&mut state); }
        assert_eq!(state.doc, doc!(p!("yyyxxx")).node);
        for _ in 0..4 { redo_once(&mut state); }
        assert_eq!(state.doc, doc!(p!("yyytop"), p!("zero one twoxxx three")).node);
    }
}

#[test]
fn restores_selection_on_undo() {
    let mut state = default_state();
    type_text(&mut state, "hi");
    close(&mut state);
    let selection = Selection::text(1, 3);
    edit(&mut state, |tr| { tr.set_selection(selection.clone()); });
    edit(&mut state, |tr| { tr.replace_with(1, 3, text("hello")); });
    let selection2 = state.selection.clone();
    undo_once(&mut state);
    assert_eq!(state.selection, selection);
    redo_once(&mut state);
    assert_eq!(state.selection, selection2);
}

#[test]
fn rebases_selection_on_undo() {
    let mut state = default_state();
    type_text(&mut state, "hi");
    close(&mut state);
    edit(&mut state, |tr| { tr.set_selection(Selection::text(1, 3)); });
    edit(&mut state, |tr| { tr.insert(1, text("hello")); });
    edit(&mut state, |tr| { tr.insert(1, text("---")).set_meta("addToHistory", json!(false)); });
    undo_once(&mut state);
    assert_eq!(state.selection.head(&state.doc), 6);
}

#[test]
fn handles_change_overwriting_in_item_preserving_mode() {
    let mut plugin = Plugin::unkeyed(None);
    plugin.spec.insert("historyPreserveItems".into(), json!(true));
    let mut state = state(None, HistoryOptions::default(), vec![plugin]);
    type_text(&mut state, "a");
    type_text(&mut state, "b");
    close(&mut state);
    edit(&mut state, |tr| { tr.set_selection(Selection::text(1, 3)); });
    type_text(&mut state, "c");
    undo_once(&mut state);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!()).node);
}

#[test]
fn supports_querying_undo_and_redo_depth() {
    let mut state = default_state();
    type_text(&mut state, "a");
    assert_eq!((undo_depth(&state), redo_depth(&state)), (1, 0));
    edit(&mut state, |tr| { tr.insert_text("b", 1, 1).set_meta("addToHistory", json!(false)); });
    assert_eq!((undo_depth(&state), redo_depth(&state)), (1, 0));
    undo_once(&mut state);
    assert_eq!((undo_depth(&state), redo_depth(&state)), (0, 1));
    redo_once(&mut state);
    assert_eq!((undo_depth(&state), redo_depth(&state)), (1, 0));
}

#[test]
fn handles_editor_states_without_history() {
    let state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(), doc: None, selection: None, stored_marks: None, plugins: vec![],
    });
    assert_eq!(undo_depth(&state), 0);
    assert_eq!(redo_depth(&state), 0);
    assert!(!undo(&state, |_| panic!("no undo transaction")));
    assert!(!redo(&state, |_| panic!("no redo transaction")));
}

#[test]
fn truncates_history() {
    let options = HistoryOptions { depth: 2, ..HistoryOptions::default() };
    let mut state = state(None, options, vec![]);
    for i in 1_i32..40 {
        type_text(&mut state, "a");
        close(&mut state);
        assert_eq!(undo_depth(&state), ((i - 2) % 21 + 2) as usize);
    }
}

#[test]
fn supports_transactions_with_multiple_steps() {
    let mut state = default_state();
    edit(&mut state, |tr| { tr.insert_text_at_selection("a").insert_text_at_selection("b"); });
    edit(&mut state, |tr| { tr.insert_text("c", 1, 1); });
    assert_eq!(state.doc, doc!(p!("cab")).node);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("ab")).node);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!()).node);
    redo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("ab")).node);
    redo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("cab")).node);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("ab")).node);
}

#[test]
fn combines_appended_transactions_into_root_event() {
    let plugin = Plugin::unkeyed(None).with_append_transaction(|_, _, state| {
        if state.doc.content().size() == 4 {
            let mut tr = state.tr();
            tr.insert(1, text("A"));
            Some(tr)
        } else {
            None
        }
    });
    let mut state = state(Some(doc!(p!("x")).node), HistoryOptions::default(), vec![plugin]);
    edit(&mut state, |tr| { tr.insert(2, text("I")); });
    assert_eq!(state.doc, doc!(p!("AxI")).node);
    assert_eq!(undo_depth(&state), 1);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("x")).node);
}

fn append_added_text() -> Plugin {
    Plugin::unkeyed(None).with_append_transaction(|transactions, _, state| {
        let value = transactions.first()?.get_meta("add").and_then(Value::as_str)?;
        let mut tr = state.tr();
        tr.insert(1, text(value));
        Some(tr)
    })
}

#[test]
fn includes_transactions_appended_to_undo_in_redo_history() {
    let mut state = state(Some(doc!(p!("x")).node), HistoryOptions::default(), vec![append_added_text()]);
    edit(&mut state, |tr| { tr.insert(2, text("I")).set_meta("add", json!("A")); });
    assert_eq!(state.doc, doc!(p!("AxI")).node);
    let mut result = None;
    undo(&state, |mut tr| {
        tr.set_meta("add", json!("B"));
        result = Some(tr);
    });
    state = state.apply(&result.take().unwrap());
    assert_eq!(state.doc, doc!(p!("Bx")).node);
    redo(&state, |mut tr| {
        tr.set_meta("add", json!("C"));
        result = Some(tr);
    });
    state = state.apply(&result.take().unwrap());
    assert_eq!(state.doc, doc!(p!("CAxI")).node);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("Bx")).node);
}

#[test]
fn keeps_history_open_for_appended_transactions() {
    let mut state = state(Some(doc!(p!("x")).node), HistoryOptions::default(), vec![append_added_text()]);
    edit(&mut state, |tr| { tr.insert(2, text("R")).set_meta("add", json!("A")); });
    edit(&mut state, |tr| { tr.insert(3, text("M")); });
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("x")).node);
}

#[test]
fn supports_rebasing() {
    let mut state = default_state();
    type_text(&mut state, "base");
    close(&mut state);
    let base_doc = state.doc.clone();
    let right = Step::replace(5, 5, Slice::new(text(" right"), 0, 0));
    edit(&mut state, |tr| { tr.step(right.clone()).unwrap(); });
    assert_eq!(state.doc, doc!(p!("base right")).node);
    assert_eq!(undo_depth(&state), 2);
    let left = Step::replace(1, 1, Slice::new(text("left "), 0, 0));
    edit(&mut state, |tr| {
        tr.step(right.invert(&base_doc)).unwrap();
        tr.step(left).unwrap();
        let rebased = right.map(&tr.mapping().slice(1, tr.steps().len())).unwrap();
        tr.step(rebased).unwrap();
        tr.transform.mapping.set_mirror(0, 2);
        tr.set_meta("addToHistory", json!(false)).set_meta("rebased", json!(1));
    });
    assert_eq!(state.doc, doc!(p!("left base right")).node);
    assert_eq!(undo_depth(&state), 2);
    undo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("left base")).node);
    redo_once(&mut state);
    assert_eq!(state.doc, doc!(p!("left base right")).node);
}

#[test]
fn maps_selection_when_rebasing() {
    let mut state = state(Some(doc!(p!("123456789ABCD")).node), HistoryOptions::default(), vec![]);
    edit(&mut state, |tr| { tr.set_selection(Selection::text(6, 13)); });
    edit(&mut state, |tr| { tr.delete(6, 13); });
    edit(&mut state, |tr| {
        tr.insert(6, text("6789ABC")).insert(14, text("E")).delete(6, 13)
            .set_meta("rebased", json!(1)).set_meta("addToHistory", json!(false));
        tr.transform.mapping.set_mirror(0, 2);
    });
    undo_once(&mut state);
}
