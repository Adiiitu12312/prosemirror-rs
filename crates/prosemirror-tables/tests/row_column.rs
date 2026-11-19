//! The upstream row and column groups in prosemirror-tables@1.8.5/test/commands.test.ts.

use std::sync::OnceLock;

use prosemirror_model::{AttributeSpec, Schema};
use prosemirror_schema_basic::{marks, nodes};
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_tables::commands::TableEdit;
use prosemirror_tables::{table_nodes, TableNodesOptions};
use serde_json::{json, Value};

fn verify_case(index: usize) {
    static CASES: OnceLock<Value> = OnceLock::new();
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    let cases = CASES.get_or_init(|| serde_json::from_str(include_str!("row_column_cases.json")).unwrap());
    let schema = SCHEMA.get_or_init(|| {
        let mut kinds = nodes();
        kinds.extend(table_nodes(TableNodesOptions {
            cell_attributes: [("test".into(), AttributeSpec::with_default(json!("default")))].into(),
            ..Default::default()
        }));
        Schema::new(kinds, marks())
    });
    let case = &cases[index];
    let source = schema.node_from_json(&case["doc"]).unwrap();
    let selection = &case["selection"];
    let cell_selection = selection["type"] == "cell";
    let edit = match case["group"].as_str().unwrap() {
        "addColumnBefore" => TableEdit::AddColumnBefore,
        "addColumnAfter" => TableEdit::AddColumnAfter,
        "deleteColumn" => TableEdit::DeleteColumns,
        "addRowBefore" => TableEdit::AddRowBefore,
        "addRowAfter" => TableEdit::AddRowAfter,
        "deleteRow" => TableEdit::DeleteRows,
        name => panic!("unknown upstream table edit {name}"),
    };
    let editor_selection = if cell_selection {
        let anchor = selection["anchor"].as_u64().unwrap() as usize;
        let head = selection["head"].as_u64().unwrap() as usize;
        Selection::cell(&source, anchor, head).unwrap()
    } else {
        serde_json::from_value(selection.clone()).unwrap()
    };
    let state = EditorState::create(EditorStateConfig {
        schema: schema.clone(),
        doc: Some(source),
        selection: Some(editor_selection),
        plugins: Vec::new(),
        stored_marks: None,
    });
    let mut transaction = None;
    let handled = edit.run(&state, Some(&mut |tr| transaction = Some(tr)));
    let label = format!("{} / {}", case["group"], case["title"]);
    if case["expected"].is_null() {
        assert!(!handled, "{label}");
    } else {
        assert!(handled, "{label}");
        let expected = schema.node_from_json(&case["expected"]).unwrap();
        assert_eq!(state.apply(&transaction.unwrap()).doc, expected, "{label}");
    }
}

include!("row_column_cases.in");
