//! Port of prosemirror-tables@1.8.5/test/fixtable.test.ts.

use prosemirror_model::{Node, Schema};
use prosemirror_schema_basic::{marks, nodes};
use prosemirror_state::{EditorState, EditorStateConfig};
use prosemirror_tables::{fix_tables, table_nodes, TableNodesOptions};
use serde_json::{json, Value};

fn cell(kind: &str, attrs: Value, empty: bool) -> Value {
    let paragraph = if empty {
        json!({"type":"paragraph"})
    } else {
        json!({"type":"paragraph","content":[{"type":"text","text":"x"}]})
    };
    json!({"type":kind,"attrs":attrs,"content":[paragraph]})
}

fn c(colspan: usize, rowspan: usize) -> Value {
    cell("table_cell", json!({"colspan":colspan,"rowspan":rowspan}), false)
}

fn x() -> Value {
    c(1, 1)
}

fn empty() -> Value {
    cell("table_cell", json!({}), true)
}

fn cw(width: u64) -> Value {
    cell("table_cell", json!({"colwidth":[width]}), false)
}

fn table(rows: Vec<Vec<Value>>) -> Value {
    let rows: Vec<_> = rows.into_iter().map(|row| json!({"type":"table_row","content":row})).collect();
    json!({"type":"table","content":rows})
}

fn schema() -> Schema {
    let mut kinds = nodes();
    kinds.extend(table_nodes(TableNodesOptions::default()));
    Schema::new(kinds, marks())
}

fn fix(schema: &Schema, tables: Vec<Value>) -> Option<Node> {
    let doc = schema.node_from_json(&json!({"type":"doc","content":tables})).unwrap();
    let state = EditorState::create(EditorStateConfig {
        schema: schema.clone(), doc: Some(doc), selection: None, plugins: Vec::new(), stored_marks: None,
    });
    fix_tables(&state, None).unwrap().map(|tr| tr.doc().clone())
}

fn check(source: Vec<Vec<Value>>, expected: Vec<Vec<Value>>) {
    let schema = schema();
    let doc = fix(&schema, vec![table(source)]).expect("needs repair");
    assert_eq!(doc.child(0), &schema.node_from_json(&table(expected)).unwrap());
}

#[test]
fn doesnt_touch_correct_tables() {
    assert!(fix(&schema(), vec![table(vec![vec![x(), x(), c(1, 2)], vec![x(), x()]])]).is_none());
}

#[test]
fn adds_trivially_missing_cells() {
    check(
        vec![vec![x(), x(), c(1, 2)], vec![x()]],
        vec![vec![x(), x(), c(1, 2)], vec![x(), empty()]],
    );
}

#[test]
fn can_add_to_multiple_rows() {
    check(
        vec![vec![x()], vec![x(), x()], vec![c(3, 1)]],
        vec![vec![x(), empty(), empty()], vec![empty(), x(), x()], vec![c(3, 1)]],
    );
}

#[test]
fn defaults_to_adding_at_the_start_of_the_first_row() {
    check(vec![vec![x()], vec![x(), x()]], vec![vec![empty(), x()], vec![x(), x()]]);
}

#[test]
fn defaults_to_adding_at_the_end_of_a_non_first_row() {
    check(vec![vec![x(), x()], vec![x()]], vec![vec![x(), x()], vec![x(), empty()]]);
}

#[test]
fn fixes_overlapping_cells() {
    check(
        vec![vec![x(), c(1, 2), x()], vec![c(2, 1)]],
        vec![vec![x(), c(1, 2), x()], vec![x(), empty(), empty()]],
    );
}

#[test]
fn fixes_a_rowspan_that_sticks_out_of_the_table() {
    check(vec![vec![x(), x()], vec![c(1, 2), x()]], vec![vec![x(), x()], vec![x(), x()]]);
}

#[test]
fn makes_sure_column_widths_are_coherent() {
    check(
        vec![vec![x(), x(), cw(200)], vec![cw(100), x(), x()]],
        vec![vec![cw(100), x(), cw(200)], vec![cw(100), x(), cw(200)]],
    );
}

#[test]
fn can_update_column_widths_on_colspan_cells() {
    let adjusted = cell("table_cell", json!({"colspan":3,"rowspan":2,"colwidth":[0,0,200]}), false);
    check(
        vec![vec![x(), x(), cw(200)], vec![c(3, 2)], vec![]],
        vec![vec![x(), x(), cw(200)], vec![adjusted], vec![]],
    );
}

#[test]
fn updates_the_odd_one_out_when_column_widths_disagree() {
    check(
        vec![
            vec![cw(100), cw(100), cw(100)],
            vec![cw(200), cw(200), cw(100)],
            vec![cw(100), cw(200), cw(200)],
        ],
        vec![vec![cw(100), cw(200), cw(100)]; 3],
    );
}

#[test]
fn respects_table_role_when_inserting_a_cell() {
    let header = || cell("table_header", json!({}), false);
    let blank = || cell("table_header", json!({}), true);
    check(
        vec![vec![header()], vec![x(), x()], vec![c(3, 1)]],
        vec![vec![header(), blank(), blank()], vec![empty(), x(), x()], vec![c(3, 1)]],
    );
}

#[test]
fn removes_a_zero_sized_table() {
    let schema = schema();
    let expected = table(vec![vec![x()]]);
    let repaired = fix(&schema, vec![table(vec![vec![]]), expected.clone()]).unwrap();
    assert_eq!(repaired.child_count(), 1);
    assert_eq!(repaired.child(0), &schema.node_from_json(&expected).unwrap());
}
