//! Port of prosemirror-changeset 2.4.0 test/test-changed-range.ts.

use prosemirror_changeset::{ChangeSet, ChangedRange};
use prosemirror_model::Fragment;
use prosemirror_transform::Transform;

use super::{single_paragraph, text};

fn changed(from: isize, to: isize) -> Option<ChangedRange> {
    Some(ChangedRange { from, to })
}

fn inserted(value: &str) -> Fragment {
    Fragment::from_nodes(vec![text(value)])
}

#[test]
fn returns_none_for_identical_sets() {
    let doc = single_paragraph("foo");
    let mut transform = Transform::new(doc.clone());
    transform.replace_with(2, 3, inserted("aaaa")).replace_with(1, 1, inserted("xx")).delete(5, 7);
    let maps = &transform.mapping.maps;
    let data = ["a"; 3];
    let set = ChangeSet::create(doc.clone()).add_steps_by_map(&transform.doc, maps, &data);
    let equivalent = ChangeSet::create(doc).add_steps_by_map(&transform.doc, maps, &data);
    assert_eq!(set.changed_range(&set, &[]), None);
    assert_eq!(set.changed_range(&equivalent, &[]), None);
}

#[test]
fn returns_changed_range_in_new_coordinates() {
    let doc = single_paragraph("abcd");
    let mut transform = Transform::new(doc.clone());
    transform.replace_with(2, 4, inserted("u"));
    let before = ChangeSet::create(doc);
    let after = before.add_steps_by_map(&transform.doc, &transform.mapping.maps, &["a"]);
    assert_eq!(before.changed_range(&after, &transform.mapping.maps), changed(2, 3));
}

#[test]
fn expands_to_cover_updated_spans() {
    let doc = single_paragraph("abcd");
    let mut transform = Transform::new(doc.clone());
    transform.replace_with(2, 2, inserted("c")).delete(3, 5);
    let maps = &transform.mapping.maps;
    let before = ChangeSet::create(doc);
    let first = before.add_steps_by_map(&transform.docs[1], &maps[..1], &["a"]);
    let after = before.add_steps_by_map(&transform.doc, maps, &["a", "a"]);
    assert_eq!(before.changed_range(&first, &maps[..1]), changed(2, 3));
    assert_eq!(first.changed_range(&after, &maps[1..]), changed(2, 3));
}

#[test]
fn detects_changes_in_deletion_metadata() {
    let doc = single_paragraph("abc");
    let mut transform = Transform::new(doc.clone());
    transform.delete(2, 3);
    let deletions = ChangeSet::create(doc).add_steps_by_map(&transform.doc, &transform.mapping.maps, &["a"]);
    assert_eq!(deletions.changed_range(&deletions.map(|_| "b"), &[]), changed(2, 2));
}
