//! Plain text, HTML, and versioned native slice clipboard data.

use std::io;

use html5ever::{
    parse_fragment, serialize,
    serialize::{Serialize, SerializeOpts, Serializer, TraversalScope},
    tendril::TendrilSink, LocalName, Namespace, QualName, ns,
};
use prosemirror_model::{DomAttribute, DomElement, DomNode, Fragment, ModelError, Node, ResolvedPos, Schema, Slice};
use prosemirror_view::{ClipboardOptions, parse_from_clipboard, serialize_for_clipboard};
use scraper::{ElementRef, Html, HtmlTreeSink, Node as HtmlNode};
use serde_json::{json, Value};

use crate::ClipboardContent;

const FORMAT: &str = "prosemirror-native-slice";
const VERSION: u64 = 1;

pub fn serialize_clipboard(slice: &Slice) -> Result<ClipboardContent, ModelError> {
    serialize_with_schema(slice, slice.content.first_child().and_then(Node::schema))
}

pub(crate) fn serialize_with_schema(slice: &Slice, schema: Option<&Schema>) -> Result<ClipboardContent, ModelError> {
    let plain_text = slice.content.text_between(0, slice.content.size(), Some("\n\n"), None);
    let internal = serde_json::to_vec(&json!({
        "format": FORMAT,
        "version": VERSION,
        "slice": slice,
    })).map_err(|error| ModelError::Serialization(error.to_string()))?;
    // Schemas can omit DOM serialization for their custom types. They can
    // still copy in the native format and provide a plain text alternative.
    let html = schema.and_then(|schema| {
        let data = serialize_for_clipboard(schema, slice.clone(), &ClipboardOptions::default()).ok()?;
        html_fragment(data.dom.children()).ok()
    });
    Ok(ClipboardContent { plain_text, html, internal: Some(internal) })
}

/// Recover the versioned slice, parse an HTML fragment, or use the plain text
/// representation when pasting as text or into a code block.
pub fn paste_slice(
    schema: &Schema, data: &ClipboardContent, plain_text: bool, context: &ResolvedPos,
) -> Result<Option<Slice>, ModelError> {
    let in_code = context.parent().node_type().is_some_and(|kind| kind.spec.code);
    if !plain_text && !in_code {
        if let Some(slice) = native_slice(schema, data.internal.as_deref()) {
            return Ok(Some(slice));
        }
        if let Some(html) = data.html.as_deref().filter(|html| !html.is_empty()) {
            return parse_from_clipboard(
                schema, &data.plain_text, Some(parse_html(html)), false, context, &ClipboardOptions::default(),
            );
        }
    }
    if data.plain_text.is_empty() {
        return Ok(None);
    }
    let text = data.plain_text.replace("\r\n", "\n").replace('\r', "\n");
    if in_code {
        return Ok(Some(Slice::new(Fragment::from_nodes(vec![schema.try_text(&text, None)?]), 0, 0)));
    }

    let kind = (0..=context.depth).rev().find_map(|depth| {
        let parent = context.node(depth);
        let matched = parent.content_match_at(context.index_after(depth))?;
        let default = schema.node_type(matched.default_type()?)?;
        (default.is_textblock() && !default.has_required_attrs()).then_some(default)
    }).or_else(|| {
        schema.node_type("paragraph").filter(|kind| kind.is_textblock())
    }).or_else(|| {
        schema.node_types().find(|kind| kind.is_textblock() && !kind.has_required_attrs())
    });

    let marks = context.marks();
    let Some(kind) = kind else {
        let text = text.replace('\n', " ");
        return Ok(Some(Slice::new(Fragment::from_nodes(vec![schema.try_text(&text, Some(marks))?]), 0, 0)));
    };
    let mut blocks = Vec::new();
    let mut start = 0;
    let mut newline_run = false;
    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            if !newline_run {
                blocks.push(textblock(schema, kind, &text[start..index], &marks)?);
            }
            start = index + 1;
        }
        newline_run = ch == '\n';
    }
    blocks.push(textblock(schema, kind, &text[start..], &marks)?);
    Ok(Some(Slice::max_open(Fragment::from_nodes(blocks), true)))
}

fn textblock(
    schema: &Schema, kind: &prosemirror_model::NodeType, text: &str, marks: &[prosemirror_model::Mark],
) -> Result<prosemirror_model::Node, ModelError> {
    let content = if text.is_empty() {
        Fragment::empty()
    } else {
        let marks = kind.allowed_marks(marks);
        Fragment::from_nodes(vec![schema.try_text(text, Some(marks))?])
    };
    kind.create_and_fill(None, Some(content), None)?
        .ok_or_else(|| ModelError::InvalidContent(kind.name.clone()))
}

fn native_slice(schema: &Schema, data: Option<&[u8]>) -> Option<Slice> {
    let data = data?;
    // A system clipboard can pad allocated byte buffers with NULs.
    let end = data.iter().rposition(|&byte| byte != 0)? + 1;
    let encoded: Value = serde_json::from_slice(&data[..end]).ok()?;
    if encoded["format"].as_str() != Some(FORMAT) || encoded["version"].as_u64() != Some(VERSION) {
        return None;
    }
    let slice = schema.slice_from_json(encoded.get("slice")?).ok()?;
    let allowed = Slice::max_open(slice.content.clone(), true);
    (slice.open_start <= allowed.open_start && slice.open_end <= allowed.open_end).then_some(slice)
}

struct HtmlFragment<'a>(&'a [DomNode]);

impl Serialize for HtmlFragment<'_> {
    fn serialize<S: Serializer>(&self, serializer: &mut S, _: TraversalScope) -> io::Result<()> {
        for node in self.0 {
            write_html(serializer, node)?;
        }
        Ok(())
    }
}

fn html_fragment(nodes: &[DomNode]) -> Result<String, ModelError> {
    let mut bytes = Vec::new();
    serialize(&mut bytes, &HtmlFragment(nodes), SerializeOpts::default())
        .map_err(|error| ModelError::Serialization(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| ModelError::Serialization(error.to_string()))
}

fn write_html(serializer: &mut impl Serializer, node: &DomNode) -> io::Result<()> {
    match node {
        DomNode::Text(text) => serializer.write_text(text),
        DomNode::Comment(text) => serializer.write_comment(text),
        DomNode::Fragment(children) => HtmlFragment(children).serialize(serializer, TraversalScope::IncludeNode),
        DomNode::Element(element) => {
            let namespace = element.namespace.as_deref().map_or(ns!(html), Namespace::from);
            let name = QualName::new(None, namespace, LocalName::from(element.name.as_str()));
            let attrs: Vec<_> = element.attributes.iter().map(|attr| {
                let namespace = attr.namespace.as_deref().map_or(ns!(), Namespace::from);
                (QualName::new(None, namespace, LocalName::from(attr.name.as_str())), attr.value.as_str())
            }).collect();
            serializer.start_elem(name.clone(), attrs.iter().map(|(name, value)| (name, *value)))?;
            for child in &element.children {
                write_html(serializer, child)?;
            }
            serializer.end_elem(name)
        }
    }
}

fn parse_html(html: &str) -> DomNode {
    let context = match first_tag(html).as_deref() {
        Some("thead" | "tbody" | "tfoot" | "caption" | "colgroup") => "table",
        Some("tr") => "tbody",
        Some("td" | "th") => "tr",
        Some("col") => "colgroup",
        _ => "body",
    };
    let parsed = parse_fragment(
        HtmlTreeSink::new(Html::new_fragment()),
        Default::default(),
        QualName::new(None, ns!(html), LocalName::from(context)),
        Vec::new(),
        false,
    ).one(html);
    DomNode::Fragment(dom_children(parsed.root_element()))
}

// A fragment copied out of a table may start with a row or cell. Choose the
// same HTML container ProseMirror uses so the tokenizer keeps these elements.
fn first_tag(html: &str) -> Option<String> {
    let mut rest = html;
    while let Some((_, tail)) = rest.split_once('<') {
        if let Some(comment) = tail.strip_prefix("!--") {
            rest = comment.split_once("-->")?.1;
            continue;
        }
        let name = tail.split([' ', '\r', '\n', '\t', '\u{c}', '/', '>']).next()?;
        if name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) {
            if !name.eq_ignore_ascii_case("meta") {
                return Some(name.to_ascii_lowercase());
            }
            rest = tail.split_once('>')?.1;
        } else {
            rest = tail;
        }
    }
    None
}

fn dom_children(parent: ElementRef<'_>) -> Vec<DomNode> {
    parent.children().filter_map(|child| match child.value() {
        HtmlNode::Text(text) => Some(DomNode::Text(text.text.to_string())),
        HtmlNode::Comment(comment) => Some(DomNode::Comment(comment.comment.to_string())),
        HtmlNode::Element(element) => {
            let children = dom_children(ElementRef::wrap(child)?);
            if element.name() == "span"
                && element.attr("class").is_some_and(|classes| classes.split_ascii_whitespace().any(|class| class == "Apple-converted-space"))
                && matches!(children.as_slice(), [DomNode::Text(text)] if text == "\u{a0}")
            {
                return Some(DomNode::text(" "));
            }
            let attributes = element.attrs.iter().map(|(name, value)| DomAttribute {
                name: name.local.to_string(),
                value: value.to_string(),
                namespace: (!name.ns.is_empty()).then(|| name.ns.to_string()),
            }).collect();
            Some(DomNode::Element(DomElement {
                name: element.name().into(),
                namespace: Some(element.name.ns.to_string()),
                attributes,
                children,
            }))
        }
        _ => None,
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use prosemirror_test_builder::{blockquote, doc, p, test_schema};

    #[test]
    fn preserves_an_open_slice_and_uses_plain_text_as_a_fallback() {
        let doc = doc!(blockquote!(p!("a"), p!("b")), p!("c")).node;
        let context = doc.resolve(3).unwrap();
        let original = doc.slice(2, 6, true).unwrap();
        let data = serialize_clipboard(&original).unwrap();
        assert_eq!(paste_slice(test_schema(), &data, false, &context).unwrap(), Some(original.clone()));
        let html_only = ClipboardContent { html: data.html.clone(), ..ClipboardContent::default() };
        assert_eq!(paste_slice(test_schema(), &html_only, false, &context).unwrap(), Some(original));
        assert_eq!(data.plain_text, "a\n\nb");
        let plain = paste_slice(test_schema(), &data, true, &context).unwrap().unwrap();
        assert_eq!(plain.open_start, 1);
        assert_eq!(plain.content.child_count(), 2);
    }

    #[test]
    fn carries_deep_context_and_encodes_text_in_html() {
        let source = doc!(blockquote!(blockquote!(p!("<a>A&B 🍃<b>"))));
        let original = source.node.slice(source.get_tag("a").unwrap(), source.get_tag("b").unwrap(), true).unwrap();
        assert_eq!(original.open_start, 3);
        let mut clipboard = serialize_clipboard(&original).unwrap();
        let html = clipboard.html.as_deref().unwrap();
        assert!(html.starts_with("<p data-pm-slice=\"1 1 ["));
        assert!(html.contains("A&amp;B 🍃"));
        let context = source.node.resolve(3).unwrap();
        clipboard.internal = None;
        assert_eq!(paste_slice(test_schema(), &clipboard, false, &context).unwrap(), Some(original));
    }

    #[test]
    fn parses_external_html_and_keeps_plain_and_code_paste_textual() {
        let schema = test_schema();
        let source = schema.node_from_json(&json!({
            "type": "doc",
            "content": [{"type": "paragraph"}, {"type": "code_block"}]
        })).unwrap();
        let context = source.resolve(1).unwrap();
        let clipboard = ClipboardContent {
            plain_text: "line 1\r\nline 2".into(),
            html: Some("<style>.accent{font-weight:bold}</style><p>A&#x1f343; &amp; <span class=accent>é</span></p>".into()),
            internal: Some(b"{\"format\":\"prosemirror-native-slice\",\"version\":99}".to_vec()),
        };
        let rich = paste_slice(schema, &clipboard, false, &context).unwrap().unwrap();
        let paragraph = rich.content.first_child().unwrap();
        assert_eq!(paragraph.text_content(), "A🍃 & é");
        assert_eq!(paragraph.last_child().unwrap().marks()[0].type_name, "strong");

        let plain = paste_slice(schema, &clipboard, true, &context).unwrap().unwrap();
        assert_eq!(plain.content.text_between(0, plain.content.size(), Some("\n"), None), "line 1\nline 2");
        let code = paste_slice(schema, &clipboard, false, &source.resolve(3).unwrap()).unwrap().unwrap();
        assert_eq!(code.open_start, 0);
        let text = code.content.first_child().unwrap();
        assert_eq!(text.text(), Some("line 1\nline 2"));
        assert!(text.marks().is_empty());
    }

    #[test]
    fn retains_table_rows_and_cells_without_an_explicit_table() {
        let html = parse_html("<!--StartFragment--><TR><td>A</td><td>B</td></TR><!--EndFragment-->");
        let row = html.children().iter().find_map(DomNode::as_element).unwrap();
        assert_eq!(row.name, "tr");
        assert_eq!(row.children.len(), 2);
        assert_eq!(row.children[0].as_element().unwrap().name, "td");
        let html = parse_html("<th>A</th><td>B</td>");
        let cells: Vec<_> = html.children().iter().filter_map(DomNode::as_element).map(|cell| cell.name.as_str()).collect();
        assert_eq!(cells, ["th", "td"]);
    }

    #[test]
    fn reads_native_system_buffers_with_trailing_padding() {
        let document = doc!(p!("clipboard")).node;
        let original = document.slice(1, 10, true).unwrap();
        let mut clipboard = serialize_clipboard(&original).unwrap();
        clipboard.internal.as_mut().unwrap().extend([0, 0, 0]);
        assert_eq!(native_slice(test_schema(), clipboard.internal.as_deref()), Some(original));
    }
}
