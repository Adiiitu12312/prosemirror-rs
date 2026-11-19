//! Browser cases ported from prosemirror-view 1.42.9 test/webtest-nodeview.ts.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use prosemirror_state::Transaction;
use prosemirror_test_builder::{blockquote, doc, p, test_schema};
use prosemirror_view::browser::{BrowserView, BrowserViewProps, GetPos, NodeView, NodeViewFactory};
use prosemirror_view::{Decoration, DecorationGroup, DecorationSet};
use serde_json::{json, Value};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::wasm_bindgen_test;
use web_sys::Element;

use crate::support::mount;

fn props(name: &str, factory: NodeViewFactory) -> BrowserViewProps {
    BrowserViewProps {
        node_views: [(name.to_owned(), factory)].into(),
        ..BrowserViewProps::default()
    }
}

fn edit(view: &BrowserView, change: impl FnOnce(&mut Transaction)) {
    let mut tr = view.state().tr();
    change(&mut tr);
    assert!(view.dispatch(tr).unwrap());
}

fn element(view: &BrowserView, selector: &str) -> Element {
    view.dom().query_selector(selector).unwrap().unwrap()
}

fn br() -> prosemirror_model::Node {
    test_schema().node("hard_break", None, None, None).unwrap()
}

fn text(view: &BrowserView, selector: &str) -> String {
    element(view, selector).text_content().unwrap()
}

fn sample_decorations() -> Vec<Decoration> {
    vec![
        Decoration::inline(2, 3, [("someattr".into(), "ok".into())].into(), Value::Null),
        Decoration::node(0, 5, [("otherattr".into(), "ok".into())].into(), Value::Null),
    ]
}

fn from_inner(inner: &DecorationGroup) -> Vec<(usize, usize)> {
    inner.members.iter().flat_map(|set| set.find(None, None)).map(|deco| (deco.from, deco.to)).collect()
}

#[wasm_bindgen_test]
fn can_replace_a_nodes_representation() {
    let view = mount(doc!(p!("foo", br())), props("hard_break", Rc::new(|doc, _, _, _, _| {
        Ok(NodeView::new(doc.create_element("var")?))
    })));
    assert!(view.dom().query_selector("var").unwrap().is_some());
}

#[wasm_bindgen_test]
fn can_override_drawing_of_a_nodes_content() {
    let view = mount(doc!(p!("foo")), props("paragraph", Rc::new(|doc, node, _, _, _| {
        let dom = doc.create_element("p")?;
        dom.set_text_content(Some(&node.text_content().to_uppercase()));
        Ok(NodeView::new(dom))
    })));
    assert_eq!(text(&view, "p"), "FOO");
    edit(&view, |tr| { tr.insert_text_at_selection("a"); });
    assert_eq!(text(&view, "p"), "AFOO");
}

#[wasm_bindgen_test]
fn can_register_its_own_update_method() {
    let view = mount(doc!(p!("foo")), props("paragraph", Rc::new(|doc, node, _, _, _| {
        let dom = doc.create_element("p")?;
        dom.set_text_content(Some(&node.text_content().to_uppercase()));
        let mut result = NodeView::new(dom.clone());
        result.update = Some(Rc::new(move |node, _, _| {
            dom.set_text_content(Some(&node.text_content().to_uppercase()));
            true
        }));
        Ok(result)
    })));
    let para = element(&view, "p");
    edit(&view, |tr| { tr.insert_text_at_selection("a"); });
    assert!(element(&view, "p").is_same_node(Some(&para)));
    assert_eq!(para.text_content().as_deref(), Some("AFOO"));
}

#[wasm_bindgen_test]
fn allows_decoration_updates_with_an_update_method() {
    let view = mount(doc!(p!("foo")), props("paragraph", Rc::new(|doc, node, _, _, _| {
        let dom = doc.create_element("p")?;
        let mut result = NodeView::new(dom.clone()).with_content_dom(dom);
        let old = node.clone();
        result.update = Some(Rc::new(move |next, _, _| old.same_markup(next)));
        Ok(result)
    })));
    view.set_props(|props| props.decorations = vec![Rc::new(|state| Some(DecorationSet::create(&state.doc, sample_decorations())))]).unwrap();
    assert!(view.dom().query_selector("[someattr]").unwrap().is_some());
    assert!(view.dom().query_selector("[otherattr]").unwrap().is_some());
}

#[wasm_bindgen_test]
fn can_provide_a_content_dom_property() {
    let view = mount(doc!(p!("foo")), props("paragraph", Rc::new(|doc, _, _, _, _| {
        let dom = doc.create_element("p")?;
        Ok(NodeView::new(dom.clone()).with_content_dom(dom))
    })));
    let para = element(&view, "p");
    edit(&view, |tr| { tr.insert_text_at_selection("a"); });
    assert!(element(&view, "p").is_same_node(Some(&para)));
    assert_eq!(para.text_content().as_deref(), Some("afoo"));
}

#[wasm_bindgen_test]
fn has_its_ignore_mutation_method_called() {
    let mutation = Rc::new(RefCell::new(None));
    let observed = mutation.clone();
    let view = mount(doc!(p!("foo")), props("paragraph", Rc::new(move |doc, _, _, _, _| {
        let dom = doc.create_element("div")?;
        let content = doc.create_element("p")?;
        dom.append_child(&content)?;
        dom.append_child(&doc.create_element("x-info")?)?;
        let mut result = NodeView::new(dom).with_content_dom(content);
        let target = observed.clone();
        result.ignore_mutation = Some(Rc::new(move |mutation| {
            *target.borrow_mut() = mutation.target().and_then(|node| node.dyn_into::<Element>().ok()).map(|element| element.tag_name());
            true
        }));
        Ok(result)
    })));
    assert!(mutation.borrow().is_none());
    element(&view, "x-info").set_text_content(Some("info"));
    view.flush_dom_changes().unwrap();
    assert_eq!(mutation.borrow().as_deref(), Some("X-INFO"));
}

#[wasm_bindgen_test]
fn has_its_destroy_method_called() {
    let destroyed = Rc::new(Cell::new(false));
    let captured = destroyed.clone();
    let view = mount(doc!(p!("foo", br())), props("hard_break", Rc::new(move |doc, _, _, _, _| {
        let mut result = NodeView::new(doc.create_element("br")?);
        let observed = captured.clone();
        result.destroy = Some(Rc::new(move || observed.set(true)));
        Ok(result)
    })));
    assert!(!destroyed.get());
    edit(&view, |tr| { tr.delete(3, 5); });
    assert!(destroyed.get());
}

#[wasm_bindgen_test]
fn can_query_its_own_position() {
    let getter: Rc<RefCell<Option<GetPos>>> = Rc::new(RefCell::new(None));
    let observed = getter.clone();
    let view = mount(doc!(blockquote!(p!("abc"), p!("foo", br()))), props("hard_break", Rc::new(move |doc, _, position, _, _| {
        assert_eq!(position(), Some(10));
        *observed.borrow_mut() = Some(position);
        Ok(NodeView::new(doc.create_element("br")?))
    })));
    assert_eq!(getter.borrow().as_ref().unwrap()(), Some(10));
    edit(&view, |tr| { tr.insert_text_at_selection("a"); });
    assert_eq!(getter.borrow().as_ref().unwrap()(), Some(11));
}

#[wasm_bindgen_test]
fn has_access_to_outer_decorations() {
    let active = Rc::new(RefCell::new(None::<String>));
    let current = active.clone();
    let mut props = props("hard_break", Rc::new(|doc, _, _, decoration, _| {
        let dom = doc.create_element("var")?;
        let draw = move |dom: &Element, deco: &[Decoration]| {
            let name = deco.first().and_then(|deco| deco.spec()["name"].as_str()).unwrap_or("[]");
            dom.set_text_content(Some(name));
        };
        draw(&dom, decoration);
        let mut node = NodeView::new(dom.clone());
        node.update = Some(Rc::new(move |_, deco, _| { draw(&dom, deco); true }));
        Ok(node)
    }));
    props.decorations.push(Rc::new(move |state| {
        let name = current.borrow().clone()?;
        Some(DecorationSet::create(&state.doc, vec![Decoration::inline(
            0, state.doc.content().size(), Default::default(), json!({"name":name}),
        )]))
    }));
    let view = mount(doc!(p!("foo", br())), props);
    assert_eq!(text(&view, "var"), "[]");
    for value in ["foo", "bar"] {
        *active.borrow_mut() = Some(value.to_owned());
        edit(&view, |tr| { tr.set_meta("setDeco", json!(value)); });
        assert_eq!(text(&view, "var"), value);
    }
}

#[wasm_bindgen_test]
fn provides_access_to_inner_decorations_in_the_constructor() {
    let mut props = props("paragraph", Rc::new(|doc, _, _, _, inner| {
        assert_eq!(from_inner(inner), [(1, 2)]);
        let dom = doc.create_element("p")?;
        Ok(NodeView::new(dom.clone()).with_content_dom(dom))
    }));
    props.decorations.push(Rc::new(|state| Some(DecorationSet::create(&state.doc, sample_decorations()))));
    let _view = mount(doc!(p!("foo")), props);
}

#[wasm_bindgen_test]
fn provides_access_to_inner_decorations_in_the_update_method() {
    let values = Rc::new(RefCell::new(Vec::new()));
    let captured = values.clone();
    let view = mount(doc!(p!("foo")), props("paragraph", Rc::new(move |doc, node, _, _, _| {
        let dom = doc.create_element("p")?;
        let mut result = NodeView::new(dom.clone()).with_content_dom(dom);
        let observed = captured.clone();
        let node = node.clone();
        result.update = Some(Rc::new(move |next, _, inner| {
            *observed.borrow_mut() = from_inner(inner);
            node.same_markup(next)
        }));
        Ok(result)
    })));
    view.set_props(|props| props.decorations = vec![Rc::new(|state| Some(DecorationSet::create(&state.doc, sample_decorations())))]).unwrap();
    assert_eq!(*values.borrow(), [(1, 2)]);
}
