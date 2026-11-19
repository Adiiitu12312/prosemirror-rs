//! Browser cases ported from prosemirror-view 1.42.9 test/webtest-draw.ts.

use std::cell::RefCell;
use std::rc::Rc;

use prosemirror_model::{DomAttribute, DomOutputSpec, Fragment, MarkSpec, NodeSpec, Schema};
use prosemirror_state::{Plugin, PluginKey, Transaction};
use prosemirror_test_builder::{doc, h1, hr, p, strong_text, test_schema, TaggedDoc};
use prosemirror_view::browser::{BrowserView, BrowserViewProps, NodeView, PluginView};
use serde_json::json;
use wasm_bindgen_test::wasm_bindgen_test;
use web_sys::{Element, Node};

use crate::support::{mount, mount_state, state};

fn edit(view: &BrowserView, change: impl FnOnce(&mut Transaction)) {
    let mut tr = view.state().tr();
    change(&mut tr);
    assert!(view.dispatch(tr).unwrap());
}

fn element(view: &BrowserView, selector: &str) -> Element {
    view.dom().query_selector(selector).unwrap().unwrap()
}

fn same(view: &BrowserView, selector: &str, old: &Element) -> bool {
    element(view, selector).is_same_node(Some(old))
}

fn code(text: &str) -> prosemirror_model::Node {
    let schema = test_schema();
    schema.node("code_block", None, Some(Fragment::from_nodes(vec![schema.text(text, None)])), None).unwrap()
}

#[wasm_bindgen_test]
fn updates_the_dom() {
    let view = mount(doc!(p!("foo")), BrowserViewProps::default());
    edit(&view, |tr| { tr.insert_text_at_selection("bar"); });
    assert_eq!(view.dom().text_content().as_deref(), Some("barfoo"));
}

#[wasm_bindgen_test]
fn keeps_nodes_after_changes() {
    let view = mount(doc!(h1!("foo<a>"), p!("bar")), BrowserViewProps::default());
    let old = element(&view, "p");
    edit(&view, |tr| { tr.insert_text_at_selection("!"); });
    assert!(same(&view, "p", &old));
}

#[wasm_bindgen_test]
fn keeps_nodes_before_changes() {
    let view = mount(doc!(p!("foo"), h1!("bar")), BrowserViewProps::default());
    let old = element(&view, "p");
    edit(&view, |tr| { tr.insert_text("!", 2, 2); });
    assert!(same(&view, "p", &old));
}

#[wasm_bindgen_test]
fn keeps_nodes_between_changes() {
    let view = mount(doc!(p!("foo"), h1!("bar"), code("baz")), BrowserViewProps::default());
    let para = element(&view, "p");
    let pre = element(&view, "pre");
    edit(&view, |tr| { tr.insert_text("!", 2, 2); });
    assert!(same(&view, "p", &para));
    assert!(same(&view, "pre", &pre));
}

#[wasm_bindgen_test]
fn keeps_siblings_of_a_split_node() {
    let view = mount(doc!(p!("foo"), h1!("bar"), code("baz")), BrowserViewProps::default());
    let para = element(&view, "p");
    let pre = element(&view, "pre");
    edit(&view, |tr| { tr.apply_transform(|transform| { transform.split(8, 1); }); });
    assert!(same(&view, "p", &para));
    assert!(same(&view, "pre", &pre));
}

#[wasm_bindgen_test]
fn keeps_siblings_of_a_joined_node() {
    let view = mount(doc!(p!("foo"), h1!("bar"), h1!("x"), code("baz")), BrowserViewProps::default());
    let para = element(&view, "p");
    let pre = element(&view, "pre");
    edit(&view, |tr| { tr.apply_transform(|transform| { transform.join(10, 1); }); });
    assert!(same(&view, "p", &para));
    assert!(same(&view, "pre", &pre));
}

#[wasm_bindgen_test]
fn keeps_nodes_after_a_big_deletion() {
    let view = mount(doc!(p!(), p!(), p!(), p!(), p!(), p!(), p!(), p!(), h1!("!"), p!(), p!()), BrowserViewProps::default());
    let heading = element(&view, "h1");
    edit(&view, |tr| { tr.delete(2, 14); });
    assert!(same(&view, "h1", &heading));
}

#[wasm_bindgen_test]
fn adds_classes_from_the_attributes_prop() {
    let view = mount(doc!(p!()), BrowserViewProps {
        attributes: vec![Rc::new(|_| [("class".to_owned(), "foo bar".to_owned())].into())],
        ..BrowserViewProps::default()
    });
    let dom = view.dom();
    assert!(dom.class_list().contains("foo"));
    assert!(dom.class_list().contains("bar"));
    assert!(dom.class_list().contains("ProseMirror"));
    view.set_props(|props| props.attributes = vec![Rc::new(|_| [("class".to_owned(), "baz".to_owned())].into())]).unwrap();
    assert!(!dom.class_list().contains("foo"));
    assert!(dom.class_list().contains("baz"));
}

#[wasm_bindgen_test]
fn adds_style_from_the_attributes_prop() {
    let state = state(doc!(p!())).reconfigure(vec![
        Plugin::new(PluginKey::new("background"), None)
            .with_prop("attributes", |_| json!({"style":"background: red;"})),
        Plugin::new(PluginKey::new("color"), None)
            .with_prop("attributes", |_| json!({"style":"color: red;"})),
    ]).unwrap();
    let view = mount_state(state, BrowserViewProps {
        attributes: vec![Rc::new(|_| [("style".to_owned(), "border: 1px solid red;".to_owned())].into())],
        ..BrowserViewProps::default()
    });
    let style = view.dom().style();
    assert_eq!(style.get_property_value("border").unwrap(), "1px solid red");
    assert_eq!(style.get_property_value("background-color").unwrap(), "red");
    assert_eq!(style.get_property_value("color").unwrap(), "red");
}

#[wasm_bindgen_test]
fn can_set_other_attributes() {
    let view = mount(doc!(p!()), BrowserViewProps {
        attributes: vec![Rc::new(|_| [
            ("spellcheck".to_owned(), "false".to_owned()), ("aria-label".to_owned(), "hello".to_owned()),
        ].into())],
        ..BrowserViewProps::default()
    });
    let dom = view.dom();
    assert!(!dom.spellcheck());
    assert_eq!(dom.get_attribute("aria-label").as_deref(), Some("hello"));
    view.set_props(|props| props.attributes = vec![Rc::new(|_| [
        ("style".to_owned(), "background-color: yellow".to_owned()),
    ].into())]).unwrap();
    assert!(!dom.has_attribute("aria-label"));
    assert_eq!(dom.style().get_property_value("background-color").unwrap(), "yellow");
}

#[wasm_bindgen_test]
fn attributes_cannot_set_contenteditable() {
    let view = mount(doc!(p!()), BrowserViewProps {
        attributes: vec![Rc::new(|_| [("contenteditable".to_owned(), "false".to_owned())].into())],
        ..BrowserViewProps::default()
    });
    assert_eq!(view.dom().content_editable(), "true");
}

#[wasm_bindgen_test]
fn understands_the_editable_prop() {
    let view = mount(doc!(p!()), BrowserViewProps { editable: Some(Rc::new(|_| false)), ..BrowserViewProps::default() });
    assert_eq!(view.dom().content_editable(), "false");
    view.set_props(|props| props.editable = None).unwrap();
    assert_eq!(view.dom().content_editable(), "true");
}

#[wasm_bindgen_test]
fn keeps_following_paragraphs_when_one_is_split() {
    let view = mount(doc!(p!("abcde"), p!("fg")), BrowserViewProps::default());
    let end = view.dom().last_child().unwrap();
    edit(&view, |tr| { tr.apply_transform(|transform| { transform.split(3, 1); }); });
    assert!(view.dom().last_child().unwrap().is_same_node(Some(&end)));
}

#[wasm_bindgen_test]
fn preserves_a_node_matched_later_when_splitting() {
    let view = mount(doc!(p!("a"), p!("b"), p!()), BrowserViewProps::default());
    let second = view.dom().query_selector_all("p").unwrap().item(1).unwrap();
    edit(&view, |tr| { tr.apply_transform(|transform| { transform.split(2, 1); }); });
    let third = view.dom().query_selector_all("p").unwrap().item(2).unwrap();
    assert!(third.is_same_node(Some(&second)));
}

struct Lifecycle(Rc<RefCell<Vec<&'static str>>>);

impl PluginView for Lifecycle {
    fn update(&mut self, _view: &BrowserView, _previous: &prosemirror_state::EditorState) {
        self.0.borrow_mut().push("update");
    }
    fn destroy(&mut self) {
        self.0.borrow_mut().push("destroy");
    }
}

#[wasm_bindgen_test]
fn creates_and_destroys_plugin_views() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let observed = events.clone();
    let key = PluginKey::new("lifecycle");
    let state = state(doc!(p!())).reconfigure(vec![Plugin::new(key.clone(), None)]).unwrap();
    let props = Rc::new(BrowserViewProps {
        plugin_views: vec![Rc::new(move |_| {
            observed.borrow_mut().push("create");
            Box::new(Lifecycle(observed.clone()))
        })],
        ..BrowserViewProps::default()
    });
    let view = mount_state(state, BrowserViewProps {
        state_plugins: [(key, props)].into(),
        ..BrowserViewProps::default()
    });
    edit(&view, |tr| { tr.insert_text_at_selection("u"); });
    view.destroy();
    assert_eq!(*events.borrow(), ["create", "update", "destroy"]);
}

#[wasm_bindgen_test]
fn redraws_changed_node_views() {
    let view = mount(doc!(p!("foo"), hr()), BrowserViewProps::default());
    assert!(view.dom().query_selector("hr").unwrap().is_some());
    view.set_props(|props| {
        props.node_views.insert("horizontal_rule".to_owned(), Rc::new(|document, _, _, _, _| {
            Ok(NodeView::new(document.create_element("var")?))
        }));
    }).unwrap();
    assert!(view.dom().query_selector("hr").unwrap().is_none());
    assert!(view.dom().query_selector("var").unwrap().is_some());
}

#[wasm_bindgen_test]
fn merges_adjacent_text_when_a_mark_is_removed() {
    let view = mount(doc!(p!(strong_text("one"), " two ", strong_text("three"))), BrowserViewProps::default());
    let mark = view.state().schema.mark_type("strong").unwrap().create(None);
    edit(&view, |tr| { tr.remove_mark(1, 4, mark); });
    assert_eq!(view.dom().query_selector_all("strong").unwrap().length(), 1);
}

#[wasm_bindgen_test]
fn preserves_unedited_marked_blocks() {
    let schema = Schema::new(vec![
        ("doc".to_owned(), NodeSpec { content: Some("paragraph+".to_owned()), marks: Some("m".to_owned()), ..Default::default() }),
        ("text".to_owned(), NodeSpec { group: Some("inline".to_owned()), ..Default::default() }),
        ("paragraph".to_owned(), test_schema().node_type("paragraph").unwrap().spec.clone()),
    ], vec![("m".to_owned(), MarkSpec {
        to_dom: Some(|_, _| DomOutputSpec::element_with_attrs(
            "div", vec![DomAttribute::new("class", "m")], vec![DomOutputSpec::Hole],
        )),
        ..Default::default()
    })]);
    let mark = schema.mark_type("m").unwrap().create(None);
    let paragraph = |index| schema.node("paragraph", None, Some(Fragment::from_nodes(vec![
        schema.text(&format!("para {index}"), None),
    ])), Some(vec![mark.clone()])).unwrap();
    let initial = (1..=10).map(paragraph).collect();
    let doc = schema.node("doc", None, Some(Fragment::from_nodes(initial)), None).unwrap();
    let view = mount(TaggedDoc { node: doc, tags: Default::default() }, BrowserViewProps::default());
    let before = view.dom().query_selector_all("p").unwrap();
    let old: Vec<Node> = (0..before.length()).map(|i| before.item(i).unwrap()).collect();
    let replacement = Fragment::from_nodes((-6..0).map(paragraph).collect());
    edit(&view, |tr| { tr.replace_with(0, 8, replacement); });
    let after = view.dom().query_selector_all("p").unwrap();
    let current: Vec<Node> = (0..after.length()).map(|i| after.item(i).unwrap()).collect();
    let same = old.iter().rev().zip(current.iter().rev()).take_while(|(old, current)| old.is_same_node(Some(current))).count();
    assert_eq!(same, 9);
}
