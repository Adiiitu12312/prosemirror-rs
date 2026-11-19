//! Browser HTML and ClipboardEvent adapters for the Rust clipboard algorithms.

use prosemirror_model::{DomAttribute, DomElement, DomNode, ResolvedPos, Slice};
use serde_json::json;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{ClipboardEvent, DataTransfer, Document, Element, HtmlElement, HtmlTemplateElement, Node};

use super::{error, viewdesc::render_dom, BrowserView};
use crate::{parse_from_clipboard, serialize_for_clipboard, ClipboardOptions};

pub struct BrowserClipboardData {
    pub dom: HtmlElement,
    pub text: String,
    pub slice: Slice,
}

impl BrowserView {
    pub fn paste_html(&self, html: &str, event: Option<&ClipboardEvent>) -> Result<bool, JsValue> {
        self.paste_content("", Some(html), false, event)
    }

    pub fn paste_text(&self, text: &str, event: Option<&ClipboardEvent>) -> Result<bool, JsValue> {
        self.paste_content(text, None, true, event)
    }

    pub fn serialize_for_clipboard(&self, slice: Slice) -> Result<BrowserClipboardData, JsValue> {
        let config = self.props();
        let transform = |slice| (config.transform_copied.as_ref().unwrap())(slice, self);
        let text = |slice: &Slice| (config.clipboard_text_serializer.as_ref().unwrap())(slice, self);
        let data = serialize_for_clipboard(&config.state.schema, slice, &ClipboardOptions {
            serializer: config.clipboard_serializer.as_deref(),
            transform_copied: config.transform_copied.as_ref().map(|_| &transform as &dyn Fn(Slice) -> Slice),
            text_serializer: config.clipboard_text_serializer.as_ref().map(|_| &text as &dyn Fn(&Slice) -> String),
            ..ClipboardOptions::default()
        }).map_err(error)?;
        let document = self.dom().owner_document().ok_or_else(|| error("No owner document"))?;
        let dom = render_dom(&document, &data.dom)?.dyn_into()?;
        Ok(BrowserClipboardData { dom, text: data.text, slice: data.slice })
    }

    pub(super) fn parse_clipboard(
        &self, text: &str, html: Option<&str>, plain: bool, context: &ResolvedPos,
    ) -> Result<Option<Slice>, JsValue> {
        let config = self.props();
        let html = html.filter(|html| !html.is_empty());
        let as_text = !text.is_empty()
            && (plain || html.is_none() || context.parent().node_type().is_some_and(|kind| kind.spec.code));
        let dom = if let Some(html) = html.filter(|_| !as_text) {
            let transformed = config.transform_pasted_html.as_ref().map(|transform| transform(html, self));
            let document = self.dom().owner_document().ok_or_else(|| error("No owner document"))?;
            Some(parse_html(&document, transformed.as_deref().unwrap_or(html))?)
        } else {
            None
        };
        let transform_text = |text: &str, plain| (config.transform_pasted_text.as_ref().unwrap())(text, plain, self);
        let transform_slice = |slice, plain| (config.transform_pasted.as_ref().unwrap())(slice, self, plain);
        let parse_text = |text: &str, context: &ResolvedPos, plain| {
            (config.clipboard_text_parser.as_ref().unwrap())(text, context, plain, self)
        };
        parse_from_clipboard(&config.state.schema, text, dom, plain, context, &ClipboardOptions {
            parser: config.clipboard_parser.as_deref().or(config.dom_parser.as_deref()),
            transform_pasted_text: config.transform_pasted_text.as_ref()
                .map(|_| &transform_text as &dyn Fn(&str, bool) -> String),
            transform_pasted: config.transform_pasted.as_ref().map(|_| &transform_slice as &dyn Fn(Slice, bool) -> Slice),
            text_parser: config.clipboard_text_parser.as_ref()
                .map(|_| &parse_text as &dyn Fn(&str, &ResolvedPos, bool) -> Option<Slice>),
            ..ClipboardOptions::default()
        }).map_err(error)
    }

    pub(super) fn paste_content(
        &self, text: &str, html: Option<&str>, plain: bool, event: Option<&ClipboardEvent>,
    ) -> Result<bool, JsValue> {
        let config = self.props();
        let context = config.state.doc.resolve(config.state.selection.from()).map_err(error)?;
        let slice = self.parse_clipboard(text, html, plain, &context)?;
        let empty = Slice::empty();
        if config.handle_paste.as_ref().is_some_and(|handler| handler(self, event, slice.as_ref().unwrap_or(&empty))) {
            return Ok(true);
        }
        let Some(slice) = slice else { return Ok(false) };
        let mut tr = config.state.tr();
        if let Some(node) = single_node(&slice) {
            tr.replace_selection_with(node.clone(), plain);
        } else {
            tr.replace_selection(slice);
        }
        tr.scroll_into_view().set_meta("paste", json!(true)).set_meta("uiEvent", json!("paste"));
        self.dispatch(tr)?;
        Ok(true)
    }
}

pub(super) fn single_node(slice: &Slice) -> Option<&prosemirror_model::Node> {
    (slice.open_start == 0 && slice.open_end == 0 && slice.content.child_count() == 1)
        .then(|| slice.content.child(0))
}

pub(super) fn clipboard_text(data: &DataTransfer) -> Result<String, JsValue> {
    let mut text = data.get_data("text/plain")?;
    if text.is_empty() {
        text = data.get_data("Text")?;
    }
    if text.is_empty() {
        text = data.get_data("text/uri-list")?.replace("\r\n", " ").replace('\n', " ");
    }
    Ok(text)
}

fn parse_html(document: &Document, html: &str) -> Result<DomNode, JsValue> {
    let template = document.create_element("template")?.dyn_into::<HtmlTemplateElement>()?;
    template.set_inner_html(html);
    Ok(DomNode::Fragment(portable_children(&template.content())))
}

fn portable_children(node: &Node) -> Vec<DomNode> {
    let children = node.child_nodes();
    (0..children.length()).filter_map(|i| children.item(i)).filter_map(|node| portable_node(&node)).collect()
}

fn portable_node(node: &Node) -> Option<DomNode> {
    match node.node_type() {
        Node::TEXT_NODE => Some(DomNode::Text(node.node_value().unwrap_or_default())),
        Node::COMMENT_NODE => Some(DomNode::Comment(node.node_value().unwrap_or_default())),
        Node::ELEMENT_NODE => {
            let element = node.dyn_ref::<Element>()?;
            let attrs = element.attributes();
            Some(DomNode::Element(DomElement {
                name: element.local_name(),
                namespace: element.namespace_uri().filter(|namespace| namespace != "http://www.w3.org/1999/xhtml"),
                attributes: (0..attrs.length()).filter_map(|i| attrs.item(i)).map(|attr| DomAttribute {
                    name: attr.name(), value: attr.value(), namespace: attr.namespace_uri(),
                }).collect(),
                children: portable_children(node),
            }))
        }
        _ => None,
    }
}
