// Ported from prosemirror-collab 1.3.1, test/test-collab.ts.
use std::sync::Arc;

use prosemirror_collab::{
    collab, get_version, receive_transaction, sendable_steps, ClientId, CollabConfig, ReceiveOptions,
};
use prosemirror_history::{close_history, history, redo, undo, HistoryOptions};
use prosemirror_model::{Fragment, Node, Slice};
use prosemirror_state::{EditorState, EditorStateConfig, Selection, Transaction};
use prosemirror_test_builder::{doc, p, test_schema};
use prosemirror_transform::Step;
use serde_json::{json, Value};

struct Server {
    states: Vec<EditorState>,
    steps: Vec<Step>,
    client_ids: Vec<ClientId>,
    delayed: Vec<usize>,
}

impl Server {
    fn new(doc: Option<Node>, n: usize) -> Self {
        let states = (0..n).map(|index| {
            let collab = collab(CollabConfig {
                client_id: Some(ClientId::Number(index as i64)),
                ..CollabConfig::default()
            });
            EditorState::create(EditorStateConfig {
                schema: test_schema().clone(),
                doc: doc.clone(),
                selection: None,
                stored_marks: None,
                plugins: vec![history(HistoryOptions::default()), collab],
            })
        }).collect();
        Self { states, steps: Vec::new(), client_ids: Vec::new(), delayed: Vec::new() }
    }

    fn pair() -> Self {
        Self::new(None, 2)
    }

    fn sync(&mut self, index: usize) {
        let state = &self.states[index];
        let version = get_version(state).unwrap() as usize;
        if version != self.steps.len() {
            let tr = receive_transaction(
                state, &self.steps[version..], &self.client_ids[version..], ReceiveOptions::default(),
            ).unwrap();
            self.states[index] = state.apply(&tr);
        }
    }

    fn send(&mut self, index: usize) {
        if let Some(sendable) = sendable_steps(&self.states[index]).unwrap() {
            if sendable.version as usize == self.steps.len() {
                self.client_ids.extend(std::iter::repeat_n(sendable.client_id, sendable.steps.len()));
                self.steps.extend(sendable.steps);
            }
        }
    }

    fn broadcast(&mut self, index: usize) {
        if self.delayed.contains(&index) {
            return;
        }
        self.sync(index);
        self.send(index);
        for peer in 0..self.states.len() {
            if peer != index {
                self.sync(peer);
            }
        }
    }

    fn update(&mut self, index: usize, f: impl FnOnce(&EditorState) -> Transaction) {
        let state = &self.states[index];
        self.states[index] = state.apply(&f(state));
        self.broadcast(index);
    }

    fn type_text(&mut self, index: usize, text: &str, pos: Option<usize>) {
        self.update(index, |state| {
            let pos = pos.unwrap_or_else(|| state.selection.head(&state.doc));
            let mut tr = state.tr();
            tr.insert_text(text, pos, pos);
            tr
        });
    }

    fn undo(&mut self, index: usize) {
        let mut tr = None;
        undo(&self.states[index], |next| tr = Some(next));
        if let Some(tr) = tr {
            self.update(index, |_| tr);
        }
    }

    fn redo(&mut self, index: usize) {
        let mut tr = None;
        redo(&self.states[index], |next| tr = Some(next));
        if let Some(tr) = tr {
            self.update(index, |_| tr);
        }
    }

    fn compare(&self, doc: Node) {
        for state in &self.states {
            assert_eq!(state.doc, doc);
        }
    }

    fn compare_text(&self, text: &str) {
        self.compare(doc!(p!(text)).node);
    }

    fn delay(&mut self, index: usize, f: impl FnOnce(&mut Self)) {
        self.delayed.push(index);
        f(self);
        self.delayed.pop();
        self.broadcast(index);
    }

    fn select(&mut self, index: usize, near: usize) {
        self.update(index, |state| {
            let mut tr = state.tr();
            tr.set_selection(Selection::near(&state.doc.resolve(near).unwrap(), 1));
            tr
        });
    }

    fn close(&mut self, index: usize) {
        self.update(index, |state| {
            let mut tr = state.tr();
            close_history(&mut tr);
            tr
        });
    }

    fn delete(&mut self, index: usize, from: usize, to: usize) {
        self.update(index, |state| {
            let mut tr = state.tr();
            tr.delete(from, to);
            tr
        });
    }
}

#[test]
fn converges_for_simple_changes() {
    let mut server = Server::pair();
    server.type_text(0, "hi", None);
    server.type_text(1, "ok", Some(3));
    server.type_text(0, "!", Some(5));
    server.type_text(1, "...", Some(1));
    server.compare_text("...hiok!");
}

#[test]
fn converges_for_multiple_local_changes() {
    let mut server = Server::pair();
    server.type_text(0, "hi", None);
    server.delay(0, |server| {
        server.type_text(0, "A", None);
        server.type_text(1, "X", None);
        server.type_text(0, "B", None);
        server.type_text(1, "Y", None);
    });
    server.compare_text("hiXYAB");
}

#[test]
fn converges_with_three_peers() {
    let mut server = Server::new(None, 3);
    server.type_text(0, "A", None);
    server.type_text(1, "U", None);
    server.type_text(2, "X", None);
    server.type_text(0, "B", None);
    server.type_text(1, "V", None);
    server.type_text(2, "C", None);
    server.compare_text("AUXBVC");
}

#[test]
fn converges_with_three_peers_with_multiple_steps() {
    let mut server = Server::new(None, 3);
    server.type_text(0, "A", None);
    server.delay(1, |server| {
        server.type_text(1, "U", None);
        server.type_text(2, "X", None);
        server.type_text(0, "B", None);
        server.type_text(1, "V", None);
        server.type_text(2, "C", None);
    });
    server.compare_text("AXBCUV");
}

#[test]
fn supports_undo() {
    let mut server = Server::pair();
    server.type_text(0, "A", None);
    server.type_text(1, "B", None);
    server.type_text(0, "C", None);
    server.undo(1);
    server.compare_text("AC");
    server.type_text(1, "D", None);
    server.type_text(0, "E", None);
    server.compare_text("ACDE");
}

#[test]
fn parity_exception_collab_redo_selection() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../oracle/parity-exceptions.json"
    ))
    .unwrap();
    let case = &cases["collab_redo_selection"];
    let mut server = Server::pair();
    server.type_text(0, "A", None);
    server.type_text(1, "B", None);
    server.type_text(0, "C", None);
    server.undo(1);
    let after_undo = server.states[1].doc.text_content();
    server.redo(1);
    let peer = &server.states[1];
    let after_redo = peer.doc.text_content();
    let redo_cursor = peer.selection.head(&peer.doc);
    server.type_text(1, "D", None);
    server.type_text(0, "E", None);
    let final_texts = server.states.iter().map(|peer| peer.doc.text_content()).collect::<Vec<_>>();
    assert_eq!(
        json!({
            "afterUndo": after_undo,
            "afterRedo": after_redo,
            "redoCursor": redo_cursor,
            "finalTexts": final_texts,
        }),
        case["pinned"]
    );
    for text in final_texts {
        assert_ne!(Some(text.as_str()), case["originalFinalText"].as_str());
        server.compare_text(&text);
    }
}

#[test]
fn supports_deep_undo() {
    let mut server = Server::new(Some(doc!(p!("hello"), p!("bye")).node), 2);
    server.select(0, 6);
    server.select(1, 11);
    server.type_text(0, "!", None);
    server.type_text(1, "!", None);
    server.close(0);
    server.delay(0, |server| {
        server.type_text(0, " ...", None);
        server.type_text(1, " ,,,", None);
    });
    server.close(0);
    server.type_text(0, "*", None);
    server.type_text(1, "*", None);
    server.undo(0);
    server.compare(doc!(p!("hello! ..."), p!("bye! ,,,*")).node);
    server.undo(0);
    server.undo(0);
    server.compare(doc!(p!("hello"), p!("bye! ,,,*")).node);
    server.redo(0);
    server.redo(0);
    server.redo(0);
    server.compare(doc!(p!("hello! ...*"), p!("bye! ,,,*")).node);
    server.undo(0);
    server.undo(0);
    server.compare(doc!(p!("hello!"), p!("bye! ,,,*")).node);
    server.undo(1);
    server.compare(doc!(p!("hello!"), p!("bye")).node);
}

#[test]
fn supports_undo_with_clashing_events() {
    let mut server = Server::new(Some(doc!(p!("hello")).node), 2);
    server.select(0, 6);
    server.type_text(0, "A", None);
    server.delay(0, |server| {
        server.type_text(0, "B", Some(4));
        server.type_text(0, "C", Some(5));
        server.type_text(0, "D", Some(1));
        server.delete(1, 2, 5);
    });
    server.compare_text("DhoA");
    server.undo(0);
    server.undo(0);
    server.compare_text("ho");
    assert_eq!(server.states[0].selection.head(&server.states[0].doc), 3);
}

#[test]
fn handles_conflicting_steps() {
    let mut server = Server::new(Some(doc!(p!("abcde")).node), 2);
    server.delay(0, |server| {
        server.delete(0, 3, 4);
        server.type_text(0, "x", None);
        server.delete(1, 2, 5);
    });
    server.undo(0);
    server.undo(0);
    server.compare_text("ae");
}

#[test]
fn can_undo_simultaneous_typing() {
    let mut server = Server::new(Some(doc!(p!("A"), p!("B")).node), 2);
    server.select(0, 2);
    server.select(1, 5);
    server.delay(0, |server| {
        server.type_text(0, "1", None);
        server.type_text(0, "2", None);
        server.type_text(1, "x", None);
        server.type_text(1, "y", None);
    });
    server.compare(doc!(p!("A12"), p!("Bxy")).node);
    server.undo(0);
    server.compare(doc!(p!("A"), p!("Bxy")).node);
    server.undo(1);
    server.compare(doc!(p!("A"), p!("B")).node);
}

#[test]
fn receives_remote_steps_with_backward_selection_and_tracks_origins() {
    let server = Server::pair();
    let state = &server.states[0];
    let step = |text: &str| {
        Step::replace(1, 1, Slice::new(Fragment::from_nodes(vec![test_schema().text(text, None)]), 0, 0))
    };
    let remote = step("R");
    let tr = receive_transaction(
        state, std::slice::from_ref(&remote), &[ClientId::Number(1)],
        ReceiveOptions { map_selection_backward: true },
    ).unwrap();
    assert_eq!(tr.selection(), Selection::cursor(1));
    assert!(!tr.selection_set());

    let mut local = state.tr();
    let typed_metadata = Arc::new("typed metadata".to_owned());
    local.insert_text_at_selection("a").insert_text_at_selection("b")
        .set_time(101).set_meta("origin", json!("preserved"))
        .set_typed_meta("typedOrigin", Arc::clone(&typed_metadata));
    let edited = state.apply_transaction(&local);
    let original = &edited.transactions[0];
    let before_rebase = sendable_steps(&edited.state).unwrap().unwrap();
    assert!(Arc::ptr_eq(original, &before_rebase.origins[0]));
    let receive = receive_transaction(
        &edited.state, &[remote], &[ClientId::Number(1)], ReceiveOptions::default()
    ).unwrap();
    let received = edited.state.apply(&receive);
    let pending = sendable_steps(&received).unwrap().unwrap();
    assert_eq!(pending.version, 1);
    assert_eq!(pending.steps.len(), 2);
    assert!(Arc::ptr_eq(original, &pending.origins[0]));
    assert!(Arc::ptr_eq(&pending.origins[0], &pending.origins[1]));
    assert_eq!(pending.origins[0].time, 101);
    assert_eq!(pending.origins[0].doc(), &doc!(p!("ab")).node);
    assert_eq!(pending.origins[0].get_meta("origin"), Some(&json!("preserved")));
    assert!(Arc::ptr_eq(
        pending.origins[0].get_typed_meta::<Arc<String>>("typedOrigin").unwrap(), &typed_metadata,
    ));
}

#[test]
fn restores_serialized_collab_steps_and_origin_metadata() {
    let plugin = collab(CollabConfig::default());
    let state = EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: None,
        selection: None,
        stored_marks: None,
        plugins: vec![plugin.clone()],
    });
    let mut tr = state.tr();
    tr.insert_text_at_selection("a").insert_text_at_selection("b").set_meta("data", json!("keep"));
    let state = state.apply(&tr);
    let fields = [("collab", &plugin)];
    let json = state.to_json_with_plugins(&fields).unwrap();
    let restored = EditorState::from_json_with_plugins(
        &state.schema, &json, vec![plugin.clone()], &fields,
    ).unwrap();
    let pending = sendable_steps(&restored).unwrap().unwrap();
    assert_eq!(pending.steps.as_slice(), tr.steps());
    assert!(Arc::ptr_eq(&pending.origins[0], &pending.origins[1]));
    assert_eq!(pending.origins[0].get_meta("data"), Some(&json!("keep")));
}
