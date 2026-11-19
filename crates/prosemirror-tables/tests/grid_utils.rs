//! Ports the four grid utility test files from prosemirror-tables 1.8.5.

use prosemirror_model::{Fragment, Node, Schema};
use prosemirror_schema_basic::{marks, nodes};
use prosemirror_tables::{table_nodes, TableNodesOptions};
use prosemirror_tables::utils::{
    convert_array_of_rows_to_table_node, convert_table_node_to_array_of_rows,
    move_row_in_array_of_rows, transpose,
};
use serde_json::json;

fn table_schema() -> Schema {
    let mut types = nodes();
    types.extend(table_nodes(TableNodesOptions::default()));
    Schema::new(types, marks())
}

type Cell<'a> = (usize, usize, &'a str);

fn cell(schema: &Schema, kind: &str, (colspan, rowspan, text): Cell<'_>) -> Node {
    let paragraph = schema.node(
        "paragraph", None,
        (!text.is_empty()).then(|| Fragment::from_nodes(vec![schema.text(text, None)])), None,
    ).unwrap();
    schema.node(
        kind, Some(json!({"colspan": colspan, "rowspan": rowspan})),
        Some(Fragment::from_nodes(vec![paragraph])), None,
    ).unwrap()
}

fn table(schema: &Schema, rows: &[&[Cell<'_>]]) -> Node {
    let rows = rows.iter().map(|row| {
        let cells = row.iter().map(|entry| cell(schema, "table_cell", *entry)).collect();
        schema.node("table_row", None, Some(Fragment::from_nodes(cells)), None).unwrap()
    }).collect();
    schema.node("table", None, Some(Fragment::from_nodes(rows)), None).unwrap()
}

fn text(value: &str) -> Cell<'_> {
    (1, 1, value)
}

fn simple() -> [[Cell<'static>; 2]; 2] {
    [[text("A1"), text("B1")], [text("A2"), text("B2")]]
}

fn merged() -> [[Cell<'static>; 3]; 3] {
    [
        [text("A1"), text("B1"), (2, 1, "C1")],
        [text("A2"), (2, 1, "B2"), (1, 2, "D1")],
        [text("A3"), text("B3"), text("C3")],
    ]
}

fn table_from_to<const N: usize, const M: usize>(original: [[Cell<'_>; M]; N], expected: [[Cell<'_>; M]; N], edits: &[(usize, usize, Cell<'_>)]) {
    let original = original.iter().map(|row| row.as_slice()).collect::<Vec<_>>();
    let expected = expected.iter().map(|row| row.as_slice()).collect::<Vec<_>>();
    assert_table(&original, &expected, edits);
}

fn assert_table(original: &[&[Cell<'_>]], expected: &[&[Cell<'_>]], edits: &[(usize, usize, Cell<'_>)]) {
    let schema = table_schema();
    let original = table(&schema, original);
    let mut rows = convert_table_node_to_array_of_rows(&original).unwrap();
    for &(row, col, value) in edits {
        rows[row][col] = Some(cell(&schema, "table_cell", value));
    }
    let converted = convert_array_of_rows_to_table_node(&original, &rows).unwrap();
    assert!(!std::ptr::eq(original.child(0).content(), converted.child(0).content()));
    assert_eq!(converted, table(&schema, expected));
}

#[test]
fn grid_round_trips_a_table() {
    table_from_to(simple(), simple(), &[]);
}

#[test]
fn grid_replaces_modified_cell_content() {
    let mut expected = simple();
    expected[0][1] = text("Modified");
    table_from_to(simple(), expected, &[(0, 1, text("Modified"))]);
}

#[test]
fn grid_accepts_empty_cells() {
    let mut expected = simple();
    expected[1][0] = text("");
    table_from_to(simple(), expected, &[(1, 0, text(""))]);
}

#[test]
fn grid_replaces_multiple_cells() {
    let start = [
        [text("A1"), text("B1"), text("C1")],
        [text("A2"), text("B2"), text("C2")],
        [text("A3"), text("B3"), text("C3")],
    ];
    let mut expected = start;
    let edits = [(0, 0, text("New A1")), (1, 1, text("New B2")), (2, 2, text("New C3"))];
    for (row, col, value) in edits {
        expected[row][col] = value;
    }
    table_from_to(start, expected, &edits);
}

#[test]
fn grid_round_trips_merged_cells() {
    table_from_to(merged(), merged(), &[]);
}

#[test]
fn grid_edits_cells_in_merged_table() {
    let mut expected = merged();
    expected[0][2] = (2, 1, "Modified C1");
    table_from_to(merged(), expected, &[(0, 2, (2, 1, "Modified C1"))]);
}

#[test]
fn grid_reconstructs_a_single_row() {
    table_from_to(
        [[text("Single"), text("Row"), text("Table")]],
        [[text("Single"), text("Modified"), text("Table")]],
        &[(0, 1, text("Modified"))],
    );
}

#[test]
fn grid_reconstructs_a_single_column() {
    table_from_to(
        [[text("A1")], [text("A2")], [text("A3")]],
        [[text("A1")], [text("Modified A2")], [text("A3")]],
        &[(1, 0, text("Modified A2"))],
    );
}

#[test]
fn grid_preserves_attributes_when_editing_content() {
    assert_table(
        &[&[text("A1"), (2, 1, "B1")],
          &[(1, 2, "A2"), text("B2"), text("C2")],
          &[text("B3"), text("C3")]],
        &[&[text("A1"), (2, 1, "Modified B1")],
          &[(1, 2, "Modified A2"), text("B2"), text("C2")],
          &[text("B3"), text("C3")]],
        &[(0, 1, (2, 1, "Modified B1")), (1, 0, (1, 2, "Modified A2"))],
    );
}

fn assert_rows(original: &[&[Cell<'_>]], expected: &[&[Option<&str>]]) {
    let schema = table_schema();
    let rows = convert_table_node_to_array_of_rows(&table(&schema, original)).unwrap();
    let actual = rows.iter().map(|row| {
        row.iter().map(|node| node.as_ref().map(Node::text_content)).collect::<Vec<_>>()
    }).collect::<Vec<_>>();
    let expected = expected.iter().map(|row| {
        row.iter().map(|cell| cell.map(str::to_owned)).collect::<Vec<_>>()
    }).collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn table_converts_a_simple_grid() {
    assert_rows(&[&simple()[0], &simple()[1]], &[&[Some("A1"), Some("B1")], &[Some("A2"), Some("B2")]]);
}

#[test]
fn table_converts_empty_cells() {
    assert_rows(
        &[&[text("A1"), text("")], &[text(""), text("B2")]],
        &[&[Some("A1"), Some("")], &[Some(""), Some("B2")]],
    );
}

#[test]
fn table_converts_rows_of_equal_length() {
    assert_rows(
        &[&[text("A1"), text("B1"), text("C1")], &[text("A2"), text("B2"), text("C2")]],
        &[&[Some("A1"), Some("B1"), Some("C1")], &[Some("A2"), Some("B2"), Some("C2")]],
    );
}

#[test]
fn table_converts_a_single_row() {
    assert_rows(&[&[text("Single"), text("Row")]], &[&[Some("Single"), Some("Row")]]);
}

#[test]
fn table_converts_a_single_column() {
    assert_rows(&[&[text("A1")], &[text("A2")], &[text("A3")]], &[&[Some("A1")], &[Some("A2")], &[Some("A3")]]);
}

#[test]
fn table_converts_spanned_cells_to_none_after_the_anchor() {
    let merged = merged();
    assert_rows(
        &[&merged[0], &merged[1], &merged[2]],
        &[&[Some("A1"), Some("B1"), Some("C1"), None],
          &[Some("A2"), Some("B2"), None, Some("D1")],
          &[Some("A3"), Some("B3"), Some("C3"), None]],
    );
}

type WordGrid = Vec<Vec<&'static str>>;

fn transpose_input() -> (WordGrid, WordGrid) {
    let source = vec![vec!["a1", "a2", "a3"], vec!["b1", "b2", "b3"],
                      vec!["c1", "c2", "c3"], vec!["d1", "d2", "d3"]];
    let target = vec![vec!["a1", "b1", "c1", "d1"], vec!["a2", "b2", "c2", "d2"],
                      vec!["a3", "b3", "c3", "d3"]];
    (source, target)
}

#[test]
fn transposes_columns_to_rows() {
    let (source, target) = transpose_input();
    assert_eq!(transpose(&source), target);
}

#[test]
fn transposition_is_its_own_inverse() {
    let (source, target) = transpose_input();
    assert_eq!(transpose(&target), source);
}

fn row_move(length: usize, origin: &[usize], target: &[usize], direction: i8, expected: &[usize]) {
    let mut source = (0..length).collect();
    assert_eq!(move_row_in_array_of_rows(&mut source, origin, target, direction).as_slice(), expected);
}

macro_rules! move_cases {
    ($($name:ident: $length:literal, $origin:expr, $target:expr, $dir:expr, $expected:expr;)*) => {$(
        #[test]
        fn $name() {
            row_move($length, &$origin, &$target, $dir, &$expected);
        }
    )*};
}

move_cases! {
    moves_one_row_down: 5, [1], [3], 0, [0, 2, 3, 1, 4];
    moves_one_row_up: 5, [3], [1], 0, [0, 3, 1, 2, 4];
    moves_first_row_to_end: 4, [0], [3], 0, [1, 2, 3, 0];
    moves_last_row_to_start: 4, [3], [0], 0, [3, 0, 1, 2];
    moves_two_rows_down: 6, [1, 2], [4, 5], 0, [0, 3, 4, 5, 1, 2];
    moves_two_rows_up: 6, [4, 5], [1, 2], 0, [0, 4, 5, 1, 2, 3];
    moves_three_rows: 7, [1, 2, 3], [5, 6], 0, [0, 4, 5, 6, 1, 2, 3];
    forces_move_before_target: 6, [1], [4], -1, [0, 2, 3, 1, 4, 5];
    moves_in_natural_direction: 6, [1], [4], 0, [0, 2, 3, 4, 1, 5];
    forces_move_after_target: 5, [3], [1], 1, [0, 1, 3, 2, 4];
    leaves_a_single_row: 1, [0], [0], 0, [0];
    moves_a_row_in_two: 2, [0], [1], 0, [1, 0];
    preserves_same_position: 4, [2], [2], 0, [0, 1, 2, 3];
    swaps_adjacent_rows: 4, [1], [2], 0, [0, 2, 1, 3];
    moves_rows_in_a_large_array: 10, [2, 3, 4], [7, 8, 9], 0, [0, 1, 5, 6, 7, 8, 9, 2, 3, 4];
    moves_start_to_end: 5, [0, 1, 2], [4], 0, [3, 4, 0, 1, 2];
    moves_end_to_start: 5, [3, 4], [0, 1], 0, [3, 4, 0, 1, 2];
}

#[test]
fn moves_string_rows() {
    let mut rows = vec!["a", "b", "c", "d"];
    assert_eq!(move_row_in_array_of_rows(&mut rows, &[0], &[2], 0).as_slice(), &["b", "c", "a", "d"]);
}

#[test]
fn moves_heterogeneous_rows() {
    let mut rows = json!([1, "a", true, null, 4]).as_array().unwrap().clone();
    move_row_in_array_of_rows(&mut rows, &[1], &[3], 0);
    assert_eq!(json!(rows), json!([1, true, null, "a", 4]));
}

#[test]
fn moves_table_cell_rows() {
    let schema = table_schema();
    let mut rows = [("0", "A"), ("1", "B"), ("2", "C")].into_iter()
        .map(|(label, value)| [cell(&schema, "table_cell", text(label)), cell(&schema, "table_cell", text(value))])
        .collect::<Vec<_>>();
    move_row_in_array_of_rows(&mut rows, &[2], &[0], 0);
    let labels: Vec<_> = rows.iter().map(|row| row[0].text_content()).collect();
    assert_eq!(labels, ["2", "0", "1"]);
}

#[test]
fn preserves_destination_header_type_when_moving_a_body_cell() {
    let schema = table_schema();
    let header = schema.node(
        "table_row", None, Some(Fragment::from_nodes(vec![cell(&schema, "table_header", text("A"))])), None,
    ).unwrap();
    let body = schema.node(
        "table_row", None, Some(Fragment::from_nodes(vec![cell(&schema, "table_cell", text("B"))])), None,
    ).unwrap();
    let table = schema.node("table", None, Some(Fragment::from_nodes(vec![header, body])), None).unwrap();
    let mut rows = convert_table_node_to_array_of_rows(&table).unwrap();
    move_row_in_array_of_rows(&mut rows, &[1], &[0], 0);
    let rebuilt = convert_array_of_rows_to_table_node(&table, &rows).unwrap();
    assert_eq!(rebuilt.child(0).child(0).type_name(), "table_header");
    assert_eq!(rebuilt.child(0).child(0).text_content(), "B");
    assert_eq!(rebuilt.child(1).child(0).type_name(), "table_cell");
    assert_eq!(rebuilt.child(1).child(0).text_content(), "A");
}
