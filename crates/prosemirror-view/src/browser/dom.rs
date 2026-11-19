//! Model DOM parsing with the browser's CSS selector semantics.

use prosemirror_model::{DomElement, DomNode, DomParseOptions, DomParser, DomSelectorResolver, ModelError, Node, Slice};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, Node as WebNode};

use super::{domchange::{browser_dom, dom_index}, render_dom};

pub fn parse(
    parser: &DomParser, node: &WebNode, options: DomParseOptions<'_>,
) -> Result<Node, ModelError> {
    let selectors = BrowserSelectors(node);
    parser.parse(&browser_dom(node), DomParseOptions { selectors: Some(&selectors), ..options })
}

pub fn parse_slice(
    parser: &DomParser, node: &WebNode, options: DomParseOptions<'_>,
) -> Result<Slice, ModelError> {
    let selectors = BrowserSelectors(node);
    parser.parse_slice(&browser_dom(node), DomParseOptions { selectors: Some(&selectors), ..options })
}

pub(super) fn parse_clipboard_slice(
    document: &Document, parser: &DomParser, dom: &DomNode, options: DomParseOptions<'_>,
) -> Result<Slice, ModelError> {
    // Match selectors on the normalized clipboard DOM in a document with no
    // browsing context. Clipboard nodes are never attached to the live page.
    let detached = document.implementation().map_err(dom_error)?
        .create_html_document().map_err(dom_error)?;
    let root = render_dom(&detached, dom).map_err(dom_error)?;
    let selectors = BrowserSelectors(&root);
    parser.parse_slice(dom, DomParseOptions { selectors: Some(&selectors), ..options })
}

pub(super) struct BrowserSelectors<'a>(pub(super) &'a WebNode);

impl BrowserSelectors<'_> {
    fn element_at(&self, portable: &DomElement, path: &[usize]) -> Option<Element> {
        let mut node = self.0.clone();
        for &index in path {
            node = node.child_nodes().item(u32::try_from(index).ok()?)?;
        }
        let element = node.dyn_into::<Element>().ok()?;
        (element.local_name() == portable.name).then_some(element)
    }
}

impl DomSelectorResolver for BrowserSelectors<'_> {
    fn matches(&self, element: &DomElement, path: &[usize], selector: &str) -> Option<bool> {
        // Descriptor rules are attached to the portable representation.
        if selector.starts_with("[data-pm-rule=") {
            return None;
        }
        self.element_at(element, path)?.matches(selector).ok()
    }

    fn content_path(&self, element: &DomElement, path: &[usize], selector: &str) -> Option<Vec<usize>> {
        let element = self.element_at(element, path)?;
        let mut node: WebNode = element.query_selector(selector).ok()??.into();
        let mut indices = Vec::new();
        while !node.is_same_node(Some(&element)) {
            indices.push(dom_index(&node) as usize);
            node = node.parent_node()?;
        }
        indices.reverse();
        Some(indices)
    }
}

fn dom_error(error: JsValue) -> ModelError {
    ModelError::Dom(error.as_string().unwrap_or_else(|| format!("{error:?}")))
}
