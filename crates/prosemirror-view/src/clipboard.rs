//! Portable clipboard logic from prosemirror-view@1.42.9/src/clipboard.ts.

use prosemirror_model::{
    DomAttribute, DomNode, DomParseOptions, DomParser, DomSerializer, Fragment, ModelError, Node,
    ResolvedPos, Schema, Slice, Whitespace,
};
use serde_json::{json, Value};

type TextSerializer<'a> = &'a dyn Fn(&Slice) -> String;
type TextParser<'a> = &'a dyn Fn(&str, &ResolvedPos, bool) -> Option<Slice>;
type PasteText<'a> = &'a dyn Fn(&str, bool) -> String;
type PasteSlice<'a> = &'a dyn Fn(Slice, bool) -> Slice;

#[derive(Default)]
pub struct ClipboardOptions<'a> {
    pub serializer: Option<&'a DomSerializer>,
    pub parser: Option<&'a DomParser>,
    pub transform_copied: Option<&'a dyn Fn(Slice) -> Slice>,
    pub text_serializer: Option<TextSerializer<'a>>,
    pub text_parser: Option<TextParser<'a>>,
    pub transform_pasted_text: Option<PasteText<'a>>,
    pub transform_pasted_dom: Option<&'a dyn Fn(DomNode) -> DomNode>,
    pub transform_pasted: Option<PasteSlice<'a>>,
}

#[derive(Debug, PartialEq)]
pub struct ClipboardData {
    pub dom: DomNode,
    pub text: String,
    pub slice: Slice,
}

pub fn serialize_for_clipboard(
    schema: &Schema, slice: Slice, options: &ClipboardOptions<'_>,
) -> Result<ClipboardData, ModelError> {
    let slice = match options.transform_copied {
        Some(transform) => transform(slice),
        None => slice,
    };
    let mut context = Vec::new();
    let mut content = &slice.content;
    let (mut start, mut end) = (slice.open_start, slice.open_end);
    while start > 1 && end > 1 && content.child_count() == 1 && content.child(0).child_count() == 1 {
        let node = content.child(0);
        let attrs = node.attrs().cloned().unwrap_or_else(|| json!({}));
        let default = node.node_type().filter(|kind| !kind.has_required_attrs())
            .and_then(|kind| kind.try_create(None, None, None).ok())
            .and_then(|node| node.attrs().cloned());
        context.push(json!(node.type_name()));
        context.push(if Some(&attrs) == default.as_ref() { Value::Null } else { attrs });
        content = node.content();
        start -= 1;
        end -= 1;
    }
    let default_serializer;
    let serializer = match options.serializer {
        Some(serializer) => serializer,
        None => {
            default_serializer = DomSerializer::from_schema(schema);
            &default_serializer
        }
    };
    let mut children = match serializer.serialize_fragment(content)? {
        DomNode::Fragment(children) => children,
        node => vec![node],
    };
    let wrappers = children.first().and_then(DomNode::as_element)
        .map_or(&[][..], |element| clipboard_wrappers(&element.name));
    for &tag in wrappers.iter().rev() {
        children = vec![DomNode::element(tag, children)];
    }
    if let Some(DomNode::Element(first)) = children.first_mut() {
        let count = if wrappers.is_empty() { String::new() } else { format!(" -{}", wrappers.len()) };
        let value = format!("{start} {end}{count} {}", Value::Array(context));
        first.attributes.retain(|attr| attr.name != "data-pm-slice" || attr.namespace.is_some());
        first.attributes.push(DomAttribute::new("data-pm-slice", value));
    }
    let text = options.text_serializer.map(|serialize| serialize(&slice))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| slice.content.text_between(0, slice.content.size(), Some("\n\n"), None));
    Ok(ClipboardData { dom: DomNode::element("div", children), text, slice })
}

/// The host supplies a parsed DOM fragment for HTML clipboard data.
pub fn parse_from_clipboard(
    schema: &Schema, text: &str, dom: Option<DomNode>, plain_text: bool, context: &ResolvedPos,
    options: &ClipboardOptions<'_>,
) -> Result<Option<Slice>, ModelError> {
    if text.is_empty() && dom.is_none() {
        return Ok(None);
    }
    let in_code = context.parent().node_type().is_some_and(|kind| kind.spec.code);
    let as_text = !text.is_empty() && (plain_text || in_code || dom.is_none());
    let slice = if as_text {
        let text = options.transform_pasted_text.map_or_else(
            || text.to_owned(), |transform| transform(text, in_code || plain_text),
        );
        if in_code {
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            let content = if text.is_empty() { Fragment::empty() } else { singleton(schema.try_text(&text, None)?) };
            Slice::new(content, 0, 0)
        } else if let Some(slice) = options.text_parser.and_then(|parse| parse(&text, context, plain_text)) {
            normalize_slice(schema, slice, context)
        } else {
            let serializer = DomSerializer::from_schema(schema);
            let marks = context.marks();
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            let mut blocks = Vec::new();
            let mut previous_newline = false;
            let mut start = 0;
            for (index, ch) in text.char_indices() {
                if ch == '\n' {
                    if !previous_newline {
                        blocks.push(text_block(schema, &serializer, &marks, &text[start..index])?);
                    }
                    start = index + 1;
                }
                previous_newline = ch == '\n';
            }
            blocks.push(text_block(schema, &serializer, &marks, &text[start..])?);
            parse_dom(schema, DomNode::Fragment(blocks), true, context, options)?
        }
    } else {
        let Some(dom) = dom else { return Ok(None) };
        let mut dom = match options.transform_pasted_dom {
            Some(transform) => transform(dom),
            None => dom,
        };
        let mut styles = Vec::new();
        collect_styles(&dom, &mut styles);
        apply_styles(&mut dom, &styles);
        parse_dom(schema, dom, false, context, options)?
    };
    Ok(Some(match options.transform_pasted {
        Some(transform) => transform(slice, as_text),
        None => slice,
    }))
}

fn text_block(schema: &Schema, serializer: &DomSerializer, marks: &[prosemirror_model::Mark], text: &str) -> Result<DomNode, ModelError> {
    let children = if text.is_empty() {
        Vec::new()
    } else {
        vec![serializer.serialize_node(&schema.try_text(text, Some(marks.to_vec()))?)?]
    };
    Ok(DomNode::element("p", children))
}

fn parse_dom(
    schema: &Schema, mut dom: DomNode, as_text: bool, context: &ResolvedPos, options: &ClipboardOptions<'_>,
) -> Result<Slice, ModelError> {
    let data = find_slice_data(&dom).and_then(SliceData::parse);
    strip_trailing_breaks(&mut dom);
    let mut root = &dom;
    if let Some(data) = &data {
        for _ in 0..data.wrappers {
            let Some(child) = root.children().iter().find(|node| matches!(node, DomNode::Element(_))) else { break };
            root = child;
        }
    }
    let default_parser;
    let parser = match options.parser {
        Some(parser) => parser,
        None => {
            default_parser = DomParser::from_schema(schema);
            &default_parser
        }
    };
    let slice = parser.parse_slice(root, DomParseOptions {
        preserve_whitespace: (as_text || data.is_some()).then_some(Whitespace::Preserve),
        context: Some(context),
        ..DomParseOptions::default()
    })?;
    Ok(if let Some(data) = data {
        add_context(schema, close_slice(schema, slice, data.start, data.end), &data.context)
    } else {
        normalize_slice(schema, slice, context)
    })
}

fn singleton(node: Node) -> Fragment {
    Fragment::from_nodes(vec![node])
}

fn clipboard_wrappers(name: &str) -> &'static [&'static str] {
    match name {
        "thead" | "tbody" | "tfoot" | "caption" | "colgroup" => &["table"],
        "col" => &["table", "colgroup"],
        "tr" => &["table", "tbody"],
        "td" | "th" => &["table", "tbody", "tr"],
        _ => &[],
    }
}

fn find_slice_data(dom: &DomNode) -> Option<&str> {
    dom.as_element().and_then(|element| element.attr("data-pm-slice"))
        .or_else(|| dom.children().iter().find_map(find_slice_data))
}

struct SliceData {
    start: usize,
    end: usize,
    wrappers: usize,
    context: Vec<Value>,
}

impl SliceData {
    fn parse(data: &str) -> Option<Self> {
        let (start, rest) = data.split_once(' ')?;
        let (end, mut rest) = rest.split_once(' ')?;
        let mut wrappers = 0;
        if let Some(wrapped) = rest.strip_prefix('-') {
            let (depth, json) = wrapped.split_once(' ')?;
            wrappers = depth.parse().ok()?;
            rest = json;
        }
        Some(Self {
            start: start.parse().ok()?,
            end: end.parse().ok()?,
            wrappers,
            context: serde_json::from_str(rest).unwrap_or_default(),
        })
    }
}

fn add_context(schema: &Schema, mut slice: Slice, context: &[Value]) -> Slice {
    if slice.size() == 0 {
        return slice;
    }
    for pair in context.rchunks_exact(2) {
        let Some(kind) = pair[0].as_str().and_then(|name| schema.node_type(name)) else { break };
        if kind.has_required_attrs() {
            break;
        }
        let attrs = (!pair[1].is_null()).then(|| pair[1].clone());
        let Ok(node) = kind.try_create(attrs, Some(slice.content.clone()), None) else { break };
        slice = Slice::new(singleton(node), slice.open_start + 1, slice.open_end + 1);
    }
    slice
}

fn normalize_slice(schema: &Schema, slice: Slice, context: &ResolvedPos) -> Slice {
    let content = normalize_siblings(schema, &slice.content, context);
    let slice = Slice::max_open(content, true);
    let closed = Slice::max_open(slice.content.clone(), false);
    close_slice(schema, slice, closed.open_start, closed.open_end)
}

fn normalize_siblings(schema: &Schema, fragment: &Fragment, context: &ResolvedPos) -> Fragment {
    if fragment.child_count() < 2 {
        return fragment.clone();
    }
    for depth in (0..=context.depth).rev() {
        let parent = context.node(depth);
        let Some(matched) = parent.content_match_at(context.index(depth)) else { continue };
        let result = (|| {
            let mut matched = matched;
            let mut nodes: Vec<Node> = Vec::new();
            let mut last_wrap: Vec<String> = Vec::new();
            for node in &fragment.children {
                let wrap = matched.find_wrapping(schema, node.type_name())?;
                if let Some(last) = nodes.last_mut() {
                    if let Some(grouped) = add_to_sibling(schema, &wrap, &last_wrap, node, last, 0) {
                        *last = grouped;
                        continue;
                    }
                    *last = close_right(schema, last, last_wrap.len())?;
                }
                let wrapped = wrap_node(schema, node.clone(), &wrap)?;
                matched = matched.match_type(wrapped.type_name())?;
                nodes.push(wrapped);
                last_wrap = wrap;
            }
            Some(Fragment::from_nodes(nodes))
        })();
        if let Some(result) = result {
            return result;
        }
    }
    fragment.clone()
}

fn wrap_node(schema: &Schema, mut node: Node, wrappers: &[String]) -> Option<Node> {
    for name in wrappers.iter().rev() {
        node = schema.node_type(name)?.try_create(None, Some(singleton(node)), None).ok()?;
    }
    Some(node)
}

fn add_to_sibling(
    schema: &Schema, wrap: &[String], previous: &[String], node: &Node, sibling: &Node, depth: usize,
) -> Option<Node> {
    if wrap.get(depth)? != previous.get(depth)? {
        return None;
    }
    let last = sibling.child_count().checked_sub(1)?;
    if let Some(inner) = add_to_sibling(schema, wrap, previous, node, sibling.child(last), depth + 1) {
        return Some(sibling.copy(Some(sibling.content().replace_child(last, inner))));
    }
    let next = wrap.get(depth + 1).map_or(node.type_name(), String::as_str);
    sibling.content_match_at(sibling.child_count())?.match_type(next)?;
    let wrapped = wrap_node(schema, node.clone(), &wrap[depth + 1..])?;
    Some(sibling.copy(Some(sibling.content().append(&singleton(wrapped)))))
}

fn close_right(schema: &Schema, node: &Node, depth: usize) -> Option<Node> {
    if depth == 0 {
        return Some(node.clone());
    }
    let last = node.child_count().checked_sub(1)?;
    let child = close_right(schema, node.child(last), depth - 1)?;
    let fill = node.content_match_at(node.child_count())?.fill_before(schema, &Fragment::empty(), true, 0)?;
    Some(node.copy(Some(node.content().replace_child(last, child).append(&fill))))
}

fn close_slice(schema: &Schema, mut slice: Slice, start: usize, end: usize) -> Slice {
    if start < slice.open_start {
        let range = CloseRange { start, end: slice.open_start, left: true };
        if let Some(content) = range.apply(schema, &slice.content, 0, slice.open_end) {
            slice.content = content;
            slice.open_start = start;
        }
    }
    if end < slice.open_end {
        let range = CloseRange { start: end, end: slice.open_end, left: false };
        if let Some(content) = range.apply(schema, &slice.content, 0, 0) {
            slice.content = content;
            slice.open_end = end;
        }
    }
    slice
}

struct CloseRange {
    start: usize,
    end: usize,
    left: bool,
}

impl CloseRange {
    fn apply(&self, schema: &Schema, fragment: &Fragment, depth: usize, mut open_end: usize) -> Option<Fragment> {
        let index = if self.left { 0 } else { fragment.child_count().checked_sub(1)? };
        let node = fragment.children.get(index)?;
        let mut inner = node.content().clone();
        if fragment.child_count() > 1 {
            open_end = 0;
        }
        if depth + 1 < self.end {
            inner = self.apply(schema, &inner, depth + 1, open_end)?;
        }
        if depth >= self.start {
            if self.left {
                inner = node.content_match_at(0)?.fill_before(schema, &inner, open_end <= depth, 0)?.append(&inner);
            } else {
                let fill = node.content_match_at(node.child_count())?.fill_before(schema, &Fragment::empty(), true, 0)?;
                inner = inner.append(&fill);
            }
        }
        Some(fragment.replace_child(index, node.copy(Some(inner))))
    }
}

fn strip_trailing_breaks(dom: &mut DomNode) {
    let children = match dom {
        DomNode::Element(element) => {
            let inline = matches!(element.name.as_str(),
                "a" | "abbr" | "acronym" | "b" | "cite" | "code" | "del" | "em" | "i" | "ins" |
                "kbd" | "label" | "output" | "q" | "ruby" | "s" | "samp" | "span" | "strong" |
                "sub" | "sup" | "time" | "u" | "tt" | "var");
            if !inline && element.children.last().and_then(DomNode::as_element).is_some_and(|child| child.name == "br") {
                element.children.pop();
            }
            &mut element.children
        }
        DomNode::Fragment(children) => children,
        DomNode::Text(_) | DomNode::Comment(_) => return,
    };
    for child in children {
        strip_trailing_breaks(child);
    }
}

fn collect_styles(dom: &DomNode, styles: &mut Vec<(String, String)>) {
    if let Some(element) = dom.as_element() {
        if element.name == "style" {
            for text in &element.children {
                if let DomNode::Text(text) = text {
                    styles.extend(text.split('}').filter_map(|rule| {
                        let (selector, declarations) = rule.split_once('{')?;
                        Some((selector.trim().to_owned(), declarations.trim().to_owned()))
                    }));
                }
            }
            return;
        }
    }
    for child in dom.children() {
        collect_styles(child, styles);
    }
}

fn apply_styles(dom: &mut DomNode, styles: &[(String, String)]) {
    let children = match dom {
        DomNode::Element(element) => {
            let rules: Vec<_> = styles.iter().filter(|(selector, _)| element.matches(selector))
                .map(|(_, declarations)| declarations.as_str()).collect();
            if !rules.is_empty() {
                let mut style = element.attr("style").unwrap_or_default().to_owned();
                for rule in rules {
                    style.push(';');
                    style.push_str(rule);
                }
                element.attributes.retain(|attr| attr.name != "style" || attr.namespace.is_some());
                element.attributes.push(DomAttribute::new("style", style));
            }
            &mut element.children
        }
        DomNode::Fragment(children) => children,
        DomNode::Text(_) | DomNode::Comment(_) => return,
    };
    for child in children {
        apply_styles(child, styles);
    }
}
