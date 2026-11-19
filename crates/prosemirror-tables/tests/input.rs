//! Table arrow navigation from prosemirror-tables@1.8.5/test/input.test.ts.

use prosemirror_model::Schema;
use prosemirror_schema_basic::{marks, nodes};
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_tables::input::{arrow, Direction};
use prosemirror_tables::{table_nodes, Axis, TableMap, TableNodesOptions};
use serde_json::json;

fn move_cursor(
    axis: Axis, direction: Direction, from: (usize, usize, usize), to: (usize, usize, usize),
) {
    let mut kinds = nodes();
    kinds.extend(table_nodes(TableNodesOptions::default()));
    let schema = Schema::new(kinds, marks());
    let cell = json!({"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"x"}]}]});
    let row = json!({"type":"table_row","content":vec![cell;3]});
    let doc = schema.node_from_json(&json!({"type":"table","content":vec![row;3]})).unwrap();
    let map = TableMap::get(&doc).unwrap();
    let pos = |(row, col, offset): (usize,usize,usize)| map.map[row * map.width + col] + 2 + offset;
    let state = EditorState::create(EditorStateConfig {
        schema, doc: Some(doc.clone()), selection: Some(Selection::cursor(pos(from))),
        plugins: Vec::new(), stored_marks: None,
    });
    let mut next = None;
    assert!(arrow(&state, Some(&mut |tr| next = Some(state.apply(&tr))), axis, direction, |_, _| true));
    let next = next.unwrap();
    assert_eq!(next.doc, doc);
    assert_eq!(next.selection, Selection::cursor(pos(to)));
}

#[test]
fn can_move_cursor_to_the_right_cell() {
    move_cursor(Axis::Horizontal, Direction::Forward, (1, 1, 1), (1, 2, 0));
}

#[test]
fn can_move_cursor_to_the_left_cell() {
    move_cursor(Axis::Horizontal, Direction::Backward, (1, 2, 0), (1, 1, 1));
}

#[test]
fn can_move_cursor_to_the_bottom_cell() {
    move_cursor(Axis::Vertical, Direction::Forward, (1, 1, 0), (2, 1, 0));
}

#[test]
fn can_move_cursor_to_the_top_cell() {
    move_cursor(Axis::Vertical, Direction::Backward, (2, 1, 0), (1, 1, 0));
}
