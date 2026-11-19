use prosemirror_gapcursor::{find_gap_cursor_from, gap_cursor, valid_gap_cursor, Arrow, GapCursorView};
use prosemirror_keymap::{KeyEvent, Platform};
use prosemirror_model::{Fragment, Node};
use prosemirror_schema_basic::schema;
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_transform::StepMap;
use serde_json::json;

fn document(content: Vec<Node>) -> Node {
    schema().node("doc", None, Some(Fragment::from_nodes(content)), None).unwrap()
}

fn rule() -> Node {
    schema().node("horizontal_rule", None, None, None).unwrap()
}

fn paragraph(text: &str) -> Node {
    let content = (!text.is_empty()).then(|| Fragment::from_nodes(vec![schema().text(text, None)]));
    schema().node("paragraph", None, content, None).unwrap()
}

fn editor(doc: Node, selection: Selection) -> EditorState {
    EditorState::create(EditorStateConfig {
        schema: schema().clone(),
        doc: Some(doc),
        selection: Some(selection),
        stored_marks: None,
        plugins: Vec::new(),
    })
}

#[derive(Default)]
struct View {
    inside: Option<usize>,
    editable: bool,
    end: bool,
}

impl GapCursorView for View {
    fn editable(&self) -> bool {
        self.editable
    }

    fn end_of_textblock(&self, _: Arrow) -> bool {
        self.end
    }

    fn node_at_point(&self, _: f64, _: f64) -> Option<usize> {
        self.inside
    }
}

#[test]
fn gap_cursor_is_available_between_closed_blocks_and_at_closed_document_edges() {
    let doc = document(vec![rule(), rule()]);
    for pos in 0..=doc.content().size() {
        assert!(valid_gap_cursor(&doc.resolve(pos).unwrap()));
    }
    let doc = document(vec![paragraph("x"), rule()]);
    assert!(!valid_gap_cursor(&doc.resolve(0).unwrap()));
    assert!(!valid_gap_cursor(&doc.resolve(3).unwrap()));
    assert!(valid_gap_cursor(&doc.resolve(4).unwrap()));
}

#[test]
fn mapping_and_bookmarks_keep_valid_positions_and_fall_back_near_invalid_gaps() {
    let doc = document(vec![rule(), rule(), rule()]);
    let mapping = StepMap::single(0, 0, 1);
    let selection = Selection::gap_cursor(1);
    assert_eq!(selection.map(&doc, &mapping), Selection::gap_cursor(2));
    assert_eq!(selection.bookmark().map(&mapping).resolve(&doc), Selection::gap_cursor(2));
    assert!(selection.empty());
    assert!(!selection.visible());
    assert_eq!(selection.content(&doc).unwrap().size(), 0);
    assert_eq!(serde_json::to_value(&selection).unwrap(), json!({"type": "gapcursor", "pos": 1}));
    assert_eq!(Selection::from_json(&doc, &json!({"type": "gapcursor", "pos": 1})).unwrap(), selection);

    let doc = document(vec![paragraph("x")]);
    assert_eq!(selection.map(&doc, &StepMap::empty()), Selection::cursor(1));
    assert_eq!(selection.bookmark().resolve(&doc), Selection::cursor(1));
}

#[test]
fn selection_and_decoration_hooks_use_the_gap_cursor_widget() {
    let doc = document(vec![rule(), rule()]);
    let gap = doc.resolve(1).unwrap();
    let cursor = gap_cursor::<View>(Platform::Other, 42);
    assert_eq!(cursor.create_selection_between(&gap, &gap), Some(Selection::gap_cursor(1)));
    let different = doc.resolve(0).unwrap();
    assert_eq!(cursor.create_selection_between(&different, &gap), None);
    let state = editor(doc, Selection::gap_cursor(1));
    let decorations = cursor.decorations(&state).unwrap().find(None, None);
    assert_eq!(decorations.len(), 1);
    assert_eq!(decorations[0].from, 1);
    assert_eq!(decorations[0].spec()["key"], "gapcursor");
}

#[test]
fn arrow_and_click_dispatch_the_gap_selection_without_intercepting_a_selectable_node() {
    let state = editor(document(vec![rule(), rule()]), Selection::node(1));
    let cursor = gap_cursor(Platform::Other, 1);
    let mut selection = None;
    let view = View { editable: true, ..View::default() };
    assert!(cursor.handle_keydown(
        &state, &mut |tr| selection = Some(tr.selection()), &view, KeyEvent::new("ArrowLeft"),
    ));
    assert_eq!(selection, Some(Selection::gap_cursor(1)));
    let inside = View { inside: Some(1), ..view };
    assert!(!cursor.handle_click(&state, &mut |_| panic!("node click should be handled elsewhere"), &inside, 1, 0.0, 0.0));
    assert!(cursor.handle_click(&state, &mut |tr| selection = Some(tr.selection()), &view, 1, 0.0, 0.0));
    let gap = state.doc.resolve(1).unwrap();
    assert_eq!(find_gap_cursor_from(&gap, -1, false).unwrap().pos, 1);
    assert!(find_gap_cursor_from(&gap, -1, true).is_none());
}

#[test]
fn composition_creates_inline_context_without_consuming_the_input_event() {
    let state = editor(document(vec![rule(), rule()]), Selection::gap_cursor(1));
    let cursor = gap_cursor::<View>(Platform::Other, 1);
    let mut transaction = None;
    let consumed = cursor.before_input(&state, &mut |tr| transaction = Some(tr), "insertCompositionText");
    assert!(!consumed);
    let tr = transaction.expect("the IME can now use a text cursor");
    assert_eq!(*tr.doc(), document(vec![rule(), paragraph(""), rule()]));
    assert_eq!(tr.selection(), Selection::cursor(2));
}
