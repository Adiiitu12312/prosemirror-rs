//! Port of prosemirror-changeset 2.4.0 test/test-merge.ts.

use prosemirror_changeset::Change;

use super::{positions, range};

fn check(left: &[[usize; 4]], right: &[[usize; 4]], expected: &[[usize; 4]]) {
    let make = |ranges: &[[usize; 4]]| {
        ranges.iter().enumerate().map(|(i, positions)| range(*positions, i as u8)).collect::<Vec<_>>()
    };
    let merged = Change::merge(&make(left), &make(right), &|a, _| Some(*a));
    assert_eq!(positions(&merged), expected);
}

macro_rules! merge_cases {
    ($($name:ident: $a:expr, $b:expr, $expected:expr;)*) => {$(
        #[test]
        fn $name() {
            check(&$a, &$b, &$expected);
        }
    )*};
}

merge_cases! {
    merges_insertions: [[1, 1, 1, 2]], [[1, 1, 1, 2]], [[1, 1, 1, 3]];
    merges_deletions: [[1, 2, 1, 1]], [[1, 2, 1, 1]], [[1, 3, 1, 1]];
    merges_insertion_before_deletion: [[2, 3, 2, 2]], [[1, 1, 1, 2]], [[1, 1, 1, 2], [2, 3, 3, 3]];
    merges_insertion_after_deletion: [[2, 3, 2, 2]], [[2, 2, 2, 3]], [[2, 3, 2, 3]];
    merges_deletion_before_insertion: [[2, 2, 2, 3]], [[1, 2, 1, 1]], [[1, 2, 1, 2]];
    merges_deletion_after_insertion: [[2, 2, 2, 3]], [[3, 4, 3, 3]], [[2, 3, 2, 3]];
    merges_deletion_of_insertion: [[2, 2, 2, 3]], [[2, 3, 2, 2]], [];
    merges_insertion_after_replacement: [[2, 3, 2, 3]], [[3, 3, 3, 4]], [[2, 3, 2, 4]];
    merges_insertion_before_replacement: [[2, 3, 2, 3]], [[2, 2, 2, 3]], [[2, 3, 2, 4]];
    merges_replacement_after_insertion: [[2, 2, 2, 3]], [[2, 3, 2, 3]], [[2, 2, 2, 3]];
}
