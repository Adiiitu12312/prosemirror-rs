//! Browser HTML and ClipboardEvent adapters for the Rust clipboard algorithms.

use prosemirror_model::{DomNode, DomParseOptions, DomParser, ResolvedPos, Slice};
use serde_json::json;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{ClipboardEvent, DataTransfer, Document, HtmlElement, HtmlTemplateElement};

use super::{domchange::browser_dom, error, viewdesc::render_dom, BrowserView};
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
        let slice = config.providers().filter_map(|props| props.transform_copied.as_ref())
            .fold(slice, |slice, transform| transform(slice, self));
        let text = |slice: &Slice| config.some_prop(|props| props.clipboard_text_serializer.as_ref()
            .map(|serialize| serialize(slice, self)).filter(|text| !text.is_empty()))
            .unwrap_or_default();
        let data = serialize_for_clipboard(&config.state.schema, slice, &ClipboardOptions {
            serializer: config.some_prop(|props| props.clipboard_serializer.as_deref()),
            text_serializer: Some(&text),
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
        let document = self.dom().owner_document().ok_or_else(|| error("No owner document"))?;
        let html = html.filter(|html| !html.is_empty());
        let as_text = !text.is_empty()
            && (plain || html.is_none() || context.parent().node_type().is_some_and(|kind| kind.spec.code));
        let dom = if let Some(html) = html.filter(|_| !as_text) {
            let html = config.providers().filter_map(|props| props.transform_pasted_html.as_ref())
                .fold(html.to_owned(), |html, transform| transform(&html, self));
            Some(parse_html(&document, &html)?)
        } else {
            None
        };
        let transform_text = |text: &str, plain| config.providers().filter_map(|props| props.transform_pasted_text.as_ref())
            .fold(text.to_owned(), |text, transform| transform(&text, plain, self));
        let transform_slice = |slice, plain| config.providers().filter_map(|props| props.transform_pasted.as_ref())
            .fold(slice, |slice, transform| transform(slice, self, plain));
        let parse_text = |text: &str, context: &ResolvedPos, plain| {
            config.some_prop(|props| props.clipboard_text_parser.as_ref()
                .and_then(|parse| parse(text, context, plain, self)))
        };
        let parse_slice = |parser: &DomParser, dom: &DomNode, options: DomParseOptions<'_>| {
            super::dom::parse_clipboard_slice(&document, parser, dom, options)
        };
        parse_from_clipboard(&config.state.schema, text, dom, plain, context, &ClipboardOptions {
            parser: config.some_prop(|props| props.clipboard_parser.as_deref())
                .or_else(|| config.some_prop(|props| props.dom_parser.as_deref())),
            transform_pasted_text: Some(&transform_text),
            transform_pasted: Some(&transform_slice),
            text_parser: Some(&parse_text),
            parse_slice: Some(&parse_slice),
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
        if config.providers().filter_map(|props| props.handle_paste.as_ref())
            .any(|handler| handler(self, event, slice.as_ref().unwrap_or(&empty)))
        {
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
    Ok(browser_dom(&template.content()))
}
