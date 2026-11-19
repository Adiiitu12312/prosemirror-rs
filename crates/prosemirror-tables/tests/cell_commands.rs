//! The upstream cell command groups in prosemirror-tables 1.8.5 test/commands.test.ts.

use std::sync::OnceLock;

use prosemirror_model::{AttributeSpec, Schema};
use prosemirror_schema_basic::{marks, nodes};
use prosemirror_state::{EditorState, EditorStateConfig, Selection, Transaction};
use prosemirror_tables::cell_commands::{
    merge_cells, set_cell_attr, split_cell, split_cell_with_type, toggle_header,
    toggle_header_column, toggle_header_row, ToggleHeaderOptions, ToggleHeaderType,
};
use prosemirror_tables::{table_nodes, TableNodesOptions};
use serde_json::{json, Value};

fn verify_case(index: usize) {
    static CASES: OnceLock<Value> = OnceLock::new();
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    let cases = CASES.get_or_init(|| serde_json::from_str(include_str!("cell_command_cases.json")).unwrap());
    let schema = SCHEMA.get_or_init(|| {
        let mut kinds = nodes();
        kinds.extend(table_nodes(TableNodesOptions {
            cell_attributes: [("test".into(), AttributeSpec::with_default(json!("default")))].into(),
            ..Default::default()
        }));
        Schema::new(kinds, marks())
    });
    let case = &cases[index];
    let doc = schema.node_from_json(&case["doc"]).unwrap();
    let selection = Selection::from_json(&doc, &case["selection"]).unwrap();
    let state = EditorState::create(EditorStateConfig {
        schema: schema.clone(),
        doc: Some(doc),
        selection: Some(selection),
        plugins: Vec::new(),
        stored_marks: None,
    });
    let mut transaction = None;
    let mut record = |tr| transaction = Some(tr);
    let dispatch: Option<&mut dyn FnMut(Transaction)> = Some(&mut record);
    let title = case["title"].as_str().unwrap();
    let group = case["group"].as_str().unwrap();
    let handled = match group {
        "mergeCells" => merge_cells(&state, dispatch),
        "splitCell" if title.contains("header and normal cell") => split_cell_with_type(|options| {
            options.schema.node_type(if options.row == 0 { "table_header" } else { "table_cell" })
        })(&state, dispatch),
        "splitCell" => split_cell(&state, dispatch),
        "setCellAttr" => {
            let value = if title.contains("already there") { "default" } else { "value" };
            set_cell_attr("test", json!(value))(&state, dispatch)
        }
        "toggleHeaderRow" => toggle_header_row(&state, dispatch),
        "toggleHeaderColumn" => toggle_header_column(&state, dispatch),
        "toggleHeader" => {
            let kind = if title.contains("header column with") || title.contains("header column into") {
                ToggleHeaderType::Column
            } else {
                ToggleHeaderType::Row
            };
            toggle_header(kind, ToggleHeaderOptions::default())(&state, dispatch)
        }
        name => panic!("unknown upstream cell command {name}"),
    };
    let label = format!("{group} / {title}");
    if case["expected"].is_null() {
        assert!(!handled, "{label}");
        assert!(transaction.is_none(), "{label}");
    } else {
        assert!(handled, "{label}");
        let state = state.apply(&transaction.expect("command dispatches"));
        let expected = schema.node_from_json(&case["expected"]).unwrap();
        assert_eq!(state.doc, expected, "{label}");
        assert_eq!(serde_json::to_value(&state.selection).unwrap(), case["finalSelection"], "{label}");
    }
}

include!("cell_command_cases.in");
