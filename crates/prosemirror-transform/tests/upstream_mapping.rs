//! Port of prosemirror-transform 1.12.7 test/test-mapping.ts.

use prosemirror_transform::{Mapping, StepMap};
use serde_json::{json, Value};

fn mapping(ranges: &[[usize; 3]], mirror: Option<(usize, usize)>) -> Mapping {
    let mut mapping = Mapping::from_maps(ranges.iter().map(|range| StepMap::new(range.to_vec())).collect());
    if let Some((left, right)) = mirror {
        mapping.set_mirror(left, right);
    }
    mapping
}

fn check_mapping(mapping: Mapping, cases: &[(usize, usize, i32, bool)]) {
    let inverted = mapping.invert();
    for &(from, to, assoc, lossy) in cases {
        assert_eq!(mapping.map(from, assoc), to);
        if !lossy {
            assert_eq!(inverted.map(to, assoc), from);
        }
    }
}

fn check_deletion(ranges: &[[usize; 3]], pos: usize, assoc: i32, expected: &str) {
    let result = mapping(ranges, None).map_result(pos, assoc);
    let flags = [
        (result.deleted(), 'd'),
        (result.deleted_before(), 'b'),
        (result.deleted_after(), 'a'),
        (result.deleted_across(), 'x'),
    ];
    let found: String = flags.into_iter().filter_map(|(set, flag)| set.then_some(flag)).collect();
    assert_eq!(found, expected);
}

#[test]
fn maps_a_single_insertion() {
    check_mapping(mapping(&[[2, 0, 4]], None), &[
        (0, 0, 1, false), (2, 6, 1, false), (2, 2, -1, false), (3, 7, 1, false),
    ]);
}

#[test]
fn maps_a_single_deletion() {
    check_mapping(mapping(&[[2, 4, 0]], None), &[
        (0, 0, 1, false), (2, 2, -1, false), (3, 2, 1, true),
        (6, 2, 1, false), (6, 2, -1, true), (7, 3, 1, false),
    ]);
}

#[test]
fn maps_a_single_replacement() {
    check_mapping(mapping(&[[2, 4, 4]], None), &[
        (0, 0, 1, false), (2, 2, 1, false), (4, 6, 1, true),
        (4, 2, -1, true), (6, 6, -1, false), (8, 8, 1, false),
    ]);
}

#[test]
fn maps_a_mirrored_deletion_and_insertion() {
    check_mapping(mapping(&[[2, 4, 0], [2, 0, 4]], Some((0, 1))), &[
        (0, 0, 1, false), (2, 2, 1, false), (4, 4, 1, false), (6, 6, 1, false), (7, 7, 1, false),
    ]);
}

#[test]
fn maps_a_mirrored_insertion_and_deletion() {
    check_mapping(mapping(&[[2, 0, 4], [2, 4, 0]], Some((0, 1))), &[
        (0, 0, 1, false), (2, 2, 1, false), (3, 3, 1, false),
    ]);
}

#[test]
fn maps_a_mirrored_deletion_and_insertion_with_an_intervening_insertion() {
    check_mapping(mapping(&[[2, 4, 0], [1, 0, 1], [3, 0, 4]], Some((0, 2))), &[
        (0, 0, 1, false), (1, 2, 1, false), (4, 5, 1, false), (6, 7, 1, false), (7, 8, 1, false),
    ]);
}

#[test]
fn detects_deletions_before_a_position() {
    check_deletion(&[[0, 2, 0]], 2, -1, "db");
    check_deletion(&[[0, 2, 0]], 2, 1, "b");
    check_deletion(&[[0, 2, 2]], 2, -1, "db");
    check_deletion(&[[0, 1, 0], [0, 1, 0]], 2, -1, "db");
    check_deletion(&[[0, 1, 0]], 2, -1, "");
}

#[test]
fn detects_deletions_after_a_position() {
    check_deletion(&[[2, 2, 0]], 2, -1, "a");
    check_deletion(&[[2, 2, 0]], 2, 1, "da");
    check_deletion(&[[2, 2, 2]], 2, 1, "da");
    check_deletion(&[[2, 1, 0], [2, 1, 0]], 2, 1, "da");
    check_deletion(&[[3, 2, 0]], 2, -1, "");
}

#[test]
fn detects_deletions_across_a_position() {
    check_deletion(&[[0, 4, 0]], 2, -1, "dbax");
    check_deletion(&[[0, 4, 0]], 2, 1, "dbax");
    check_deletion(&[[0, 4, 0]], 2, 1, "dbax");
    check_deletion(&[[0, 1, 0], [4, 1, 0], [0, 3, 0]], 2, 1, "dbax");
}

#[test]
fn detects_deletions_around_a_position() {
    check_deletion(&[[4, 1, 0], [0, 1, 0]], 2, -1, "");
    check_deletion(&[[2, 1, 0], [0, 2, 0]], 2, -1, "dba");
    check_deletion(&[[2, 1, 0], [0, 1, 0]], 2, -1, "a");
    check_deletion(&[[3, 1, 0], [0, 2, 0]], 2, -1, "db");
}

#[test]
fn does_not_assign_deleted_flags_for_insertions() {
    let map = mapping(&[[4, 0, 2]], None);
    assert_eq!(map.map_result(4, -1).del_info, 0);
    assert_eq!(map.map_result(4, 1).del_info, 0);
}

#[test]
fn can_slice_a_mapping() {
    let map = mapping(&[[0, 0, 1], [5, 2, 0], [1, 0, 4]], None);
    assert_eq!(map.slice(1, 2).map(2, 1), 2);
    assert_eq!(map.slice(0, 2).map(7, 1), 6);
    assert_eq!(map.slice(1, 3).map(10, 1), 12);
}

#[test]
fn can_append_a_sliced_mapping() {
    let mut map = mapping(&[[3, 1, 2]], None);
    map.append_mapping(&mapping(&[[0, 0, 1], [5, 2, 0], [1, 0, 4]], None).slice(1, 2));
    assert_eq!(map.maps.len(), 2);
    assert_eq!(map.map(7, 1), 6);
}

#[test]
fn preserves_mirror_information_when_appending_sliced_mappings() {
    let mut map = mapping(&[[3, 1, 2], [10, 1, 0]], None);
    let mut other = mapping(&[[0, 0, 1], [0, 1, 0], [3, 1, 0], [3, 0, 1]], Some((0, 1)));
    other.set_mirror(2, 3);
    map.append_mapping(&other.slice(1, 4));
    assert_eq!(map.get_mirror(2), None);
    assert_eq!(map.get_mirror(3), Some(4));
}

#[test]
fn can_invert_a_sliced_mapping() {
    let map = mapping(&[[0, 0, 1], [5, 2, 0], [1, 0, 4]], None);
    assert_eq!(map.slice(1, 2).invert().map(10, 1), 12);
    assert_eq!(map.slice(0, 2).invert().map(10, 1), 11);
    assert_eq!(map.slice(1, 3).invert().map(10, 1), 8);
}

#[test]
fn parity_exception_sliced_mapping_inverse_mirror() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../oracle/parity-exceptions.json"
    ))
    .unwrap();
    let case = &cases["sliced_mapping_inverse_mirror"];
    let source = mapping(&[[2, 4, 0], [2, 0, 4], [0, 0, 1], [1, 0, 1]], Some((0, 1)));
    let sliced = source.slice(0, 2);
    let inverted = sliced.invert();
    let forward = sliced.map(4, 1);
    let observed = json!({
        "mapCount": inverted.maps.len(),
        "mirror": [inverted.get_mirror(0), inverted.get_mirror(1)],
        "forward": forward,
        "roundTrip": inverted.map(forward, 1),
    });
    assert_eq!(observed, case["rust"]);
    assert_eq!(observed["forward"], case["upstream"]["forward"]);
    assert_ne!(observed["mirror"], case["upstream"]["mirror"]);
    assert_ne!(observed["roundTrip"], case["upstream"]["roundTrip"]);
    assert_eq!(inverted.map(forward, 1), 4);
    assert!(inverted
        .mirror
        .iter()
        .flatten()
        .all(|&index| index < inverted.maps.len()));
}

#[test]
fn appending_a_map_to_a_slice_does_not_mutate_the_source() {
    let original = mapping(&[[0, 0, 1], [3, 2, 0]], None);
    let mut slice = original.slice(1, 2);
    slice.append_map(StepMap::single(5, 0, 1), Some(1));
    assert_eq!(original.maps.len(), 2);
    assert_eq!(original.get_mirror(1), None);
    assert_eq!(slice.get_mirror(1), Some(2));
}

#[test]
fn empty_mapping_slice_preserves_positions_and_flags() {
    let mapping = mapping(&[[0, 0, 6], [4, 2, 0]], None);
    let empty = mapping.slice(0, 0);
    assert_eq!(empty.map(2, 1), 2);
    assert!(!empty.map_result(2, 1).deleted_after());
}

#[test]
fn inverted_map_reports_old_and_new_ranges() {
    let map = StepMap::new(vec![2, 3, 1, 8, 0, 2]).invert();
    let mut ranges = Vec::new();
    map.for_each(|old_from, old_to, new_from, new_to| ranges.push((old_from, old_to, new_from, new_to)));
    assert_eq!(ranges, [(2, 3, 2, 5), (6, 8, 8, 8)]);
    let recovery = map.map_result(7, 1).recover.expect("position inside a replaced range");
    assert!(map.touches(7, recovery));
    assert!(!map.touches(2, recovery));
}
