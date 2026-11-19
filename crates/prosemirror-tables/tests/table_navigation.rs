//! Native command regressions for table navigation, clearing, removal, and movement.

use prosemirror_model::{Node, Schema};
use prosemirror_schema_basic::{marks, nodes};
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_tables::cell_commands::{
    delete_cell_selection, delete_table, go_to_next_cell, move_table_column, move_table_row,
    MoveTableOptions,
};
use prosemirror_tables::{table_nodes, TableMap, TableNodesOptions};
use serde_json::{json, Value};

fn cell(text: &str, rowspan: usize) -> Value {
    let paragraph = if text.is_empty() {
        json!({"type": "paragraph"})
    } else {
        json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})
    };
    json!({"type": "table_cell", "attrs": {"rowspan": rowspan}, "content": [paragraph]})
}

fn document(rows: &[&[Value]]) -> Node {
    let mut kinds = nodes();
    kinds.extend(table_nodes(TableNodesOptions::default()));
    let schema = Schema::new(kinds, marks());
    let rows = rows.iter().map(|row| json!({"type": "table_row", "content": row})).collect::<Vec<_>>();
    schema.node_from_json(&json!({
        "type": "doc",
        "content": [
            {"type": "table", "content": rows},
            {"type": "paragraph", "content": [{"type": "text", "text": "after"}]},
        ],
    })).unwrap()
}

fn state(doc: Node, selection: Selection) -> EditorState {
    EditorState::create(EditorStateConfig {
        schema: doc.schema().unwrap().clone(),
        doc: Some(doc),
        selection: Some(selection),
        plugins: Vec::new(),
        stored_marks: None,
    })
}

fn offset(doc: &Node, row: usize, col: usize) -> usize {
    let map = TableMap::get(doc.child(0)).unwrap();
    1 + map.map[row * map.width + col]
}

#[test]
fn next_and_previous_skip_rows_without_distinct_cells() {
    let doc = document(&[&[cell("first", 2)], &[], &[cell("end", 1)]]);
    let first = offset(&doc, 0, 0);
    let end = offset(&doc, 2, 0);
    let mut state = state(doc, Selection::cursor(first + 2));
    let mut tr = None;
    assert!(!go_to_next_cell(-1)(&state, None));
    assert!(go_to_next_cell(1)(&state, Some(&mut |next| tr = Some(next))));
    let tr = tr.unwrap();
    assert!(tr.scrolled_into_view());
    assert_eq!(tr.selection(), Selection::text(end + 2, end + 5));
    state = state.apply(&tr);
    assert!(!go_to_next_cell(1)(&state, None));
    let mut previous = None;
    assert!(go_to_next_cell(-1)(&state, Some(&mut |next| previous = Some(next))));
    assert_eq!(previous.unwrap().selection(), Selection::text(first + 2, first + 7));
}

#[test]
fn removes_the_table_containing_the_anchor() {
    let doc = document(&[&[cell("first", 1)], &[cell("second", 1)]]);
    let start = offset(&doc, 0, 0);
    let state = state(doc, Selection::cursor(start + 2));
    let mut tr = None;
    assert!(delete_table(&state, Some(&mut |next| tr = Some(next))));
    let tr = tr.unwrap();
    assert!(tr.scrolled_into_view());
    assert_eq!(tr.doc().child_count(), 1);
    assert_eq!(tr.doc().child(0).text_content(), "after");
    let outside = state.apply(&tr);
    assert!(!delete_table(&outside, None));
}

#[test]
fn clears_only_selected_content_and_retains_cell_shape() {
    let doc = document(&[
        &[cell("first", 1), cell("second", 2), cell("unselected", 1)],
        &[cell("third", 1), cell("outside", 1)],
    ]);
    let first = offset(&doc, 0, 0);
    let second = offset(&doc, 0, 1);
    let selection = Selection::cell(&doc, first, second).unwrap();
    let state = state(doc, selection);
    let mut tr = None;
    assert!(delete_cell_selection(&state, Some(&mut |next| tr = Some(next))));
    let cleared = state.apply(&tr.unwrap());
    assert_eq!(cleared.doc, document(&[
        &[cell("", 1), cell("", 2), cell("unselected", 1)],
        &[cell("", 1), cell("outside", 1)],
    ]));
    assert!(matches!(cleared.selection, Selection::Cell(_)));
    assert!(delete_cell_selection(&cleared, Some(&mut |_| panic!("unchanged cells should not dispatch"))));
}

#[test]
fn moves_a_row_and_selects_it_at_its_destination() {
    let doc = document(&[
        &[cell("A1", 1), cell("B1", 1)],
        &[cell("A2", 1), cell("B2", 1)],
        &[cell("A3", 1), cell("B3", 1)],
    ]);
    let cursor = offset(&doc, 0, 0) + 2;
    let state = state(doc, Selection::cursor(cursor));
    let mut tr = None;
    assert!(move_table_row(MoveTableOptions::new(0, 2))(&state, Some(&mut |next| tr = Some(next))));
    let state = state.apply(&tr.unwrap());
    assert_eq!(state.doc.child(0).child(0).text_content(), "A2B2");
    assert_eq!(state.doc.child(0).child(2).text_content(), "A1B1");
    assert!(matches!(&state.selection, Selection::Cell(cell) if cell.is_row_selection(&state.doc)));
}

#[test]
fn moves_a_column_without_forcing_a_selection() {
    let doc = document(&[
        &[cell("A1", 1), cell("B1", 1), cell("C1", 1)],
        &[cell("A2", 1), cell("B2", 1), cell("C2", 1)],
    ]);
    let cursor = offset(&doc, 0, 0) + 2;
    let state = state(doc, Selection::cursor(cursor));
    let options = MoveTableOptions { select: false, ..MoveTableOptions::new(2, 0) };
    let mut tr = None;
    assert!(move_table_column(options)(&state, Some(&mut |next| tr = Some(next))));
    let state = state.apply(&tr.unwrap());
    assert_eq!(state.doc.child(0).child(0).text_content(), "C1A1B1");
    assert_eq!(state.doc.child(0).child(1).text_content(), "C2A2B2");
    assert!(!matches!(state.selection, Selection::Cell(_)));
}

#[test]
fn moving_a_row_preserves_its_spanning_cells() {
    let doc = document(&[
        &[cell("A", 2), cell("B", 1)],
        &[cell("C", 1)],
        &[cell("D", 1), cell("E", 1)],
    ]);
    let cursor = offset(&doc, 0, 0) + 2;
    let state = state(doc, Selection::cursor(cursor));
    assert!(!move_table_row(MoveTableOptions::new(0, 1))(&state, None));
    let options = MoveTableOptions { select: false, ..MoveTableOptions::new(0, 2) };
    let mut tr = None;
    assert!(move_table_row(options)(&state, Some(&mut |next| tr = Some(next))));
    assert_eq!(
        state.apply(&tr.unwrap()).doc,
        document(&[&[cell("D", 1), cell("E", 1)], &[cell("A", 2), cell("B", 1)], &[cell("C", 1)]]),
    );
}
