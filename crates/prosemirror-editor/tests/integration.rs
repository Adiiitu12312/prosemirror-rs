//! Native editor integrations exercised without a window or toolkit.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use prosemirror_editor::{
    BlockUpdate, ClipboardContent, ClipboardEvent, CompositionEvent, Decoration, DecorationGroup,
    DecorationSet, DisplayList, DragEvent, Editor, EditorProps, Event, EventAction, Host, KeyInput,
    Layout, Modifiers, NodeView, PluginProps, PluginView, Point, PointerButton, PointerEvent,
    PointerInput, PositionAtCoords, Rect, ViewMutation,
};
use prosemirror_keymap::Platform;
use prosemirror_model::{Fragment, Node, Slice};
use prosemirror_state::{EditorState, EditorStateConfig, Plugin, PluginKey, Selection};
use prosemirror_test_builder::{doc, p, test_schema};
use prosemirror_view::AccessibilityTreeUpdate;
use serde_json::{json, Value};

#[derive(Default)]
struct MockHost {
    layout_updates: usize,
    draws: usize,
    frame: Option<DisplayList>,
    changed: BlockUpdate,
    focus_requests: usize,
    caret: Option<Rect>,
    scroll: Vec<Point>,
    clipboard: Option<ClipboardContent>,
    accessibility: Option<AccessibilityTreeUpdate>,
}

impl Layout for MockHost {
    fn coords_at_pos(&self, pos: usize, _side: i32) -> Option<Rect> {
        let x = pos as f64;
        Some(Rect { left: x, top: 0.0, right: x, bottom: 16.0 })
    }

    fn pos_at_coords(&self, point: Point) -> Option<PositionAtCoords> {
        Some(PositionAtCoords { pos: point.x.round() as usize, inside: None })
    }
}

impl Host for MockHost {
    fn update_layout(&mut self, _: &DisplayList, _: &BlockUpdate) {
        self.layout_updates += 1;
    }

    fn draw(&mut self, frame: &DisplayList, changed: &BlockUpdate) {
        self.draws += 1;
        self.frame = Some(frame.clone());
        self.changed = changed.clone();
    }

    fn request_focus(&mut self) {
        self.focus_requests += 1;
    }

    fn set_ime_candidate_rect(&mut self, rect: Option<Rect>) {
        self.caret = rect;
    }

    fn viewport(&self) -> Option<Rect> {
        Some(Rect { left: 0.0, top: 0.0, right: 5.0, bottom: 20.0 })
    }

    fn scroll_by(&mut self, delta: Point) {
        self.scroll.push(delta);
    }

    fn write_clipboard(&mut self, content: &ClipboardContent) {
        self.clipboard = Some(content.clone());
    }

    fn read_clipboard(&mut self) -> Option<ClipboardContent> {
        self.clipboard.clone()
    }

    fn update_accessibility(&mut self, update: &AccessibilityTreeUpdate) {
        self.accessibility = Some(update.clone());
    }
}

fn state(doc: Node, plugins: Vec<Plugin>) -> EditorState {
    EditorState::create(EditorStateConfig {
        schema: test_schema().clone(),
        doc: Some(doc),
        selection: Some(Selection::cursor(1)),
        stored_marks: None,
        plugins,
    })
}

fn editor(state: EditorState, props: EditorProps) -> Editor<MockHost> {
    Editor::with_platform(state, MockHost::default(), props, Platform::Other)
}

fn select(editor: &mut Editor<MockHost>, selection: Selection) {
    let mut transaction = editor.state().tr();
    transaction.set_selection(selection);
    assert!(editor.dispatch(transaction).unwrap());
}

fn point(pos: usize) -> Point {
    Point { x: pos as f64, y: 0.0 }
}

fn pointer(pos: usize) -> PointerInput {
    PointerInput {
        point: point(pos), button: PointerButton::Primary, click_count: 1, modifiers: Modifiers::default(),
    }
}

fn key(key: &str) -> Event {
    Event::KeyDown(KeyInput { key: key.into(), code: None, modifiers: Modifiers::default(), repeating: false })
}

#[test]
fn queries_direct_view_and_state_props_in_precedence_order() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let provider = |name: &'static str, id| {
        let log = log.clone();
        PluginProps {
            attributes: Some(Rc::new(move |_| {
                [
                    ("layer".into(), name.into()),
                    (name.into(), "present".into()),
                    ("class".into(), format!("layer-{name}")),
                    ("style".into(), format!("--layer-{name}:{id}")),
                    ("first-nonempty".into(), if name == "direct" { "" } else { name }.into()),
                    ("contenteditable".into(), "false".into()),
                    ("nodeName".into(), "span".into()),
                ].into()
            })),
            decorations: Some(Rc::new(move |state| DecorationGroup {
                members: vec![DecorationSet::create(&state.doc, vec![Decoration::widget(1, id, Value::Null)])],
            })),
            handle_key_down: Some(Rc::new(move |_, event| {
                log.borrow_mut().push(name);
                if event.key == name { EventAction::Prevent } else { EventAction::Continue }
            })),
            ..PluginProps::default()
        }
    };
    let plugin_key = PluginKey::new("state-layer");
    let props = EditorProps {
        direct: provider("direct", 1),
        view_plugins: vec![Rc::new(provider("view", 2))],
        state_plugins: [(plugin_key.clone(), Rc::new(provider("state", 3)))].into(),
        ..EditorProps::default()
    };
    let mut editor = editor(state(doc!(p!("abc")).node, vec![Plugin::new(plugin_key, None)]), props);
    assert_eq!(editor.attributes()["layer"], "direct");
    assert_eq!(editor.attributes()["view"], "present");
    assert_eq!(editor.attributes()["state"], "present");
    assert_eq!(editor.attributes()["class"], "layer-direct layer-view layer-state");
    assert_eq!(editor.attributes()["style"], "--layer-direct:1;--layer-view:2;--layer-state:3");
    assert_eq!(editor.attributes()["first-nonempty"], "view");
    assert!(!editor.attributes().contains_key("contenteditable"));
    assert!(!editor.attributes().contains_key("nodeName"));
    let frame = editor.host().frame.as_ref().unwrap();
    assert_eq!(&frame.attributes, editor.attributes());
    assert!(frame.editable);
    let ids: Vec<_> = frame.blocks[0].children.iter().filter_map(|item| match item {
        prosemirror_editor::DisplayItem::Widget(widget) => Some(widget.id),
        _ => None,
    }).collect();
    assert_eq!(ids, [1, 2, 3]);

    assert!(editor.handle(key("direct")).unwrap());
    assert_eq!(*log.borrow(), ["direct"]);
    log.borrow_mut().clear();
    assert!(editor.handle(key("view")).unwrap());
    assert_eq!(*log.borrow(), ["direct", "view"]);
    log.borrow_mut().clear();
    assert!(editor.handle(key("state")).unwrap());
    assert_eq!(*log.borrow(), ["direct", "view", "state"]);

    editor.set_props(EditorProps {
        direct: PluginProps { editable: Some(Rc::new(|_| true)), ..PluginProps::default() },
        view_plugins: vec![Rc::new(PluginProps {
            editable: Some(Rc::new(|_| false)),
            ..PluginProps::default()
        })],
        ..EditorProps::default()
    });
    assert!(!editor.editable());
    assert!(!editor.host().frame.as_ref().unwrap().editable);
    assert!(!editor.handle(Event::TextInput("x".into())).unwrap());
    assert_eq!(editor.state().doc.text_content(), "abc");
}

#[test]
fn respects_filtered_transactions_and_renders_the_appended_result_once() {
    let plugin = Plugin::new(PluginKey::new("normalize"), None)
        .with_filter_transaction(|transaction, _| transaction.get_meta("reject").is_none())
        .with_append_transaction(|transactions, _, next| {
            if !transactions.iter().any(|transaction| transaction.get_meta("append-punctuation").is_some()) {
                return None;
            }
            let mut tr = next.tr();
            tr.insert_text_at_selection("!");
            Some(tr)
        });
    let mut editor = editor(state(doc!(p!("a")).node, vec![plugin]), EditorProps::default());
    let original_draws = editor.host().draws;
    let mut rejected = editor.state().tr();
    rejected.insert_text_at_selection("x").set_meta("reject", json!(true));
    assert!(!editor.dispatch(rejected).unwrap());
    assert_eq!(editor.state().doc.text_content(), "a");
    assert_eq!(editor.host().draws, original_draws);

    let mut accepted = editor.state().tr();
    accepted.insert_text_at_selection("x").set_meta("append-punctuation", json!(true));
    assert!(editor.dispatch(accepted).unwrap());
    assert_eq!(editor.state().doc.text_content(), "x!a");
    assert_eq!(editor.host().draws, original_draws + 1);
    assert_eq!(editor.host().changed, BlockUpdate { previous: 0..1, current: 0..1 });
}

struct PluginTrace {
    name: &'static str,
    log: Rc<RefCell<Vec<String>>>,
}

impl PluginView for PluginTrace {
    fn update(&mut self, state: &EditorState, previous: &EditorState) {
        self.log.borrow_mut().push(format!("update {} {} → {}", self.name, previous.doc.text_content(), state.doc.text_content()));
    }

    fn destroy(&mut self) {
        self.log.borrow_mut().push(format!("destroy {}", self.name));
    }
}

#[test]
fn updates_and_recreates_direct_and_state_plugin_views_in_order() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let provider = |name: &'static str| {
        let log = log.clone();
        PluginProps {
            plugin_view: Some(Rc::new(move |_| {
                log.borrow_mut().push(format!("create {name}"));
                Box::new(PluginTrace { name, log: log.clone() })
            })),
            ..PluginProps::default()
        }
    };
    let plugin_key = PluginKey::new("state-view");
    let props = EditorProps {
        direct: provider("direct"),
        view_plugins: vec![Rc::new(provider("view"))],
        state_plugins: [(plugin_key.clone(), Rc::new(provider("state")))].into(),
        ..EditorProps::default()
    };
    let mut editor = editor(state(doc!(p!("a")).node, vec![Plugin::new(plugin_key, None)]), props);
    assert_eq!(*log.borrow(), ["create direct", "create view", "create state"]);
    log.borrow_mut().clear();
    assert!(editor.handle(Event::TextInput("x".into())).unwrap());
    assert_eq!(*log.borrow(), ["update direct a → xa", "update view a → xa", "update state a → xa"]);

    log.borrow_mut().clear();
    editor.update_state(editor.state().reconfigure(Vec::new()).unwrap());
    assert_eq!(*log.borrow(), ["destroy state", "destroy view", "destroy direct", "create direct", "create view"]);
    log.borrow_mut().clear();
    drop(editor);
    assert_eq!(*log.borrow(), ["destroy view", "destroy direct"]);
}

struct NodeTrace(Rc<RefCell<Vec<String>>>);

impl NodeView for NodeTrace {
    fn update(&mut self, node: &Node, pos: usize, _decorations: &DecorationGroup) -> bool {
        assert_eq!(node.type_name(), "image");
        self.0.borrow_mut().push(format!("update {pos}"));
        true
    }

    fn select(&mut self) {
        self.0.borrow_mut().push("select".into());
    }

    fn deselect(&mut self) {
        self.0.borrow_mut().push("deselect".into());
    }

    fn stop_event(&self, event: &Event) -> bool {
        if matches!(event, Event::Pointer(PointerEvent::Down(_))) {
            self.0.borrow_mut().push("stop".into());
            return true;
        }
        false
    }

    fn ignore_mutation(&self, mutation: ViewMutation) -> bool {
        mutation == ViewMutation::Layout
    }

    fn destroy(&mut self) {
        self.0.borrow_mut().push("destroy".into());
    }
}

#[test]
fn reconciles_native_node_views_and_routes_their_events() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let sink = log.clone();
    let decoration_mode = Rc::new(Cell::new(0));
    let mode = decoration_mode.clone();
    let direct = PluginProps {
        node_views: [("image".into(), Rc::new(move |_: &Node, pos, _: &DecorationGroup| {
            sink.borrow_mut().push(format!("create {pos}"));
            Box::new(NodeTrace(sink.clone())) as Box<dyn NodeView>
        }) as prosemirror_editor::MakeNodeView)].into(),
        decorations: Some(Rc::new(move |state| DecorationGroup {
            members: vec![DecorationSet::create(&state.doc, match mode.get() {
                1 => vec![Decoration::widget(6, 1, Value::Null)],
                2 => vec![Decoration::node(2, 3, [("selected".into(), "yes".into())].into(), Value::Null)],
                _ => Vec::new(),
            })],
        })),
        ..PluginProps::default()
    };
    let props = EditorProps { direct, ..EditorProps::default() };
    let image = test_schema().node("image", Some(json!({"src":"sample.png"})), None, None).unwrap();
    let mut editor = editor(state(doc!(p!("a", image, "b"), p!("next")).node, Vec::new()), props.clone());
    assert_eq!(*log.borrow(), ["create 2"]);
    assert!(editor.ignores_mutation(2, ViewMutation::Layout));
    assert!(!editor.ignores_mutation(2, ViewMutation::Content));
    assert!(!editor.ignores_mutation(1, ViewMutation::Layout));
    assert!(!editor.handle(Event::Pointer(PointerEvent::Down(pointer(2)))).unwrap());
    assert_eq!(editor.host().focus_requests, 0);
    assert_eq!(*log.borrow(), ["create 2", "stop"]);

    decoration_mode.set(1);
    editor.set_props(props.clone());
    assert_eq!(*log.borrow(), ["create 2", "stop"]);
    decoration_mode.set(2);
    editor.set_props(props);
    assert_eq!(*log.borrow(), ["create 2", "stop", "update 2"]);
    select(&mut editor, Selection::node(2));
    let mut insert = editor.state().tr();
    insert.replace_with(1, 1, Fragment::from_nodes(vec![test_schema().text("x", None)]));
    editor.dispatch(insert).unwrap();
    assert_eq!(editor.state().selection, Selection::node(3));
    select(&mut editor, Selection::cursor(1));
    let mut remove = editor.state().tr();
    remove.delete(3, 4);
    editor.dispatch(remove).unwrap();
    assert_eq!(*log.borrow(), ["create 2", "stop", "update 2", "select", "update 3", "deselect", "destroy"]);
}

#[test]
fn reports_focus_candidate_geometry_scrolling_and_clipboard_to_the_host() {
    let mut editor = editor(state(doc!(p!("abcdefgh")).node, Vec::new()), EditorProps::default());
    assert_eq!(editor.host().caret, None);
    editor.handle(Event::Focus).unwrap();
    assert!(editor.has_focus());
    assert_eq!(editor.host().caret.unwrap().left, 1.0);
    editor.handle(Event::Pointer(PointerEvent::Down(pointer(6)))).unwrap();
    assert_eq!(editor.host().focus_requests, 1);
    assert_eq!(editor.host().caret.unwrap().left, 6.0);
    editor.handle(Event::Composition(CompositionEvent::Start)).unwrap();
    assert!(editor.host().frame.as_ref().unwrap().composition.is_some());
    editor.handle(Event::Composition(CompositionEvent::Cancel)).unwrap();
    assert_eq!(editor.host().frame.as_ref().unwrap().composition, None);

    select(&mut editor, Selection::text(2, 5));
    assert!(editor.handle(Event::Clipboard(ClipboardEvent::Copy)).unwrap());
    assert_eq!(editor.host().clipboard.as_ref().unwrap().plain_text, "bcd");
    assert!(editor.host().clipboard.as_ref().unwrap().internal.is_some());
    editor.handle(Event::Scroll(Point { x: 1.0, y: 2.0 })).unwrap();
    assert_eq!(editor.host().scroll, [Point { x: 1.0, y: 2.0 }]);
    let accessibility = editor.host().accessibility.as_ref().unwrap();
    assert_ne!(accessibility.focus_id, accessibility.root_id);
    let layouts = editor.host().layout_updates;
    editor.handle(Event::Blur).unwrap();
    assert!(!editor.has_focus());
    assert!(!editor.host().frame.as_ref().unwrap().focused);
    assert_eq!(editor.host().caret, None);
    assert_eq!(editor.host().layout_updates, layouts);
}

fn slice_text(slice: &Slice) -> String {
    slice.content.text_between(0, slice.content.size(), Some("\n"), None)
}

fn new_text_slice(state: &EditorState, text: &str) -> Slice {
    Slice::new(Fragment::from_nodes(vec![state.schema.text(text, None)]), 0, 0)
}

#[test]
fn routes_transformed_paste_and_clears_a_prevented_drag_preview() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let direct_transform = calls.clone();
    let view_transform = calls.clone();
    let direct_paste = calls.clone();
    let view_paste = calls.clone();
    let drop_hook = calls.clone();
    let direct = PluginProps {
        transform_pasted: Some(Rc::new(move |state, slice, plain| {
            direct_transform.borrow_mut().push("transform direct");
            if !plain { return slice }
            assert_eq!(slice_text(&slice), "incoming");
            new_text_slice(state, "intermediate")
        })),
        handle_paste: Some(Rc::new(move |_, slice, plain| {
            assert!(plain);
            assert_eq!(slice_text(slice), "native");
            direct_paste.borrow_mut().push("paste direct");
            EventAction::Continue
        })),
        handle_drop: Some(Rc::new(move |_, slice, pos, moved| {
            assert!(moved);
            assert_eq!(pos, 9);
            assert_eq!(slice_text(slice), "native");
            drop_hook.borrow_mut().push("drop direct");
            EventAction::Prevent
        })),
        ..PluginProps::default()
    };
    let view = PluginProps {
        transform_pasted: Some(Rc::new(move |state, slice, plain| {
            view_transform.borrow_mut().push("transform view");
            if !plain { return slice }
            assert_eq!(slice_text(&slice), "intermediate");
            new_text_slice(state, "native")
        })),
        handle_paste: Some(Rc::new(move |state, slice, _| {
            view_paste.borrow_mut().push("paste view");
            let mut tr = state.tr();
            tr.replace_selection(slice.clone());
            EventAction::transaction(tr)
        })),
        ..PluginProps::default()
    };
    let props = EditorProps { direct, view_plugins: vec![Rc::new(view)], ..EditorProps::default() };
    let mut editor = editor(state(doc!(p!("abc")).node, Vec::new()), props);
    assert!(editor.handle(Event::Clipboard(ClipboardEvent::PasteContent {
        content: ClipboardContent { plain_text: "incoming".into(), ..ClipboardContent::default() },
        plain_text: true,
    })).unwrap());
    assert_eq!(editor.state().doc.text_content(), "nativeabc");
    assert_eq!(*calls.borrow(), ["transform direct", "transform view", "paste direct", "paste view"]);
    calls.borrow_mut().clear();

    select(&mut editor, Selection::text(1, 7));
    assert!(editor.handle(Event::Drag(DragEvent::Start)).unwrap());
    assert!(editor.handle(Event::Drag(DragEvent::Over(point(9)))).unwrap());
    assert!(editor.host().frame.as_ref().unwrap().drag.is_some());
    assert!(editor.handle(Event::Drag(DragEvent::Drop { point: point(9), slice: None, copy: false })).unwrap());
    assert_eq!(editor.state().doc.text_content(), "nativeabc");
    assert_eq!(*calls.borrow(), ["transform direct", "transform view", "drop direct"]);
    assert!(editor.host().frame.as_ref().unwrap().drag.is_none());
}

#[test]
fn clears_drag_preview_when_dispatch_filters_or_intercepts_drop() {
    let filter_called = Arc::new(AtomicBool::new(false));
    let seen = filter_called.clone();
    let filter = Plugin::new(PluginKey::new("filter-drop"), None)
        .with_filter_transaction(move |tr, _| {
            if tr.get_meta("uiEvent").and_then(Value::as_str) == Some("drop") {
                seen.store(true, Ordering::Relaxed);
                return false;
            }
            true
        });
    let filtered = editor(state(doc!(p!("one two")).node, vec![filter]), EditorProps::default());

    let dispatch_called = Arc::new(AtomicBool::new(false));
    let seen = dispatch_called.clone();
    let intercepted = editor(state(doc!(p!("one two")).node, Vec::new()), EditorProps {
        dispatch_transaction: Some(Rc::new(move |state, tr| {
            if tr.get_meta("uiEvent").and_then(Value::as_str) == Some("drop") {
                seen.store(true, Ordering::Relaxed);
                return None;
            }
            Some(state.apply(&tr))
        })),
        ..EditorProps::default()
    });

    for (mut editor, handled, called) in [(filtered, false, filter_called), (intercepted, true, dispatch_called)] {
        select(&mut editor, Selection::text(1, 4));
        assert!(editor.handle(Event::Drag(DragEvent::Start)).unwrap());
        assert!(editor.handle(Event::Drag(DragEvent::Over(point(8)))).unwrap());
        assert!(editor.host().frame.as_ref().unwrap().drag.is_some());

        let before = editor.host().draws;
        let event = Event::Drag(DragEvent::Drop { point: point(8), slice: None, copy: false });
        assert_eq!(editor.handle(event).unwrap(), handled);
        assert!(called.load(Ordering::Relaxed));
        assert_eq!(editor.state().doc.text_content(), "one two");
        assert_eq!(editor.state().selection, Selection::text(1, 4));
        assert_eq!(editor.host().draws, before + 1);
        assert!(editor.host().frame.as_ref().unwrap().drag.is_none());
    }
}
