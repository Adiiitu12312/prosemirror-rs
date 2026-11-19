//! Port of prosemirror-view 1.42.9 test/webtest-draw-decoration.ts.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use prosemirror_model::{DomOutputSpec, Fragment, NodeSpec, Schema};
use prosemirror_state::{Plugin, PluginKey, Selection, Transaction};
use prosemirror_test_builder::{blockquote, doc, em_text as em, h1, hr, p, strong_text as strong, test_schema, TaggedDoc};
use prosemirror_view::browser::{BrowserViewProps, GetPos, NodeView, WidgetView, WidgetViewFactory};
use prosemirror_view::{Decoration, DecorationAttrs, DecorationGroup, DecorationSet};
use serde_json::{json, Value};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::wasm_bindgen_test;
use web_sys::{Element, HtmlElement};

use crate::support::{mount, mount_state, state, TestView};

fn attrs(pairs: &[(&str, &str)]) -> DecorationAttrs {
    pairs.iter().map(|(name, val)| ((*name).into(), (*val).into())).collect()
}

fn inline(from: usize, to: usize, class: &str) -> Decoration {
    Decoration::inline(from, to, attrs(&[("class", class)]), Value::Null)
}

fn widget(pos: usize, id: u64) -> Decoration {
    Decoration::widget(pos, id, Value::Null)
}

fn widget_element(tag: &'static str, text: Option<&'static str>) -> WidgetViewFactory {
    Rc::new(move |doc, _, _, _| {
        let el = doc.create_element(tag)?;
        el.set_text_content(text);
        Ok(WidgetView::new(el))
    })
}

fn widget_text(text: &'static str) -> WidgetViewFactory {
    Rc::new(move |doc, _, _, _| Ok(WidgetView::new(doc.create_text_node(text))))
}

fn setup(fixture: TaggedDoc, decorations: Vec<Decoration>) -> (TestView, Rc<RefCell<DecorationSet>>) {
    setup_with(fixture, decorations, BrowserViewProps::default())
}

fn setup_with(fixture: TaggedDoc, decorations: Vec<Decoration>, mut props: BrowserViewProps)
    -> (TestView, Rc<RefCell<DecorationSet>>)
{
    let set = Rc::new(RefCell::new(DecorationSet::create(&fixture.node, decorations)));
    let source = set.clone();
    props.decorations.insert(0, Rc::new(move |_| Some(source.borrow().clone())));
    for id in 1..=3 {
        props.widgets.entry(id).or_insert_with(|| widget_element("button", Some("ω")));
    }
    (mount(fixture, props), set)
}

fn update(view: &TestView, set: &RefCell<DecorationSet>, add: Vec<Decoration>, remove: &[Decoration]) {
    edit(view, set, add, remove, |_| {});
}

fn edit(
    view: &TestView,
    set: &RefCell<DecorationSet>,
    add: Vec<Decoration>,
    remove: &[Decoration],
    edit: impl FnOnce(&mut Transaction),
) {
    let mut tr = view.state().tr();
    edit(&mut tr);
    let next = set.borrow().map(tr.mapping(), tr.doc()).remove(remove).add(tr.doc(), add);
    set.replace(next);
    view.dispatch(tr).unwrap();
}

fn one(view: &TestView, selector: &str) -> Element {
    view.dom().query_selector(selector).unwrap().unwrap()
}

fn all(view: &TestView, selector: &str) -> Vec<Element> {
    let nodes = view.dom().query_selector_all(selector).unwrap();
    (0..nodes.length()).map(|index| nodes.item(index).unwrap().unchecked_into()).collect()
}

fn contents(nodes: &[Element]) -> Vec<String> {
    nodes.iter().map(|node| node.text_content().unwrap()).collect()
}

fn style(node: &Element, property: &str) -> String {
    node.unchecked_ref::<HtmlElement>().style().get_property_value(property).unwrap()
}

#[wasm_bindgen_test]
fn draws_inline_decorations() {
    let (view, _) = setup(doc!(p!("foobar")), vec![inline(2, 5, "foo")]);
    assert_eq!(one(&view, ".foo").text_content().unwrap(), "oob");
}

#[wasm_bindgen_test]
fn draws_wrapping_decorations() {
    let (view, _) = setup(doc!(p!("foo")), vec![
        Decoration::inline(1, 5, attrs(&[("nodeName", "i")]), Value::Null),
    ]);
    assert_eq!(one(&view, "i").inner_html(), "foo");
}

#[wasm_bindgen_test]
fn draws_node_decorations() {
    let (view, _) = setup(doc!(p!("foo"), p!("bar")), vec![
        Decoration::node(5, 10, attrs(&[("class", "cls")]), Value::Null),
    ]);
    let found = all(&view, ".cls");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].node_name(), "P");
    assert_eq!(found[0].previous_sibling().unwrap().node_name(), "P");
}

#[wasm_bindgen_test]
fn updates_multilevel_wrapping_decorations() {
    let wrap = |class| Decoration::inline(1, 5, attrs(&[("nodeName", "i"), ("class", class)]), Value::Null);
    let second = wrap("b");
    let (view, set) = setup(doc!(p!("hello")), vec![wrap("a"), second.clone()]);
    assert_eq!(all(&view, "i").len(), 2);
    update(&view, &set, vec![wrap("c")], &[second]);
    let mut classes: Vec<_> = all(&view, "i").into_iter().map(|node| node.class_name()).collect();
    classes.sort();
    assert_eq!(classes, ["a", "c"]);
}

#[wasm_bindgen_test]
fn draws_overlapping_inline_decorations() {
    let (view, _) = setup(doc!(p!("abcdef")), vec![inline(3, 5, "foo"), inline(4, 6, "bar"), inline(1, 7, "baz")]);
    let decorated = all(&view, ".baz");
    assert_eq!(contents(&decorated), ["ab", "c", "d", "e", "f"]);
    let classes = |node: &Element| {
        let class = node.class_name();
        let mut names: Vec<_> = class.split_whitespace().collect();
        names.sort_unstable();
        names.join(" ")
    };
    assert_eq!(classes(&decorated[1]), "baz foo");
    assert_eq!(classes(&decorated[2]), "bar baz foo");
    assert_eq!(classes(&decorated[3]), "bar baz");
}

#[wasm_bindgen_test]
fn draws_multiple_widgets() {
    let (view, _) = setup(doc!(p!("foobar")), vec![widget(1, 1), widget(4, 2), widget(7, 3)]);
    let found = all(&view, "button");
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].next_sibling().unwrap().text_content().unwrap(), "foo");
    assert_eq!(found[1].next_sibling().unwrap().text_content().unwrap(), "bar");
    assert_eq!(found[2].previous_sibling().unwrap().text_content().unwrap(), "bar");
}

#[wasm_bindgen_test]
fn orders_widgets_by_side() {
    let (view, _) = setup_with(doc!(p!("foobar")), vec![
        widget(4, 1),
        Decoration::widget(4, 2, json!({"side":-100})),
        Decoration::widget(4, 3, json!({"side":2})),
    ], BrowserViewProps { widgets: [(1, widget_text("B")), (2, widget_text("A")), (3, widget_text("C"))].into(),
        ..BrowserViewProps::default() });
    assert_eq!(view.dom().text_content().unwrap(), "fooABCbar");
}

#[wasm_bindgen_test]
fn draws_a_widget_in_an_empty_node() {
    let (view, _) = setup(doc!(p!()), vec![widget(1, 1)]);
    assert_eq!(all(&view, "button").len(), 1);
}

#[wasm_bindgen_test]
fn draws_widgets_on_node_boundaries() {
    let (view, _) = setup(doc!(p!("foo", em("bar"))), vec![widget(4, 1)]);
    assert_eq!(all(&view, "button").len(), 1);
}

#[wasm_bindgen_test]
fn draws_decorations_from_multiple_plugins() {
    let plugin = |key, pos, id| {
        let key = PluginKey::new(key);
        let props = Rc::new(BrowserViewProps {
            decorations: vec![Rc::new(move |state| Some(DecorationSet::create(&state.doc, vec![widget(pos, id)])))],
            widgets: [(id, widget_element("button", Some("ω")))].into(),
            ..BrowserViewProps::default()
        });
        (key, props)
    };
    let sources = [plugin("first", 2, 1), plugin("second", 6, 2)];
    let state_plugins = sources.iter().map(|(key, _)| Plugin::new(key.clone(), None)).collect();
    let state = state(doc!(p!("foo", em("bar")))).reconfigure(state_plugins).unwrap();
    let view = mount_state(state, BrowserViewProps {
        state_plugins: sources.into(),
        ..BrowserViewProps::default()
    });
    assert_eq!(all(&view, "button").len(), 2);
}

#[wasm_bindgen_test]
fn calls_widget_destroy_methods() {
    let destroyed = Rc::new(Cell::new(false));
    let effect = destroyed.clone();
    let factory: WidgetViewFactory = Rc::new(move |doc, _, _, _| {
        let effect = effect.clone();
        let mut widget = WidgetView::new(doc.create_element("button")?);
        widget.destroy = Some(Rc::new(move |dom| {
            assert_eq!(dom.node_name(), "BUTTON");
            effect.set(true);
        }));
        Ok(widget)
    });
    let (view, set) = setup_with(doc!(p!("abc")), vec![widget(2, 1)], BrowserViewProps {
        widgets: [(1, factory)].into(), ..BrowserViewProps::default()
    });
    edit(&view, &set, vec![], &[], |tr| { tr.delete(1, 4); });
    assert!(destroyed.get());
}

#[wasm_bindgen_test]
fn draws_inline_decorations_spanning_multiple_parents() {
    let (view, _) = setup(doc!(p!("long first ", em("p"), "aragraph"), p!("two")), vec![inline(7, 25, "foo")]);
    assert_eq!(contents(&all(&view, ".foo")), ["irst ", "p", "aragraph", "tw"]);
}

#[wasm_bindgen_test]
fn draws_inline_decorations_across_empty_paragraphs() {
    let (view, _) = setup(doc!(p!("first"), p!(), p!("second")), vec![inline(3, 12, "foo")]);
    assert_eq!(contents(&all(&view, ".foo")), ["rst", "se"]);
}

#[wasm_bindgen_test]
fn handles_inline_decorations_ending_at_node_boundaries() {
    let (view, _) = setup(doc!(p!(), p!()), vec![inline(1, 3, "foo")]);
    assert!(all(&view, ".foo").is_empty());
}

#[wasm_bindgen_test]
fn draws_decorations_with_multiple_classes() {
    let (view, _) = setup(doc!(p!("foo")), vec![inline(1, 4, "foo bar")]);
    assert_eq!(all(&view, ".foo").len(), 1);
    assert_eq!(all(&view, ".bar").len(), 1);
}

#[wasm_bindgen_test]
fn supports_overlapping_inline_decorations() {
    let (view, _) = setup(doc!(p!("foobar")), vec![inline(1, 3, "foo"), inline(2, 5, "bar")]);
    assert_eq!(contents(&all(&view, ".foo")), ["f", "o"]);
    assert_eq!(contents(&all(&view, ".bar")), ["o", "ob"]);
}

#[wasm_bindgen_test]
fn preserves_nodes_when_irrelevant_decorations_change() {
    let (view, set) = setup(doc!(p!("foo"), p!("baz")), vec![inline(7, 8, "foo")]);
    let paragraph = view.dom().last_child().unwrap();
    update(&view, &set, vec![inline(2, 3, "bar")], &[]);
    assert!(paragraph.is_same_node(view.dom().last_child().as_ref()));
    assert_eq!(all(&view, ".bar").len(), 1);
}

#[wasm_bindgen_test]
fn preserves_nodes_when_irrelevant_content_changes() {
    let (view, set) = setup(doc!(p!("foo"), p!("baz")), vec![inline(7, 8, "foo")]);
    let paragraph = view.dom().last_child().unwrap();
    for _ in 0..2 {
        edit(&view, &set, vec![], &[], |tr| { tr.delete(2, 3); });
    }
    assert!(paragraph.is_same_node(view.dom().last_child().as_ref()));
}

#[wasm_bindgen_test]
fn adds_a_widget_on_a_node_boundary() {
    let (view, set) = setup(doc!(p!("foo", em("bar"))), vec![]);
    update(&view, &set, vec![widget(4, 1)], &[]);
    assert_eq!(all(&view, "button").len(), 1);
}

#[wasm_bindgen_test]
fn removes_a_widget_on_a_node_boundary() {
    let dec = widget(4, 1);
    let (view, set) = setup(doc!(p!("foo", em("bar"))), vec![dec.clone()]);
    update(&view, &set, vec![], &[dec]);
    assert!(all(&view, "button").is_empty());
}

#[wasm_bindgen_test]
fn removes_the_class_from_a_text_node() {
    let dec = inline(1, 4, "foo");
    let (view, set) = setup(doc!(p!("abc")), vec![dec.clone()]);
    assert_eq!(all(&view, ".foo").len(), 1);
    update(&view, &set, vec![], &[dec]);
    assert!(all(&view, ".foo").is_empty());
}

#[wasm_bindgen_test]
fn removes_the_class_from_part_of_a_text_node() {
    let dec = inline(2, 4, "foo");
    let (view, set) = setup(doc!(p!("abcd")), vec![dec.clone()]);
    assert_eq!(all(&view, ".foo").len(), 1);
    update(&view, &set, vec![], &[dec]);
    assert!(all(&view, ".foo").is_empty());
    assert_eq!(one(&view, "p").inner_html(), "abcd");
}

#[wasm_bindgen_test]
fn changes_the_class_for_part_of_a_text_node() {
    let dec = inline(2, 4, "foo");
    let (view, set) = setup(doc!(p!("abcd")), vec![dec.clone()]);
    assert_eq!(all(&view, ".foo").len(), 1);
    update(&view, &set, vec![inline(2, 4, "bar")], &[dec]);
    assert!(all(&view, ".foo").is_empty());
    assert_eq!(all(&view, ".bar").len(), 1);
}

#[wasm_bindgen_test]
fn draws_a_widget_added_in_the_middle_of_a_text_node() {
    let (view, set) = setup(doc!(p!("foo")), vec![]);
    update(&view, &set, vec![widget(3, 1)], &[]);
    assert_eq!(one(&view, "p").text_content().unwrap(), "foωo");
}

#[wasm_bindgen_test]
fn updates_a_text_node_around_a_widget() {
    let (view, set) = setup(doc!(p!("bar")), vec![widget(3, 1)]);
    edit(&view, &set, vec![], &[], |tr| { tr.delete(1, 2); });
    assert_eq!(all(&view, "button").len(), 1);
    assert_eq!(one(&view, "p").text_content().unwrap(), "aωr");
}

#[wasm_bindgen_test]
fn updates_a_text_node_with_an_inline_decoration() {
    let (view, set) = setup(doc!(p!("bar")), vec![inline(1, 3, "foo")]);
    edit(&view, &set, vec![], &[], |tr| { tr.delete(1, 2); });
    let decorated = one(&view, ".foo");
    assert_eq!(decorated.text_content().unwrap(), "a");
    assert_eq!(decorated.next_sibling().unwrap().text_content().unwrap(), "r");
}

#[wasm_bindgen_test]
fn redraws_a_partially_decorated_node_when_a_widget_is_added() {
    let (view, set) = setup(doc!(p!("one", em("two"))), vec![inline(1, 6, "foo")]);
    update(&view, &set, vec![widget(6, 1)], &[]);
    assert_eq!(contents(&all(&view, ".foo")), ["one", "tw"]);
}

#[wasm_bindgen_test]
fn redraws_when_skipping_a_split_text_node() {
    let (view, set) = setup(doc!(p!("foo")), vec![widget(3, 1), inline(3, 4, "foo")]);
    update(&view, &set, vec![widget(4, 2)], &[]);
    assert_eq!(all(&view, "button").len(), 2);
}

#[wasm_bindgen_test]
fn drops_removed_node_decorations() {
    let dec = Decoration::node(1, 6, attrs(&[("class", "cls")]), Value::Null);
    let (view, set) = setup(doc!(blockquote!(p!("foo"), p!("bar"))), vec![dec.clone()]);
    update(&view, &set, vec![], &[dec]);
    assert!(all(&view, ".cls").is_empty());
}

#[wasm_bindgen_test]
fn updates_a_nodes_attributes_without_replacing_it() {
    let dec = Decoration::node(0, 5, attrs(&[("title", "title"), ("class", "foo")]), Value::Null);
    let (view, set) = setup(doc!(p!("foo")), vec![dec.clone()]);
    let paragraph = one(&view, "p");
    update(&view, &set, vec![Decoration::node(0, 5, attrs(&[("class", "foo bar")]), Value::Null)], &[dec]);
    assert!(paragraph.is_same_node(Some(&one(&view, "p"))));
    assert_eq!(paragraph.class_name(), "foo bar");
    assert!(!paragraph.has_attribute("title"));
}

#[wasm_bindgen_test]
fn adds_and_removes_css_custom_properties() {
    let dec = Decoration::node(0, 5, attrs(&[("style", "--my-custom-property:36px")]), Value::Null);
    let (view, set) = setup(doc!(p!("foo")), vec![dec.clone()]);
    assert_eq!(style(&one(&view, "p"), "--my-custom-property"), "36px");
    update(&view, &set, vec![], &[dec]);
    assert_eq!(style(&one(&view, "p"), "--my-custom-property"), "");
}

#[wasm_bindgen_test]
fn updates_decorated_nodes_when_a_widget_is_added_before_them() {
    let (view, set) = setup(doc!(p!("a"), p!("b")), vec![]);
    let paragraph = all(&view, "p").pop().unwrap();
    update(&view, &set, vec![widget(3, 1), Decoration::node(3, 6, attrs(&[("style", "color: red")]), Value::Null)], &[]);
    assert_eq!(style(&paragraph, "color"), "red");
}

#[wasm_bindgen_test]
fn preserves_nodes_when_a_widget_before_them_is_replaced() {
    let first = widget(3, 1);
    let (view, set) = setup(doc!(h1!("a"), p!("b")), vec![first.clone()]);
    let paragraph = one(&view, "p");
    edit(&view, &set, vec![widget(3, 2)], &[first], |tr| { tr.insert_text("c", 5, 5); });
    assert!(paragraph.is_same_node(Some(&one(&view, "p"))));
}

#[wasm_bindgen_test]
fn adds_and_removes_inline_style() {
    let dec = Decoration::inline(1, 6, attrs(&[("style", "color: rgba(0,10,200,.4); text-decoration: underline")]), Value::Null);
    let image = test_schema().node("image", Some(json!({"src":"img.png"})), None, None).unwrap();
    let (view, set) = setup(doc!(p!("al", image, "lo")), vec![dec.clone()]);
    let img = one(&view, "img");
    assert!(style(&img, "color").contains("rgba"));
    assert_eq!(style(&img.previous_sibling().unwrap().unchecked_into(), "text-decoration"), "underline");
    update(&view, &set, vec![], &[dec]);
    let img = one(&view, "img");
    assert_eq!(style(&img, "color"), "");
    assert_eq!(style(&img, "text-decoration"), "");
}

#[wasm_bindgen_test]
fn passes_decorations_to_a_node_view() {
    let current = Rc::new(RefCell::new(String::new()));
    let latest = current.clone();
    let mut props = BrowserViewProps::default();
    props.node_views.insert("horizontal_rule".into(), Rc::new(move |doc, _, _, _, _| {
        let latest = latest.clone();
        let mut view = NodeView::new(doc.create_element("hr")?);
        view.update = Some(Rc::new(move |_, decos, _| {
            latest.replace(decos.iter().filter_map(|d| d.spec()["name"].as_str()).collect::<Vec<_>>().join(","));
            false
        }));
        Ok(view)
    }));
    let (view, set) = setup_with(doc!(p!("foo"), hr()), vec![], props);
    let named = |name| Decoration::node(5, 6, attrs(&[]), json!({"name":name}));
    let first = named("a");
    update(&view, &set, vec![first.clone()], &[]);
    assert_eq!(*current.borrow(), "a");
    update(&view, &set, vec![named("b"), named("c")], &[first]);
    assert_eq!(*current.borrow(), "b,c");
}

#[wasm_bindgen_test]
fn draws_specified_marks_around_a_widget() {
    let (view, _) = setup_with(doc!(p!("foobar")), vec![
        Decoration::widget(4, 1, json!({"marks":[{"type":"em"}]})),
    ], BrowserViewProps { widgets: [(1, widget_element("img", None))].into(), ..Default::default() });
    assert_eq!(all(&view, "em img").len(), 1);
}

#[wasm_bindgen_test]
fn draws_widgets_inside_the_marks_for_their_side() {
    let (view, _) = setup_with(doc!(p!(em("foo"), strong("bar"))), vec![
        Decoration::widget(4, 1, json!({"side":-1})),
        widget(4, 2),
        Decoration::widget(7, 3, json!({"side":1})),
    ], BrowserViewProps {
        widgets: [(1, widget_element("img", None)), (2, widget_element("br", None)), (3, widget_element("span", None))].into(),
        ..Default::default()
    });
    assert_eq!(all(&view, "em img").len(), 1);
    assert!(all(&view, "strong img").is_empty());
    assert_eq!(all(&view, "strong br").len(), 1);
    assert!(all(&view, "em br").is_empty());
    assert!(all(&view, "strong span").is_empty());
}

#[wasm_bindgen_test]
fn draws_decorations_inside_node_views() {
    let mut props = BrowserViewProps { widgets: [(1, widget_element("img", None))].into(), ..Default::default() };
    props.node_views.insert("paragraph".into(), Rc::new(|doc, _, _, _, _| {
        let p = doc.create_element("p")?;
        let mut view = NodeView::new(p.clone());
        view.content_dom = Some(p.into());
        Ok(view)
    }));
    let (view, _) = setup_with(doc!(p!("foo")), vec![widget(2, 1)], props);
    assert_eq!(all(&view, "img").len(), 1);
}

#[wasm_bindgen_test]
fn delays_widget_drawing_until_render() {
    let actual_state = Rc::new(RefCell::new(None));
    let expected = actual_state.clone();
    let factory: WidgetViewFactory = Rc::new(move |doc, state, _, _| {
        assert!(state.doc.ptr_eq(expected.borrow().as_ref().unwrap()));
        let span = doc.create_element("span")?;
        span.set_text_content(Some("!"));
        Ok(WidgetView::new(span))
    });
    let view = mount(doc!(p!("hi")), BrowserViewProps {
        decorations: vec![Rc::new(move |state| {
            actual_state.replace(Some(state.doc.clone()));
            Some(DecorationSet::create(&state.doc, vec![widget(3, 1)]))
        })],
        widgets: [(1, factory)].into(), ..Default::default()
    });
    assert_eq!(view.dom().text_content().unwrap(), "hi!");
}

#[wasm_bindgen_test]
fn supports_widgets_querying_their_own_position() {
    let get = Rc::new(RefCell::new(None::<GetPos>));
    let captured = get.clone();
    let factory: WidgetViewFactory = Rc::new(move |doc, _, _, position| {
        assert_eq!(position(), Some(3));
        captured.replace(Some(position));
        Ok(WidgetView::new(doc.create_element("button")?))
    });
    let (_view, _) = setup_with(doc!(p!("hi")), vec![widget(3, 1)], BrowserViewProps {
        widgets: [(1, factory)].into(), ..Default::default()
    });
    assert_eq!((get.borrow().as_ref().unwrap())(), Some(3));
}

#[wasm_bindgen_test]
fn preserves_widgets_with_matching_keys() {
    let id = Rc::new(Cell::new(1));
    let next_id = id.clone();
    let view = mount(doc!(p!("hi")), BrowserViewProps {
        decorations: vec![Rc::new(move |state| {
            let previous = next_id.replace(2);
            Some(DecorationSet::create(&state.doc, vec![Decoration::widget(2, previous, json!({"key":"myButton"}))]))
        })],
        widgets: [(1, widget_element("button", None)), (2, widget_element("button", None))].into(),
        ..Default::default()
    });
    let button = one(&view, "button");
    let mut tr = view.state().tr();
    tr.insert_text("!", 2, 2);
    view.dispatch(tr).unwrap();
    assert!(button.is_same_node(Some(&one(&view, "button"))));
}

#[wasm_bindgen_test]
fn preserves_widgets_with_identical_specs() {
    let view = mount(doc!(p!("hi")), BrowserViewProps {
        decorations: vec![Rc::new(|state| {
            Some(DecorationSet::create(&state.doc, vec![Decoration::widget(2, 1, json!({"side":1}))]))
        })],
        widgets: [(1, widget_element("button", None))].into(), ..Default::default()
    });
    let button = one(&view, "button");
    let mut tr = view.state().tr();
    tr.insert_text("!", 2, 2);
    view.dispatch(tr).unwrap();
    assert!(button.is_same_node(Some(&one(&view, "button"))));
}

#[wasm_bindgen_test]
fn handles_split_text_nodes() {
    let view = mount(doc!(p!("abab")), BrowserViewProps {
        decorations: vec![Rc::new(|state| (state.selection.from() > 1).then(|| {
            DecorationSet::create(&state.doc, vec![inline(1, 2, "foo"), inline(3, 4, "foo")])
        }))],
        ..Default::default()
    });
    let mut tr = view.state().tr();
    tr.set_selection(Selection::text(5, 5));
    view.dispatch(tr).unwrap();
    assert_eq!(view.dom().text_content().unwrap(), "abab");
}

fn thing_schema() -> Schema {
    Schema::new(vec![
        ("doc".into(), NodeSpec { content: Some("(text | thing)*".into()), ..Default::default() }),
        ("text".into(), NodeSpec::default()),
        ("thing".into(), NodeSpec {
            content: Some("text*".into()),
            inline: true,
            to_dom: Some(|_| DomOutputSpec::element("strong", vec![DomOutputSpec::Hole])),
            ..Default::default()
        }),
    ], vec![])
}

fn thing_child(schema: &Schema, text: &str) -> prosemirror_model::Node {
    schema.node("thing", None, Some(Fragment::from_nodes(vec![schema.text(text, None)])), None).unwrap()
}

#[wasm_bindgen_test]
fn draws_inline_decorations_only_on_the_innermost_level() {
    let schema = thing_schema();
    let content = Fragment::from_nodes(vec![schema.text("abc", None), thing_child(&schema, "def"), schema.text("ghi", None)]);
    let doc = schema.node("doc", None, Some(content), None).unwrap();
    let (view, _) = setup(TaggedDoc { node: doc, tags: Default::default() }, vec![inline(1, 10, "dec")]);
    let styled = all(&view, ".dec");
    assert_eq!(contents(&styled), ["bc", "def", "gh"]);
    assert_eq!(styled[1].parent_node().unwrap().node_name(), "STRONG");
}

#[wasm_bindgen_test]
fn handles_inline_decorations_ending_on_inline_node_boundaries() {
    let schema = thing_schema();
    let content = Fragment::from_nodes(vec![thing_child(&schema, "abc"), schema.text("def", None)]);
    let doc = schema.node("doc", None, Some(content), None).unwrap();
    let (view, _) = setup(TaggedDoc { node: doc, tags: Default::default() }, vec![inline(3, 5, "c")]);
    assert_eq!(all(&view, ".c").len(), 1);
}

#[wasm_bindgen_test]
fn handles_node_name_decorations_overlapping_classes() {
    let (view, _) = setup(doc!(p!("one two three")), vec![
        inline(2, 13, "foo"), Decoration::inline(5, 8, attrs(&[("nodeName", "em")]), Value::Null),
    ]);
    assert_eq!(one(&view, "p").inner_html(),
        r#"o<span class="foo">ne </span><em class="foo">two</em><span class="foo"> thre</span>e"#);
}

#[wasm_bindgen_test]
fn combines_decorations_from_parent_editors_in_child_editors() {
    let received = Rc::new(RefCell::new(DecorationGroup::default()));
    let inner = received.clone();
    let mut parent_props = BrowserViewProps {
        decorations: vec![
            Rc::new(|state| Some(DecorationSet::create(&state.doc, vec![inline(2, 13, "foo")]))),
            Rc::new(|state| Some(DecorationSet::create(&state.doc, vec![inline(2, 13, "bar")]))),
        ],
        ..Default::default()
    };
    parent_props.node_views.insert("paragraph".into(), Rc::new(move |doc, _, _, _, decorations| {
        inner.replace(decorations.clone());
        Ok(NodeView::new(doc.create_element("p")?))
    }));
    let _parent = mount(doc!(p!("one two three")), parent_props);
    let (view, _) = setup_with(doc!(p!("one two three")), vec![inline(1, 12, "baz")], BrowserViewProps {
        decoration_groups: vec![Rc::new(move |_| received.borrow().clone())], ..Default::default()
    });
    assert_eq!(all(&view, ".foo").len(), 1);
    assert_eq!(all(&view, ".bar").len(), 1);
    assert_eq!(all(&view, ".baz").len(), 1);
}
