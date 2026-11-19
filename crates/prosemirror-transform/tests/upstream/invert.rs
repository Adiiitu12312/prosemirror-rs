//! Port of prosemirror-transform 1.12.7 test/test-invert.ts with reproducible seeds.

use prosemirror_model::{Fragment, Node, Schema, Slice};
use prosemirror_transform::{Step, StepResult, Transform};
use serde_json::{json, Value};

struct Random(u64);

impl Random {
    fn index(&mut self, upper: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % upper.max(1) as u64) as usize
    }

    fn text(&mut self, len: usize) -> String {
        (0..len).map(|_| char::from(b'a' + self.index(26) as u8)).collect()
    }
}

fn schema() -> Schema {
    super::corpus().schemas[0].build()
}

fn node(schema: &Schema, name: &str, attrs: Option<Value>, children: Vec<Node>) -> Node {
    schema.node(name, attrs, Some(Fragment::from_nodes(children)), None).unwrap()
}

fn random_doc(schema: &Schema, random: &mut Random) -> Node {
    let mut blocks = Vec::new();
    let mut len = 0;
    while len < 40 {
        let chars = (40usize - len).min(random.index(6) + 2);
        let text = schema.text(&random.text(chars), None);
        let kind = random.index(5);
        let p = |text| node(schema, "paragraph", None, vec![text]);
        let block = match kind {
            0 => node(schema, "heading", Some(json!({"level": 2})), vec![text]),
            1 => node(schema, "blockquote", None, vec![p(text)]),
            2 => {
                let li = |text| node(schema, "list_item", None, vec![p(text)]);
                let other = schema.text(&random.text(5), None);
                node(schema, "bullet_list", None, vec![li(text), li(other)])
            }
            _ => p(text),
        };
        len += block.node_size();
        blocks.push(block);
    }
    node(schema, "doc", None, blocks)
}

fn random_step(transform: &mut Transform, schema: &Schema, random: &mut Random) {
    let len = transform.doc.content().size();
    let operation = random.index(5);
    match operation {
        0 | 1 => {
            let mark = match random.index(3) {
                0 => schema.mark_type("strong").unwrap().create(None),
                1 => schema.mark_type("em").unwrap().create(None),
                _ => schema.mark_type("link").unwrap().create(Some(json!({"href": format!("/{}", random.index(5))}))),
            };
            let from = random.index(len);
            let to = (from + random.index(10)).saturating_sub(1).min(len);
            let from = from.saturating_sub(1);
            if operation == 1 {
                transform.add_mark(from, to, mark);
            } else {
                transform.remove_mark(from, to, mark);
            }
        }
        2 => {
            let pos = random.index(len.saturating_sub(2)) + 1;
            let chars = random.index(2) + 1;
            transform.insert(pos, Fragment::from_nodes(vec![schema.text(&random.text(chars), None)]));
        }
        3 if len > 2 => {
            let from = random.index(len - 3) + 1;
            transform.delete(from, (from + 1 + random.index(8)).min(len - 1));
        }
        _ => {
            let name = if random.index(2) == 1 { "paragraph" } else { "heading" };
            let block = node(schema, name, None, Vec::new());
            let pos = random.index(len.saturating_sub(2)) + 1;
            transform.replace(pos, pos, Slice::new(Fragment::from_nodes(vec![block.clone(), block]), 1, 1));
        }
    }
}

fn applied(step: &Step, doc: &Node) -> Node {
    match step.apply(doc) {
        StepResult::Ok(doc) => doc,
        StepResult::Failed(error) => panic!("inverse failed: {error}; step {step:?}"),
    }
}

#[test]
fn reverts_to_the_previous_document() {
    let schema = schema();
    let mut random = Random(0x43ac_0d12_8f25_793e);
    for example in 0..200 {
        let mut transform = Transform::new(random_doc(&schema, &mut random));
        for edit in 0..10 {
            let before = transform.doc.clone();
            let first = transform.steps.len();
            random_step(&mut transform, &schema, &mut random);
            let mut doc = transform.doc.clone();
            for (step, before_step) in transform.steps[first..].iter().zip(&transform.docs[first..]).rev() {
                doc = applied(&step.invert(before_step), &doc);
            }
            assert_eq!(doc, before, "example {example}, edit {edit}");
        }
    }
}

#[test]
fn reverts_accurately_when_mapped() {
    let schema = schema();
    let mut random = Random(0x694f_b024_0d97_27ac);
    for example in 0..1_000 {
        let before = random_doc(&schema, &mut random);
        let mut first = Transform::new(before.clone());
        random_step(&mut first, &schema, &mut random);
        let Some(step) = first.steps.first() else { continue };
        let mut other = Transform::new(before);
        random_step(&mut other, &schema, &mut random);
        let Some(mapped) = step.map(&other.mapping) else { continue };
        let StepResult::Ok(doc) = mapped.apply(&other.doc) else { continue };
        assert_eq!(applied(&mapped.invert(&other.doc), &doc), other.doc, "example {example}; {mapped:?}");
    }
}

#[test]
fn inverted_mark_sets_keep_exclusive_link_changes_serializable() {
    let schema = schema();
    let link = |href| schema.mark_type("link").unwrap().create(Some(json!({"href": href})));
    let before = node(&schema, "doc", None, vec![node(&schema, "paragraph", None, vec![
        schema.text("abc", Some(vec![link("/old")])),
    ])]);
    let mut transform = Transform::new(before.clone());
    transform.add_mark(1, 4, link("/new"));
    assert_eq!(transform.steps.len(), 1);
    let undo = transform.steps[0].invert(&before);
    assert_eq!(applied(&undo, &transform.doc), before);
    let undo_json = serde_json::to_value(&undo).unwrap();
    assert_eq!(undo_json["stepType"], "markSet");
    assert_eq!(applied(&Step::from_json(&schema, &undo_json).unwrap(), &transform.doc), before);

    let redo = undo.invert(&transform.doc);
    let redo_json = serde_json::to_value(&redo).unwrap();
    // The upstream representation nests another markSet here and its own JSON decoder rejects it.
    let replay = Step::from_json(&schema, &redo_json).unwrap();
    assert_eq!(applied(&replay, &before), transform.doc);
    assert!(replay.get_map().ranges.is_empty());
    assert!(replay.map(&prosemirror_transform::StepMap::single(1, 3, 0)).is_none());
    assert_eq!(transform.mapping.map(3, 1), 3);
}
