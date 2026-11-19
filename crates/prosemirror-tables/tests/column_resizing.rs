use prosemirror_model::Schema;
use prosemirror_schema_basic::{marks, nodes};
use prosemirror_state::{EditorState, EditorStateConfig};
use prosemirror_tables::columnresizing::{column_resizing, handle_decorations, ColumnResizingOptions, Edge, ResizeState};
use prosemirror_tables::tableview::update_columns_on_resize;
use prosemirror_tables::{table_nodes, TableNodesOptions};
use serde_json::json;

fn editor() -> EditorState {
    let mut kinds = nodes();
    kinds.extend(table_nodes(TableNodesOptions::default()));
    let schema = Schema::new(kinds, marks());
    let cell = json!({"type":"table_cell","content":[{"type":"paragraph"}]});
    let row = json!({"type":"table_row","content":[cell.clone(),cell.clone(),cell]});
    let doc = schema.node_from_json(&json!({"type":"doc","content":[{"type":"table","content":[row.clone(),row]}]})).unwrap();
    EditorState::create(EditorStateConfig {
        schema, doc: Some(doc), selection: None, plugins: vec![column_resizing()], stored_marks: None,
    })
}

// prosemirror-tables@1.8.5/test/column-resizing.test.ts.
#[test]
fn returns_an_empty_decoration_set_if_cell_is_missing() {
    assert!(handle_decorations(&editor(), None).is_empty());
}

#[test]
fn updates_every_cell_in_the_dragged_column_and_resets_the_drag() {
    let options = ColumnResizingOptions::default();
    let state = editor();
    let state = state.apply(&options.hover(&state, Some((2, Edge::Right))).unwrap());
    assert_eq!(ResizeState::get(&state).unwrap().active_handle, Some(2));
    assert_eq!(handle_decorations(&state, Some(2)).find(None, None).len(), 2);
    let state = state.apply(&options.start_drag(&state, 20.0, 100.0).unwrap());
    let preview = options.preview(&state, 40.0).unwrap();
    assert_eq!(preview.columns, [Some(120.0), None, None]);
    assert_eq!(preview.min_width, Some(320.0));

    let state = state.apply(&options.commit_width(&state, 40.0).unwrap());
    let state = state.apply(&options.end_drag(&state));
    assert!(ResizeState::get(&state).unwrap().dragging.is_none());
    let table = state.doc.child(0);
    for row in &table.content().children {
        assert_eq!(row.child(0).attrs().unwrap()["colwidth"][0].as_f64(), Some(120.0));
    }
    assert_eq!(update_columns_on_resize(table, 100.0, None), preview);
}

#[test]
fn can_disable_the_last_column_handle() {
    let state = editor();
    let options = ColumnResizingOptions { last_column_resizable: false, ..Default::default() };
    assert!(options.hover(&state, Some((10, Edge::Right))).is_none());
}
