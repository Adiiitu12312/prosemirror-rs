use std::sync::{Arc, Once};

use prosemirror_model::{Fragment, ModelError, Node, ResolvedPos, Slice};
use prosemirror_state::{
    CustomSelection, CustomSelectionBookmark, EditorState, EditorStateConfig, Selection,
    SelectionBookmark, SelectionRange,
};
use prosemirror_test_builder::test_schema;
use prosemirror_transform::Mappable;
use serde_json::{json, Map, Value};

const ID: &str = "test-disjoint-ranges";
type Ranges = [(usize, usize); 2];

#[derive(Debug)]
struct DisjointSelection([SelectionRange; 2]);

impl DisjointSelection {
    fn new(doc: &Node, [(from1, to1), (from2, to2)]: Ranges) -> Result<Self, ModelError> {
        Ok(Self([
            SelectionRange { from: doc.resolve(from1)?, to: doc.resolve(to1)? },
            SelectionRange { from: doc.resolve(from2)?, to: doc.resolve(to2)? },
        ]))
    }

    fn positions(&self) -> Ranges {
        [(self.0[0].from.pos, self.0[0].to.pos), (self.0[1].from.pos, self.0[1].to.pos)]
    }
}

impl CustomSelection for DisjointSelection {
    fn anchor(&self) -> &ResolvedPos { &self.0[0].to }
    fn head(&self) -> &ResolvedPos { &self.0[0].from }
    fn visible(&self) -> bool { false }
    fn ranges(&self) -> Vec<SelectionRange> { self.0.to_vec() }

    fn map(&self, doc: &Node, mapping: &dyn Mappable) -> Selection {
        resolve(doc, map_positions(self.positions(), mapping))
    }

    fn bookmark(&self) -> SelectionBookmark {
        SelectionBookmark::custom(ID, DisjointBookmark(self.positions()))
    }

    fn json_fields(&self) -> Map<String, Value> {
        Map::from_iter([("ranges".into(), json!(self.positions()))])
    }
}

#[derive(Debug)]
struct DisjointBookmark(Ranges);

impl CustomSelectionBookmark for DisjointBookmark {
    fn map(&self, mapping: &dyn Mappable) -> SelectionBookmark {
        SelectionBookmark::custom(ID, Self(map_positions(self.0, mapping)))
    }

    fn resolve(&self, doc: &Node) -> Selection {
        resolve(doc, self.0)
    }

    fn json_fields(&self) -> Map<String, Value> {
        Map::from_iter([("ranges".into(), json!(self.0))])
    }
}

fn resolve(doc: &Node, ranges: Ranges) -> Selection {
    DisjointSelection::new(doc, ranges).and_then(|selection| Selection::custom(ID, selection))
        .unwrap_or_else(|_| Selection::at_start(doc))
}

fn map_positions(ranges: Ranges, mapping: &dyn Mappable) -> Ranges {
    ranges.map(|(from, to)| (mapping.map(from, 1), mapping.map(to, 1)))
}

fn parse_positions(json: &Value) -> Result<Ranges, ModelError> {
    serde_json::from_value(json["ranges"].clone()).map_err(|error| ModelError::Serialization(error.to_string()))
}

fn decode(doc: &Node, json: &Value) -> Result<Arc<dyn CustomSelection>, ModelError> {
    Ok(Arc::new(DisjointSelection::new(doc, parse_positions(json)?)?))
}

fn register() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        Selection::json_id(ID, decode).unwrap();
        SelectionBookmark::json_id(ID, |json| Ok(Arc::new(DisjointBookmark(parse_positions(json)?)))).unwrap();
    });
}

fn editor() -> EditorState {
    register();
    let schema = test_schema();
    let doc = schema.node_from_json(&json!({"type":"doc","content":[
        {"type":"paragraph","content":[{"type":"text","text":"abcdef"}]},
    ]})).unwrap();
    let selection = resolve(&doc, [(2, 3), (5, 6)]);
    EditorState::create(EditorStateConfig {
        schema: schema.clone(), doc: Some(doc), selection: Some(selection), stored_marks: None, plugins: vec![],
    })
}

#[test]
fn maps_custom_selection_through_a_transaction_and_restores_its_json() {
    let state = editor();
    assert_eq!((state.selection.from(), state.selection.to(&state.doc)), (2, 3));
    assert_eq!((state.selection.anchor(), state.selection.head(&state.doc)), (3, 2));
    assert!(!state.selection.empty());
    assert!(!state.selection.visible());
    assert_eq!(state.selection.ranges(&state.doc).unwrap().len(), 2);
    assert_eq!(state.selection.content(&state.doc).unwrap().content.first_child().unwrap().text_content(), "b");

    let mut transaction = state.tr();
    transaction.insert(1, Fragment::from_nodes(vec![state.schema.text("z", None)]));
    let mapped = state.apply(&transaction);
    assert_eq!(mapped.selection.as_custom::<DisjointSelection>().unwrap().positions(), [(3, 4), (6, 7)]);
    let json = mapped.to_json();
    assert_eq!(json["selection"], json!({"type": ID, "ranges": [[3, 4], [6, 7]]}));
    let restored = EditorState::from_json(&state.schema, &json).unwrap();
    assert_eq!(restored.selection, mapped.selection);
    assert!(restored.selection.as_custom::<DisjointSelection>().unwrap().head().doc().ptr_eq(&restored.doc));
}

#[test]
fn replaces_the_main_range_and_deletes_mapped_secondary_ranges() {
    let state = editor();
    let mut transaction = state.tr();
    let slice = Slice::new(Fragment::from_nodes(vec![state.schema.text("XYZ", None)]), 0, 0);
    transaction.replace_selection(slice);
    assert_eq!(transaction.doc().text_content(), "aXYZcdf");
    assert_eq!(transaction.selection(), Selection::cursor(5));

    let mut transaction = state.tr();
    transaction.replace_selection_with(state.schema.text("Q", None), false);
    assert_eq!(transaction.doc().text_content(), "aQcdf");
    assert_eq!(transaction.selection(), Selection::cursor(3));
}

#[test]
fn stores_a_document_independent_custom_bookmark_and_maps_it_before_resolving() {
    let state = editor();
    let saved = serde_json::to_value(state.selection.bookmark()).unwrap();
    assert_eq!(saved, json!({"bookmarkType": ID, "value": {"ranges": [[2, 3], [5, 6]]}}));
    let bookmark: SelectionBookmark = serde_json::from_value(saved).unwrap();
    assert!(bookmark.as_custom::<DisjointBookmark>().is_some());
    let mut transaction = state.tr();
    transaction.insert(1, Fragment::from_nodes(vec![state.schema.text("zzz", None)]));
    let mapped = bookmark.map(transaction.mapping()).resolve(transaction.doc());
    assert_eq!(mapped, transaction.selection());

    let builtin = json!({"type": "text", "anchor": 1, "head": 1});
    assert_eq!(serde_json::to_value(Selection::cursor(1).bookmark()).unwrap(), builtin);
    assert_eq!(serde_json::from_value::<SelectionBookmark>(builtin).unwrap().resolve(&state.doc), Selection::cursor(1));
}

#[test]
fn rejects_duplicate_unknown_or_invalid_custom_selection_json() {
    let state = editor();
    assert!(Selection::json_id(ID, decode).is_err());
    assert!(Selection::json_id("text", decode).is_err());
    assert!(Selection::from_json(&state.doc, &json!({"type":"undefined-selection"})).is_err());
    assert!(Selection::from_json(&state.doc, &json!({"type":ID,"ranges":[[2,1],[5,6]]})).is_err());
    assert!(Selection::from_json(&state.doc, &json!({"type":ID,"ranges":[[2,300],[5,6]]})).is_err());
}
