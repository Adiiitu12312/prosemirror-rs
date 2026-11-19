use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use prosemirror_state::{EditorState, EditorStateConfig, Plugin, Selection};
use prosemirror_test_builder::{doc, p, test_schema};
use prosemirror_view::{ClipboardOptions, EditorProps, EditorView, LayoutRect, ViewFrame, ViewHost};

#[derive(Default)]
struct HostState {
    renders: usize,
    scrolls: usize,
    destroyed: usize,
    focused: bool,
}

struct Host(Rc<RefCell<HostState>>);

impl ViewHost for Host {
    fn render(&mut self, _: ViewFrame<'_>) {
        self.0.borrow_mut().renders += 1;
    }

    fn has_focus(&self) -> bool {
        self.0.borrow().focused
    }

    fn focus(&mut self) {
        self.0.borrow_mut().focused = true;
    }

    fn scroll_to_selection(&mut self, _: LayoutRect) {
        self.0.borrow_mut().scrolls += 1;
    }

    fn destroy(&mut self) {
        self.0.borrow_mut().destroyed += 1;
    }
}

fn state(plugins: Vec<Plugin>) -> EditorState {
    EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc!(p!("hello")).node),
        selection: Some(Selection::cursor(6)),
        stored_marks: None,
        plugins,
    })
}

#[test]
fn applies_edits_renders_and_cleans_up_the_host() {
    let host = Rc::new(RefCell::new(HostState::default()));
    let mut view = EditorView::new(Host(Rc::clone(&host)), state(Vec::new()), EditorProps::default());
    assert_eq!(host.borrow().renders, 1);
    view.focus();
    assert!(view.has_focus());
    assert!(view.insert_text(" world"));
    assert_eq!(view.state().doc.text_between(1, 12), "hello world");
    assert_eq!(host.borrow().renders, 2);
    assert_eq!(host.borrow().scrolls, 1);
    view.destroy();
    assert!(view.is_destroyed());
    assert!(!view.insert_text("!"));
    drop(view);
    assert_eq!(host.borrow().destroyed, 1);
}

#[test]
fn dispatches_pastes_with_metadata_and_respects_editability() {
    let host = Rc::new(RefCell::new(HostState::default()));
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&events);
    let props = EditorProps {
        dispatch_transaction: Some(Box::new(move |state, tr| {
            recorded.borrow_mut().push(tr.get_meta("uiEvent").cloned());
            Some(state.apply(&tr))
        })),
        ..EditorProps::default()
    };
    let mut view = EditorView::new(Host(host), state(Vec::new()), props);
    assert!(view.paste(" world", None, true, &ClipboardOptions::default()).unwrap());
    assert_eq!(view.state().doc.text_between(1, 12), "hello world");
    assert_eq!(events.borrow().as_slice(), [Some(serde_json::json!("paste"))]);

    view.update(view.state().clone(), EditorProps { editable: Some(Arc::new(|_| false)), ..EditorProps::default() });
    assert!(!view.insert_text("!"));
    assert!(!view.paste("!", None, true, &ClipboardOptions::default()).unwrap());
}

#[test]
fn applies_composition_updates_and_commits_through_the_view() {
    let host = Rc::new(RefCell::new(HostState::default()));
    let mut view = EditorView::new(Host(host), state(Vec::new()), EditorProps::default());
    view.composition_start();
    assert!(view.composing());
    assert!(view.composition_update("nihon"));
    assert_eq!(view.state().doc.text_between(1, 11), "hellonihon");
    assert!(view.composition_end("日本"));
    assert!(!view.composing());
    assert_eq!(view.state().doc.text_between(1, 8), "hello日本");
}
