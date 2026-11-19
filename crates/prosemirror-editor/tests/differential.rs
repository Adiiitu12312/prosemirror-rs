//! Native controller against the pinned upstream ProseMirror model in Node.

use std::cell::RefCell;
use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::sync::OnceLock;

use prosemirror_commands::{set_block_type, toggle_mark, wrap_in};
use prosemirror_editor::{
    BlockUpdate, ClipboardContent, ClipboardEvent, CompositionEvent, Decoration, DecorationGroup,
    DecorationSet, DisplayList, Editor, EditorProps, Event, EventAction, Host, KeyInput, Layout,
    Modifiers, Point, PositionAtCoords, Rect, TouchEvent,
};
use prosemirror_history::{close_history, history, redo, undo, undo_depth, HistoryOptions};
use prosemirror_inputrules::{ellipsis, em_dash, input_rules, smart_quotes, undo_input_rule};
use prosemirror_keymap::Platform;
use prosemirror_state::{EditorState, EditorStateConfig, Selection, Transaction};
use prosemirror_test_builder::test_schema;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    name: String,
    initial: Value,
    selection: Value,
    spans: Vec<Span>,
    steps: Vec<Checkpoint>,
    undo_restores: bool,
}

#[derive(Debug, Deserialize)]
struct Checkpoint {
    event: Action,
    expected: Value,
}

#[derive(Debug, Deserialize)]
struct Span {
    kind: SpanKind,
    id: u64,
    from: usize,
    #[serde(default)]
    to: usize,
    #[serde(default)]
    side: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SpanKind {
    Inline,
    Node,
    Widget,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Action {
    Text { text: String },
    Select { anchor: usize, head: usize },
    NodeSelection { pos: usize },
    Key { key: String, control: bool, shift: bool },
    Paste { text: String },
    Clipboard { action: ClipboardAction },
    Composition { phase: CompositionPhase, text: String, from: usize, to: usize },
    Touch { phase: TouchPhase, pos: usize },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ClipboardAction {
    Copy,
    Cut,
    Paste,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CompositionPhase {
    Start,
    Update,
    Commit,
    Cancel,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TouchPhase {
    LongPress,
    DragStart,
    Drag,
    DragEnd,
}

#[derive(Default)]
struct ReplayHost {
    clipboard: Option<ClipboardContent>,
    frame: Option<DisplayList>,
}

impl Layout for ReplayHost {
    fn coords_at_pos(&self, pos: usize, _side: i32) -> Option<Rect> {
        let x = pos as f64;
        Some(Rect { left: x, top: 0.0, right: x, bottom: 16.0 })
    }

    fn pos_at_coords(&self, point: Point) -> Option<PositionAtCoords> {
        Some(PositionAtCoords { pos: point.x.round() as usize, inside: None })
    }
}

impl Host for ReplayHost {
    fn draw(&mut self, frame: &DisplayList, _: &BlockUpdate) {
        self.frame = Some(frame.clone());
    }

    fn read_clipboard(&mut self) -> Option<ClipboardContent> {
        self.clipboard.clone()
    }

    fn write_clipboard(&mut self, content: &ClipboardContent) {
        self.clipboard = Some(content.clone());
    }
}

struct Ledger {
    tick: u64,
    decorations: DecorationSet,
    transactions: Vec<Value>,
}

#[test]
fn scripted_editor_events_match_upstream_transactions_and_state() {
    for fixture in fixtures().iter().filter(|fixture| !fixture.undo_restores) {
        replay(fixture);
    }
}

#[test]
fn seeded_editor_sequences_match_upstream_and_undo_to_the_initial_document() {
    for fixture in fixtures().iter().filter(|fixture| fixture.undo_restores) {
        replay(fixture);
    }
}

fn fixtures() -> &'static [Fixture] {
    static FIXTURES: OnceLock<Vec<Fixture>> = OnceLock::new();
    FIXTURES.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let oracle = root.join("oracle/editor_oracle.mjs");
        let output = Command::new("node").arg(oracle).current_dir(root)
            .output().expect("Node is required for the upstream editor differential tests");
        assert!(output.status.success(), "the upstream oracle failed: {}", String::from_utf8_lossy(&output.stderr));
        serde_json::from_slice(&output.stdout).expect("the upstream oracle must emit editor fixtures")
    })
}

fn replay(fixture: &Fixture) {
    let schema = test_schema();
    let doc = schema.node_from_json(&fixture.initial).unwrap();
    let selection = Selection::from_json(&doc, &fixture.selection).unwrap();
    let mut rules = vec![em_dash(), ellipsis()];
    rules.extend(smart_quotes());
    let state = EditorState::create(EditorStateConfig {
        schema: schema.clone(),
        doc: Some(doc.clone()),
        selection: Some(selection),
        stored_marks: None,
        plugins: vec![input_rules(rules), history(HistoryOptions { depth: 1000, ..HistoryOptions::default() })],
    });
    let initial_decorations = DecorationSet::create(&doc, fixture.spans.iter().map(make_span).collect());
    let ledger = Rc::new(RefCell::new(Ledger { tick: 1000, decorations: initial_decorations, transactions: Vec::new() }));
    let mut props = EditorProps::default();
    props.direct.handle_key_down = Some(Rc::new(shortcuts));
    let for_draw = ledger.clone();
    props.direct.decorations = Some(Rc::new(move |_| DecorationGroup {
        members: vec![for_draw.borrow().decorations.clone()],
    }));
    let for_dispatch = ledger.clone();
    props.dispatch_transaction = Some(Rc::new(move |state, mut tr| {
        let mut ledger = for_dispatch.borrow_mut();
        ledger.tick += 1;
        tr.set_time(ledger.tick);
        close_history(&mut tr);
        let applied = state.apply_transaction(&tr);
        for tr in applied.transactions {
            if tr.doc_changed() {
                ledger.decorations = ledger.decorations.map(tr.mapping(), tr.doc());
            }
            ledger.transactions.push(normalized_transaction(&tr));
        }
        Some(applied.state)
    }));
    let mut editor = Editor::with_platform(state, ReplayHost::default(), props, Platform::Other);

    for (index, checkpoint) in fixture.steps.iter().enumerate() {
        ledger.borrow_mut().transactions.clear();
        run(&mut editor, &checkpoint.event);
        let state = editor.state();
        state.doc.check().unwrap_or_else(|error| {
            panic!("{} event {index} ({:?}) created invalid content: {error}", fixture.name, checkpoint.event);
        });
        for range in state.selection.ranges(&state.doc).unwrap() {
            assert!(range.from.pos <= range.to.pos && range.to.pos <= state.doc.content().size());
        }
        let transactions = ledger.borrow();
        let value = snapshot(&editor, &checkpoint.event, &transactions.transactions);
        assert_eq!(value, checkpoint.expected, "{} event {index}: {:?}", fixture.name, checkpoint.event);
    }
    if fixture.undo_restores {
        assert_eq!(undo_depth(editor.state()), 0, "{}", fixture.name);
        assert_eq!(editor.state().doc, doc, "{}", fixture.name);
    }
}

fn make_span(span: &Span) -> Decoration {
    let attrs = [("role".into(), "review".into())].into();
    match span.kind {
        SpanKind::Inline => Decoration::inline(span.from, span.to, attrs, json!({"id":span.id,"inclusiveStart":true})),
        SpanKind::Node => Decoration::node(span.from, span.to, attrs, json!({"id":span.id})),
        SpanKind::Widget => Decoration::widget(span.from, span.id, json!({"id":span.id,"side":span.side})),
    }
}

fn shortcuts(state: &EditorState, event: &KeyInput) -> EventAction {
    let mut pending = None;
    let mut dispatch = |tr| pending = Some(tr);
    let handled = if event.modifiers.control {
        match event.key.as_str() {
            "b" => toggle_mark("strong", None)(state, Some(&mut dispatch)),
            "i" => toggle_mark("em", None)(state, Some(&mut dispatch)),
            "0" => set_block_type("paragraph", None)(state, Some(&mut dispatch)),
            "2" => set_block_type("heading", Some(json!({"level":2})))(state, Some(&mut dispatch)),
            "q" => wrap_in("blockquote", None)(state, Some(&mut dispatch)),
            "z" if event.modifiers.shift => redo(state, &mut dispatch),
            "z" => undo(state, &mut dispatch),
            _ => false,
        }
    } else {
        event.key == "Backspace" && undo_input_rule(state, &mut dispatch)
    };
    match pending {
        Some(tr) => EventAction::transaction(tr),
        None if handled => EventAction::Prevent,
        None => EventAction::Continue,
    }
}

fn set_selection(editor: &mut Editor<ReplayHost>, selection: Selection) {
    if editor.state().selection != selection {
        let mut tr = editor.state().tr();
        tr.set_selection(selection);
        editor.dispatch(tr).unwrap();
    }
}

fn run(editor: &mut Editor<ReplayHost>, action: &Action) {
    let event = match action {
        Action::Text { text } => Event::TextInput(text.clone()),
        Action::Select { anchor, head } => {
            let doc = &editor.state().doc;
            let selection = Selection::between(&doc.resolve(*anchor).unwrap(), &doc.resolve(*head).unwrap(), 1);
            set_selection(editor, selection);
            return;
        }
        Action::NodeSelection { pos } => {
            set_selection(editor, Selection::node(*pos));
            return;
        }
        Action::Key { key, control, shift } => Event::KeyDown(KeyInput {
            key: key.clone(), code: None, modifiers: Modifiers { control: *control, shift: *shift, ..Default::default() },
            repeating: false,
        }),
        Action::Paste { text } => Event::Clipboard(ClipboardEvent::PasteContent {
            content: ClipboardContent { plain_text: text.clone(), internal: None },
            plain_text: true,
        }),
        Action::Clipboard { action } => Event::Clipboard(match action {
            ClipboardAction::Copy => ClipboardEvent::Copy,
            ClipboardAction::Cut => ClipboardEvent::Cut,
            ClipboardAction::Paste => ClipboardEvent::Paste,
        }),
        Action::Composition { phase, text, from, to } => Event::Composition(match phase {
            CompositionPhase::Start => CompositionEvent::Start,
            CompositionPhase::Update => CompositionEvent::Update { text: text.clone(), selection: *from..*to },
            CompositionPhase::Commit => CompositionEvent::Commit(text.clone()),
            CompositionPhase::Cancel => CompositionEvent::Cancel,
        }),
        Action::Touch { phase, pos } => {
            let point = Point { x: *pos as f64, y: 0.0 };
            Event::Touch(match phase {
                TouchPhase::LongPress => TouchEvent::LongPress(point),
                TouchPhase::DragStart => TouchEvent::DragStart(point),
                TouchPhase::Drag => TouchEvent::Drag(point),
                TouchPhase::DragEnd => TouchEvent::DragEnd(point),
            })
        }
    };
    editor.handle(event).unwrap();
    if let Action::Composition { phase, text, from, to } = action {
        let composition = &editor.host().frame.as_ref().unwrap().composition;
        match phase {
            CompositionPhase::Start => assert!(composition.is_some()),
            CompositionPhase::Update => {
                let composition = composition.as_ref().unwrap();
                assert_eq!(&composition.text, text);
                assert_eq!(composition.selection, *from..*to);
            }
            CompositionPhase::Commit | CompositionPhase::Cancel => assert!(composition.is_none()),
        }
    }
    if matches!(action, Action::Touch { .. }) {
        assert!(editor.host().frame.as_ref().unwrap().touch_handles);
    }
}

fn normalized_transaction(tr: &Transaction) -> Value {
    json!({
        "steps": tr.steps(),
        "selection": tr.selection(),
        "selectionSet": tr.selection_set(),
        "composition": tr.get_meta("composition"),
        "pointer": tr.get_meta("pointer"),
    })
}

fn snapshot(editor: &Editor<ReplayHost>, action: &Action, transactions: &[Value]) -> Value {
    let state = editor.state();
    let mut decorations: Vec<_> = editor.decorations().members.iter().flat_map(|set| set.find(None, None))
        .map(|span| (span.spec()["id"].as_u64().unwrap(), span.from, span.to)).collect();
    decorations.sort_unstable();
    let mut state = json!({
        "doc": state.doc,
        "selection": state.selection,
        "storedMarks": state.stored_marks,
        "decorations": decorations,
        "clipboard": editor.host().clipboard.as_ref().map(|contents| &contents.plain_text),
    });
    if matches!(action, Action::Composition { .. } | Action::Touch { .. }) {
        state["transactions"] = json!(transactions);
    }
    state
}
