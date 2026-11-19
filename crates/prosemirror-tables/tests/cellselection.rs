// Ported from prosemirror-tables 1.8.5, test/cellselection.test.ts and its fixtures.
use std::sync::OnceLock;

use prosemirror_model::{AttributeSpec, Schema, Slice};
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_tables::commands::TableEdit;
use prosemirror_tables::{
    cell_around, draw_cell_selection, table_editing, table_nodes, CellSelection,
    TableEditingOptions, TableNodesOptions,
};
use prosemirror_test_builder::{builders, ChildSpec, TaggedDoc};
use prosemirror_view::DecorationType;
use serde_json::{json, Value};

fn schema() -> &'static Schema {
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        let mut nodes = prosemirror_schema_basic::nodes();
        nodes.extend(table_nodes(TableNodesOptions {
            cell_attributes: [("test".into(), AttributeSpec::with_default(json!("default")))].into(),
            ..Default::default()
        }));
        Schema::new(nodes, prosemirror_schema_basic::marks())
    })
}

macro_rules! node {
    ($kind:expr; $($child:expr),* $(,)?) => {
        builders(schema(), Default::default())
            .node($kind, None, vec![$(ChildSpec::from($child)),*]).unwrap()
    };
}

macro_rules! table {
    ($($row:expr),* $(,)?) => { node!("table"; $($row),*) };
}

macro_rules! row {
    ($($cell:expr),* $(,)?) => { node!("table_row"; $($cell),*) };
}

fn td(attrs: Option<Value>, text: &str) -> TaggedDoc {
    builders(schema(), Default::default())
        .node("table_cell", attrs, vec![node!("paragraph"; text).into()]).unwrap()
}

fn c(colspan: usize, rowspan: usize) -> TaggedDoc {
    td(Some(json!({"colspan": colspan, "rowspan": rowspan})), "x")
}

fn c11() -> TaggedDoc {
    c(1, 1)
}

fn empty() -> TaggedDoc {
    td(None, "")
}

fn anchor() -> TaggedDoc {
    td(None, "x<anchor>")
}

fn head() -> TaggedDoc {
    td(None, "x<head>")
}

fn selection_for(doc: &TaggedDoc) -> CellSelection {
    let anchor = cell_around(&doc.resolve(doc.tag("anchor")).unwrap()).unwrap().pos;
    let head = doc.get_tag("head").map_or(anchor, |head| {
        cell_around(&doc.resolve(head).unwrap()).unwrap().pos
    });
    CellSelection::create(doc, anchor, head).unwrap()
}

fn state(doc: &TaggedDoc, selection: Selection, editing: Option<TableEditingOptions>) -> EditorState {
    EditorState::create(EditorStateConfig {
        schema: schema().clone(),
        doc: Some(doc.node.clone()),
        selection: Some(selection),
        stored_marks: None,
        plugins: editing.into_iter().map(table_editing).collect(),
    })
}

fn empty_grid() -> TaggedDoc {
    node!("doc"; table!(
        row!(empty(), empty(), empty()),
        row!(empty(), empty(), empty()),
        row!(empty(), empty(), empty())
    ))
}

fn grid() -> TaggedDoc {
    node!("doc"; table!(row!(c11(), c11(), c11()), row!(c11(), c11(), c11()), row!(c11(), c11(), c11())))
}

fn run(anchor: usize, head: usize, command: TableEdit) -> CellSelection {
    let doc = empty_grid();
    let state = state(&doc, Selection::cell(&doc, anchor, head).unwrap(), None);
    let mut result = None;
    assert!(command.run(&state, Some(&mut |tr| result = Some(state.apply(&tr)))));
    let Selection::Cell(cell) = result.unwrap().selection else { panic!("expected cell selection") };
    cell
}

fn check_content(doc: TaggedDoc, expected: TaggedDoc) {
    let expected = Slice::new(expected.content().clone(), 1, 1);
    assert_eq!(selection_for(&doc).content(&doc).unwrap(), expected);
}

#[test]
fn puts_anchor_and_head_around_head_cell_content() {
    let doc = empty_grid();
    for (anchor, head, from, to) in [(2, 24, 25, 27), (24, 2, 3, 5), (10, 30, 31, 33), (30, 10, 11, 13)] {
        let selection = Selection::cell(&doc, anchor, head).unwrap();
        assert_eq!(selection.anchor(), from);
        assert_eq!(selection.head(&doc), to);
    }
}

#[test]
fn extends_cell_selection_when_adding_row() {
    let selection = run(34, 6, TableEdit::AddRowBefore);
    assert_eq!((selection.anchor, selection.head), (48, 6));
    let selection = run(6, 30, TableEdit::AddRowAfter);
    assert_eq!((selection.anchor, selection.head), (6, 44));
}

#[test]
fn extends_cell_selection_when_adding_column() {
    let selection = run(16, 24, TableEdit::AddColumnAfter);
    assert_eq!((selection.anchor, selection.head), (20, 32));
    let selection = run(24, 30, TableEdit::AddColumnBefore);
    assert_eq!((selection.anchor, selection.head), (32, 38));
}

#[test]
fn content_contains_only_selected_cells() {
    check_content(
        table!(row!(c11(), anchor(), empty()), row!(c11(), empty(), head()), row!(c11(), c11(), c11())),
        table!("<a>", row!(c11(), empty()), row!(empty(), c11())),
    );
}

#[test]
fn content_understands_spanning_cells() {
    check_content(
        table!(row!(anchor(), c(2, 2), c11(), c11()), row!(c11(), head(), c11(), c11())),
        table!(row!(c11(), c(2, 2), c11()), row!(c11(), c11())),
    );
}

#[test]
fn content_cuts_off_cells_sticking_out_horizontally() {
    check_content(
        table!(row!(c11(), anchor(), c(2, 1)), row!(c(4, 1)), row!(c(2, 1), head(), c11())),
        table!(row!(c11(), c11()), row!(td(Some(json!({"colspan": 2})), "")), row!(empty(), c11())),
    );
}

#[test]
fn content_cuts_off_cells_sticking_out_vertically() {
    check_content(
        table!(row!(c11(), c(1, 4), c(1, 2)), row!(anchor()), row!(c(1, 2), head()), row!(c11())),
        table!(row!(c11(), td(Some(json!({"rowspan": 2})), ""), empty()), row!(c11(), c11())),
    );
}

#[test]
fn content_preserves_column_widths() {
    check_content(
        table!(
            row!(c11(), anchor(), c11()),
            row!(td(Some(json!({"colspan": 3, "colwidth": [100, 200, 300]})), "x")),
            row!(c11(), head(), c11()),
        ),
        table!(
            row!(c11()),
            row!(td(Some(json!({"colwidth": [200]})), "")),
            row!(c11()),
        ),
    );
}

#[test]
fn normalizes_table_node_to_all_cells() {
    let doc = grid();
    let state = state(&doc, Selection::node(0), Some(TableEditingOptions::default()));
    assert_eq!(state.apply(&state.tr()).selection, Selection::cell(&doc, 2, 46).unwrap());
}

#[test]
fn retains_table_node_selection_when_allowed() {
    let doc = grid();
    let state = state(
        &doc,
        Selection::node(0),
        Some(TableEditingOptions { allow_table_node_selection: true }),
    );
    assert_eq!(state.apply(&state.tr()).selection, Selection::node(0));
}

#[test]
fn normalizes_row_node_to_cell_selection() {
    let doc = grid();
    let state = state(&doc, Selection::node(1), Some(TableEditingOptions::default()));
    assert_eq!(state.apply(&state.tr()).selection, Selection::cell(&doc, 2, 12).unwrap());
}

#[test]
fn normalizes_cell_node_to_cell_selection() {
    let doc = grid();
    let state = state(&doc, Selection::node(2), Some(TableEditingOptions::default()));
    assert_eq!(state.apply(&state.tr()).selection, Selection::cell(&doc, 2, 2).unwrap());
}

#[test]
fn preserves_cell_json_bookmarks_and_draws_cell_decorations() {
    let doc = grid();
    let selection = Selection::cell(&doc, 2, 24).unwrap();
    assert_eq!(serde_json::to_value(&selection).unwrap(), json!({"type": "cell", "anchor": 2, "head": 24}));
    let state = state(&doc, selection.clone(), None);
    let restored = EditorState::from_json(schema(), &state.to_json()).unwrap();
    assert_eq!(restored.selection, selection);
    assert_eq!(selection.bookmark().resolve(&doc), selection);
    let decorations = draw_cell_selection(&state).unwrap().find(None, None);
    assert_eq!(decorations.len(), 4);
    for decoration in decorations {
        let DecorationType::Node { attrs } = decoration.kind else { panic!("expected cell decoration") };
        assert_eq!(attrs["class"], "selectedCell");
    }
}

#[test]
fn deletes_each_selected_cell_content_through_transaction() {
    let doc = node!("doc"; table!(row!(c11(), c11()), row!(c11(), c11())));
    let expected = node!("doc"; table!(row!(c11(), empty()), row!(c11(), empty())));
    let state = state(&doc, Selection::cell(&doc, 7, 19).unwrap(), None);
    let mut tr = state.tr();
    tr.delete_selection();
    assert_eq!(state.apply(&tr).doc, expected.node);
}
