use std::ops::Deref;

use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_test_builder::{test_schema, TaggedDoc};
use prosemirror_view::browser::{BrowserView, BrowserViewProps, Mount, CSS};
use web_sys::{Document, Element};

pub fn document() -> Document {
    web_sys::window().unwrap().document().unwrap()
}

pub fn state(fixture: TaggedDoc) -> EditorState {
    let selection = fixture.get_tag("a").map(|anchor| {
        if fixture.node.resolve(anchor).unwrap().parent().inline_content() {
            Selection::text(anchor, fixture.get_tag("b").unwrap_or(anchor))
        } else {
            Selection::node(anchor)
        }
    });
    EditorState::create(EditorStateConfig {
        schema: fixture.node.schema().unwrap_or(test_schema()).clone(),
        doc: Some(fixture.node),
        selection,
        stored_marks: None,
        plugins: Vec::new(),
    })
}

pub fn mount(fixture: TaggedDoc, props: BrowserViewProps) -> TestView {
    mount_state(state(fixture), props)
}

pub fn mount_state(state: EditorState, props: BrowserViewProps) -> TestView {
    let document = document();
    let space = document.create_element("div").unwrap();
    space.set_id("workspace");
    let style = document.create_element("style").unwrap();
    style.set_text_content(Some(CSS));
    space.append_child(&style).unwrap();
    document.body().unwrap().append_child(&space).unwrap();
    let view = BrowserView::new(Mount::Append(space.clone()), state, props).unwrap();
    TestView { view, space }
}

pub struct TestView {
    view: BrowserView,
    space: Element,
}

impl Deref for TestView {
    type Target = BrowserView;

    fn deref(&self) -> &Self::Target {
        &self.view
    }
}

impl Drop for TestView {
    fn drop(&mut self) {
        self.view.destroy();
        self.space.remove();
    }
}
