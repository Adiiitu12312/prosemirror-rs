use prosemirror_dropcursor::{DragOver, DropCursor, DropCursorGeometry, DropPosition};
use prosemirror_model::Node;
use prosemirror_schema_basic::schema;
use prosemirror_view::LayoutRect;
use serde_json::json;

struct Layout;

impl DropCursorGeometry for Layout {
    fn position_rect(&self, pos: usize) -> Option<LayoutRect> {
        Some(LayoutRect { x: pos as f32 * 10.0, y: 0.0, width: 0.0, height: 10.0 })
    }

    fn node_rect(&self, pos: usize) -> Option<LayoutRect> {
        Some(LayoutRect { x: 4.0, y: pos as f32 * 5.0, width: 40.0, height: 10.0 })
    }
}

fn doc() -> Node {
    schema().node_from_json(&json!({"type":"doc","content":[
        {"type":"paragraph","content":[{"type":"text","text":"a"}]},
        {"type":"paragraph","content":[{"type":"text","text":"b"}]}
    ]})).unwrap()
}

fn over(pos: usize, inside: Option<usize>, time_ms: u64) -> DragOver<'static> {
    DragOver { editable: true, target: Some(DropPosition { pos, inside }), dragging: None, time_ms }
}

#[test]
fn draws_an_inline_cursor_and_clears_it_after_drop() {
    let doc = doc();
    let mut cursor = DropCursor::default();
    cursor.drag_over(&doc, over(1, Some(0), 100), |_, _| false);
    let overlay = cursor.overlay(&doc, &Layout).unwrap();
    assert!(!overlay.block);
    assert_eq!(overlay.rect, LayoutRect { x: 9.5, y: 0.0, width: 1.0, height: 10.0 });
    cursor.drop(120);
    cursor.tick(139);
    assert_eq!(cursor.position(), Some(1));
    cursor.tick(140);
    assert_eq!(cursor.position(), None);
}

#[test]
fn centers_a_block_cursor_between_siblings() {
    let doc = doc();
    let mut cursor = DropCursor::default();
    cursor.drag_over(&doc, over(3, None, 0), |_, _| false);
    let overlay = cursor.overlay(&doc, &Layout).unwrap();
    assert!(overlay.block);
    assert_eq!(overlay.rect, LayoutRect { x: 4.0, y: 12.0, width: 40.0, height: 1.0 });
}

#[test]
fn respects_node_disabling_and_drag_leave() {
    let doc = doc();
    let mut cursor = DropCursor::default();
    cursor.drag_over(&doc, over(1, Some(0), 0), |node, _| node.type_name() == "paragraph");
    assert_eq!(cursor.position(), None);
    cursor.drag_over(&doc, over(1, Some(0), 0), |_, _| false);
    cursor.drag_leave(true);
    assert_eq!(cursor.position(), Some(1));
    cursor.drag_leave(false);
    assert_eq!(cursor.position(), None);
}

#[test]
fn recomputes_the_active_drop_target_after_an_edit() {
    let doc = doc();
    let mut cursor = DropCursor::default();
    let slice = doc.slice(0, 3, false).unwrap();
    cursor.drag_over(&doc, DragOver { dragging: Some(&slice), ..over(1, Some(0), 0) }, |_, _| false);
    assert_eq!(cursor.position(), Some(0));
    cursor.update_drag(&doc, Some(DropPosition { pos: 5, inside: Some(3) }), |_, _| false);
    assert_eq!(cursor.position(), Some(6));
    cursor.update_drag(&doc, None, |_, _| false);
    assert_eq!(cursor.position(), None);
}

#[test]
fn removing_the_plugin_clears_an_active_overlay() {
    let doc = doc();
    let mut cursor = DropCursor::default();
    cursor.drag_over(&doc, over(1, Some(0), 0), |_, _| false);
    cursor.destroy();
    assert!(cursor.overlay(&doc, &Layout).is_none());
    assert_eq!(cursor.position(), None);
}
