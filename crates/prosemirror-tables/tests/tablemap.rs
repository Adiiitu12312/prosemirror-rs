//! Port of prosemirror-tables@1.8.5/test/tablemap.test.ts.

use prosemirror_model::Schema;
use prosemirror_schema_basic::{marks, nodes};
use prosemirror_tables::{table_nodes, Axis, Rect, TableMap, TableNodesOptions};
use serde_json::json;

fn table(rows: &[&[(usize, usize)]]) -> TableMap {
    let mut nodes = nodes();
    nodes.extend(table_nodes(TableNodesOptions::default()));
    let schema = Schema::new(nodes, marks());
    let content: Vec<_> = rows.iter().map(|row| {
        let cells: Vec<_> = row.iter().map(|&(cols, rows)| json!({
            "type":"table_cell","attrs":{"colspan":cols,"rowspan":rows},"content":[
                {"type":"paragraph","content":[{"type":"text","text":"x"}]}
            ]
        })).collect();
        json!({"type":"table_row","content":cells})
    }).collect();
    let doc = schema.node_from_json(&json!({"type":"table","content":content})).unwrap();
    TableMap::get(&doc).unwrap()
}

#[test]
fn finds_the_right_shape_for_a_simple_table() {
    let row = &[(1, 1); 3][..];
    assert_eq!(table(&[row; 4]).map, [1, 6, 11, 18, 23, 28, 35, 40, 45, 52, 57, 62]);
}

#[test]
fn finds_the_right_shape_for_colspans() {
    let map = table(&[&[(1, 1), (2, 1)], &[(2, 1), (1, 1)], &[(1, 1); 3]]);
    assert_eq!(map.map, [1, 6, 6, 13, 13, 18, 25, 30, 35]);
}

#[test]
fn finds_the_right_shape_for_rowspans() {
    let map = table(&[&[(1, 2), (1, 1), (1, 2)], &[(1, 1)]]);
    assert_eq!(map.map, [1, 6, 11, 1, 18, 11]);
}

#[test]
fn finds_the_right_shape_for_deep_rowspans() {
    let map = table(&[&[(1, 4), (2, 1)], &[(1, 2), (1, 2)], &[]]);
    assert_eq!(map.map, [1, 6, 6, 1, 13, 18, 1, 13, 18]);
}

#[test]
fn finds_the_right_shape_for_larger_rectangles() {
    let map = table(&[&[(1, 1), (4, 4)], &[(1, 1)], &[(1, 1)], &[(1, 1)]]);
    assert_eq!(map.map, [1, 6, 6, 6, 6, 13, 6, 6, 6, 6, 20, 6, 6, 6, 6, 27, 6, 6, 6, 6]);
}

fn spanned_map() -> TableMap {
    table(&[&[(2, 3), (1, 1), (1, 2)], &[(1, 1)], &[(2, 1)]])
}

#[test]
fn can_accurately_find_cell_sizes() {
    let map = spanned_map();
    assert_eq!((map.width, map.height), (4, 3));
    for (pos, left, right, top, bottom) in [
        (1, 0, 2, 0, 3), (6, 2, 3, 0, 1), (11, 3, 4, 0, 2), (18, 2, 3, 1, 2), (25, 2, 4, 2, 3),
    ] {
        assert_eq!(map.find_cell(pos).unwrap(), Rect { left, right, top, bottom });
    }
}

#[test]
fn can_find_the_rectangle_between_two_cells() {
    let map = spanned_map();
    for (a, b, expected) in [
        (1, 6, &[1, 6, 18, 25][..]),
        (1, 25, &[1, 6, 11, 18, 25]),
        (1, 1, &[1]),
        (6, 25, &[6, 11, 18, 25]),
        (6, 11, &[6, 11, 18]),
        (11, 6, &[6, 11, 18]),
        (18, 25, &[18, 25]),
        (6, 18, &[6, 18]),
    ] {
        assert_eq!(map.cells_in_rect(map.rect_between(a, b).unwrap()), expected);
    }
}

#[test]
fn can_find_adjacent_cells() {
    use Axis::{Horizontal as H, Vertical as V};

    let map = spanned_map();
    for (pos, axis, direction, expected) in [
        (1, H, 1, Some(6)), (1, H, -1, None), (1, V, 1, None), (1, V, -1, None),
        (18, H, 1, Some(11)), (18, H, -1, Some(1)), (18, V, 1, Some(25)), (18, V, -1, Some(6)),
        (25, V, 1, None), (25, V, -1, Some(18)), (25, H, 1, None), (25, H, -1, Some(1)),
    ] {
        assert_eq!(map.next_cell(pos, axis, direction).unwrap(), expected);
    }
}
