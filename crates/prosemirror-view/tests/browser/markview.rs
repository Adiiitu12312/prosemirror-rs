//! Browser cases ported from prosemirror-view 1.42.9 test/webtest-markview.ts.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use prosemirror_model::Mark;
use prosemirror_state::Transaction;
use prosemirror_test_builder::{doc, p, strong_text, test_schema};
use prosemirror_view::browser::{BrowserView, BrowserViewProps, MarkView, MarkViewFactory};
use serde_json::json;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::wasm_bindgen_test;
use web_sys::Element;

use crate::support::mount;

fn props(mark: &str, factory: MarkViewFactory) -> BrowserViewProps {
    BrowserViewProps {
        mark_views: [(mark.to_owned(), factory)].into(),
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

fn link(url: &str) -> Mark {
    test_schema().mark_type("link").unwrap().create(Some(json!({"href":url})))
}

fn initial_link() -> prosemirror_model::Node {
    test_schema().text("foo", Some(vec![link("foo")]))
}

fn href(mark: &Mark) -> &str {
    mark.attrs.as_ref().unwrap()["href"].as_str().unwrap()
}

#[wasm_bindgen_test]
fn can_replace_a_marks_representation() {
    let view = mount(doc!(p!("foo", strong_text("bar"))), props("strong", Rc::new(|doc, _, _| {
        Ok(MarkView::new(doc.create_element("var")?))
    })));
    assert!(view.dom().query_selector("var").unwrap().is_some());
}

#[wasm_bindgen_test]
fn can_provide_a_content_dom_property() {
    let view = mount(doc!(p!(strong_text("foo"))), props("strong", Rc::new(|doc, _, _| {
        let dom = doc.create_element("var")?;
        let span = doc.create_element("span")?;
        dom.append_child(&span)?;
        Ok(MarkView::new(dom).with_content_dom(span))
    })));
    let span = element(&view, "span");
    edit(&view, |tr| { tr.insert_text("a", 2, 2); });
    assert!(element(&view, "span").is_same_node(Some(&span)));
    assert_eq!(span.text_content().as_deref(), Some("faoo"));
}

#[wasm_bindgen_test]
fn has_its_ignore_mutation_method_called() {
    let observed = Rc::new(RefCell::new(None));
    let captured = observed.clone();
    let view = mount(doc!(p!("foo", strong_text("bar"))), props("strong", Rc::new(move |doc, _, _| {
        let mut result = MarkView::new(doc.create_element("var")?);
        let target = captured.clone();
        result.ignore_mutation = Some(Rc::new(move |mutation| {
            *target.borrow_mut() = mutation.target().and_then(|node| node.dyn_into::<Element>().ok()).map(|element| element.tag_name());
            true
        }));
        Ok(result)
    })));
    assert!(observed.borrow().is_none());
    element(&view, "var").class_list().add_1("foo").unwrap();
    view.flush_dom_changes().unwrap();
    assert_eq!(observed.borrow().as_deref(), Some("VAR"));
}

#[wasm_bindgen_test]
fn has_its_destroy_method_called() {
    let destroyed = Rc::new(Cell::new(false));
    let captured = destroyed.clone();
    let view = mount(doc!(p!(strong_text("foo"))), props("strong", Rc::new(move |doc, _, _| {
        let mut result = MarkView::new(doc.create_element("var")?);
        let state = captured.clone();
        result.destroy = Some(Rc::new(move || state.set(true)));
        Ok(result)
    })));
    assert_eq!(view.dom().text_content().as_deref(), Some("foo"));
    assert!(!destroyed.get());
    edit(&view, |tr| { tr.delete(1, 2); });
    assert_eq!(view.dom().text_content().as_deref(), Some("oo"));
    assert!(!destroyed.get());
    edit(&view, |tr| { tr.delete(1, 3); });
    assert_eq!(view.dom().text_content().as_deref(), Some(""));
    assert!(destroyed.get());
}

#[wasm_bindgen_test]
fn reuses_the_dom_and_calls_update_when_only_a_mark_attr_changes() {
    let updates = Rc::new(RefCell::new(Vec::new()));
    let captured = updates.clone();
    let view = mount(doc!(p!(initial_link())), props("link", Rc::new(move |doc, mark, _| {
        let dom = doc.create_element("span")?;
        dom.set_attribute("data-href", href(mark))?;
        let mut result = MarkView::new(dom.clone());
        let updates = captured.clone();
        result.update = Some(Rc::new(move |mark| {
            let href = href(mark);
            updates.borrow_mut().push(href.to_owned());
            dom.set_attribute("data-href", href).unwrap();
            true
        }));
        Ok(result)
    })));
    let span = element(&view, "span");
    edit(&view, |tr| { tr.remove_mark(1, 4, link("foo")).add_mark(1, 4, link("bar")); });
    assert!(element(&view, "span").is_same_node(Some(&span)));
    assert_eq!(span.get_attribute("data-href").as_deref(), Some("bar"));
    assert_eq!(*updates.borrow(), ["bar"]);
}

#[wasm_bindgen_test]
fn rebuilds_a_mark_view_without_an_update_method() {
    let view = mount(doc!(p!(initial_link())), props("link", Rc::new(|doc, mark, _| {
        let dom = doc.create_element("span")?;
        dom.set_attribute("data-href", href(mark))?;
        Ok(MarkView::new(dom))
    })));
    let span = element(&view, "span");
    edit(&view, |tr| { tr.remove_mark(1, 4, link("foo")).add_mark(1, 4, link("bar")); });
    let current = element(&view, "span");
    assert!(!current.is_same_node(Some(&span)));
    assert_eq!(current.get_attribute("data-href").as_deref(), Some("bar"));
}
