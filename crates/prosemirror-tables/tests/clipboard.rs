//! Port of prosemirror-tables@1.8.5/test/copypaste.test.ts.

use std::sync::OnceLock;

use prosemirror_model::{AttributeSpec, Schema};
use prosemirror_schema_basic::{marks, nodes};
use prosemirror_state::{EditorState, EditorStateConfig};
use prosemirror_tables::copypaste::{clip_cells, insert_cells, pasted_cells, Area};
use prosemirror_tables::{table_nodes, Rect, TableNodesOptions};
use serde_json::{json, Value};

fn area_json(area: Option<Area>) -> Value {
    area.map_or(Value::Null, |area| {
        let rows: Vec<_> = area.rows.iter().map(|row| {
            if row.child_count() == 0 { Value::Null } else { serde_json::to_value(row).unwrap() }
        }).collect();
        json!({"width":area.width,"height":area.height,"rows":rows})
    })
}

fn number(value: &Value) -> usize {
    usize::try_from(value.as_u64().unwrap()).unwrap()
}

fn verify_case(index: usize) {
    static CASES: OnceLock<Value> = OnceLock::new();
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    let cases = CASES.get_or_init(|| serde_json::from_str(include_str!("clipboard_cases.json")).unwrap());
    let schema = SCHEMA.get_or_init(|| {
        let mut kinds = nodes();
        kinds.extend(table_nodes(TableNodesOptions {
            cell_attributes: [("test".into(), AttributeSpec::with_default(json!("default")))].into(),
            ..Default::default()
        }));
        Schema::new(kinds, marks())
    });
    let case = &cases[index];
    let label = format!("{} / {}", case["group"], case["title"]);
    let slice = schema.slice_from_json(&case["slice"]).unwrap();
    let area = pasted_cells(&slice);
    match case["group"].as_str().unwrap() {
        "pastedCells" => assert_eq!(area_json(area), case["expected"], "{label}"),
        "clipCells" => {
            let result = clip_cells(area.unwrap(), number(&case["width"]), number(&case["height"]));
            assert_eq!(area_json(result), case["expected"], "{label}");
        }
        "insertCells" => {
            let doc = schema.node_from_json(&case["doc"]).unwrap();
            let state = EditorState::create(EditorStateConfig {
                schema: schema.clone(), doc: Some(doc), selection: None, plugins: Vec::new(), stored_marks: None,
            });
            let rect = &case["rect"];
            let rect = Rect {
                left: number(&rect["left"]), top: number(&rect["top"]),
                right: number(&rect["right"]), bottom: number(&rect["bottom"]),
            };
            let tr = insert_cells(&state, number(&case["start"]), rect, area.unwrap()).unwrap();
            assert_eq!(serde_json::to_value(tr.doc()).unwrap(), case["expected"], "{label}");
            assert_eq!(serde_json::to_value(tr.selection()).unwrap(), case["selection"], "{label}: selection");
        }
        group => panic!("unexpected group {group}"),
    }
}

include!("clipboard_cases.in");
