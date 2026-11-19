use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use prosemirror_model::{Mark, Node};
use regex::Regex;

use crate::MarkdownError;

pub type NodeSerializer = Arc<
    dyn Fn(&mut MarkdownSerializerState<'_>, &Node, &Node, usize) -> Result<(), MarkdownError>
        + Send + Sync,
>;
pub type MarkDelimiterFn =
    Arc<dyn Fn(&mut MarkdownSerializerState<'_>, &Mark, &Node, usize) -> String + Send + Sync>;

/// Opening or closing syntax for a mark.
#[derive(Clone)]
pub enum MarkDelimiter {
    Literal(String),
    Dynamic(MarkDelimiterFn),
}

impl From<&str> for MarkDelimiter {
    fn from(value: &str) -> Self {
        Self::Literal(value.to_owned())
    }
}

impl MarkDelimiter {
    pub fn dynamic(
        render: impl Fn(&mut MarkdownSerializerState<'_>, &Mark, &Node, usize) -> String
            + Send + Sync + 'static,
    ) -> Self {
        Self::Dynamic(Arc::new(render))
    }
}

#[derive(Clone)]
pub struct MarkSerializerSpec {
    pub open: MarkDelimiter,
    pub close: MarkDelimiter,
    pub mixable: bool,
    pub expel_enclosing_whitespace: bool,
    pub escape: bool,
}

impl MarkSerializerSpec {
    pub fn new(open: impl Into<MarkDelimiter>, close: impl Into<MarkDelimiter>) -> Self {
        Self { open: open.into(), close: close.into(), mixable: false, expel_enclosing_whitespace: false, escape: true }
    }
}

static BLANK_MARK: MarkSerializerSpec = MarkSerializerSpec {
    open: MarkDelimiter::Literal(String::new()),
    close: MarkDelimiter::Literal(String::new()),
    mixable: true,
    expel_enclosing_whitespace: false,
    escape: true,
};

#[derive(Clone, Debug, Default)]
pub struct MarkdownSerializerOptions {
    pub tight_lists: Option<bool>,
    pub escape_extra_characters: Option<Regex>,
    pub hard_break_node_name: Option<String>,
    pub strict: Option<bool>,
}

impl MarkdownSerializerOptions {
    fn merge(&self, other: Self) -> Self {
        Self {
            tight_lists: other.tight_lists.or(self.tight_lists),
            escape_extra_characters: other.escape_extra_characters.or_else(|| self.escape_extra_characters.clone()),
            hard_break_node_name: other.hard_break_node_name.or_else(|| self.hard_break_node_name.clone()),
            strict: other.strict.or(self.strict),
        }
    }
}

/// A configurable serializer from the ProseMirror document model to CommonMark.
#[derive(Clone)]
pub struct MarkdownSerializer {
    pub nodes: HashMap<String, NodeSerializer>,
    pub marks: HashMap<String, MarkSerializerSpec>,
    pub options: MarkdownSerializerOptions,
}

impl MarkdownSerializer {
    pub fn new(
        nodes: HashMap<String, NodeSerializer>,
        marks: HashMap<String, MarkSerializerSpec>,
        options: MarkdownSerializerOptions,
    ) -> Self {
        Self { nodes, marks, options }
    }

    pub fn serialize(&self, content: &Node) -> Result<String, MarkdownError> {
        self.serialize_with_options(content, MarkdownSerializerOptions::default())
    }

    pub fn serialize_with_options(
        &self, content: &Node, options: MarkdownSerializerOptions,
    ) -> Result<String, MarkdownError> {
        let mut state = MarkdownSerializerState::new(&self.nodes, &self.marks, self.options.merge(options));
        state.render_content(content)?;
        Ok(state.out)
    }
}

/// State and formatting helpers available to node and mark serializers.
pub struct MarkdownSerializerState<'a> {
    nodes: &'a HashMap<String, NodeSerializer>,
    marks: &'a HashMap<String, MarkSerializerSpec>,
    pub options: MarkdownSerializerOptions,
    pub out: String,
    pub delim: String,
    pub in_autolink: bool,
    pub at_block_start: bool,
    closed: Option<String>,
    in_tight_list: bool,
}

impl<'a> MarkdownSerializerState<'a> {
    pub fn new(
        nodes: &'a HashMap<String, NodeSerializer>,
        marks: &'a HashMap<String, MarkSerializerSpec>,
        options: MarkdownSerializerOptions,
    ) -> Self {
        Self {
            nodes, marks, options, out: String::new(), delim: String::new(),
            in_autolink: false, at_block_start: false, closed: None, in_tight_list: false,
        }
    }

    pub fn flush_close(&mut self, size: usize) {
        if self.closed.take().is_none() {
            return;
        }
        self.ensure_newline();
        let delim = self.delim.trim_end_matches(js_whitespace);
        for _ in 1..size {
            self.out.push_str(delim);
            self.out.push('\n');
        }
    }

    pub fn get_mark(&self, name: &str) -> Result<&MarkSerializerSpec, MarkdownError> {
        if let Some(spec) = self.marks.get(name) {
            Ok(spec)
        } else if self.options.strict == Some(false) {
            Ok(&BLANK_MARK)
        } else {
            Err(MarkdownError::UnsupportedMark(name.to_owned()))
        }
    }

    pub fn wrap_block(
        &mut self, delim: &str, first_delim: Option<&str>, node: &Node,
        render: impl FnOnce(&mut Self) -> Result<(), MarkdownError>,
    ) -> Result<(), MarkdownError> {
        let previous_delim = self.delim.len();
        self.write(first_delim.unwrap_or(delim));
        self.delim.push_str(delim);
        let result = render(self);
        self.delim.truncate(previous_delim);
        result?;
        self.close_block(node);
        Ok(())
    }

    pub fn at_blank(&self) -> bool {
        self.out.is_empty() || self.out.ends_with('\n')
    }

    pub fn ensure_newline(&mut self) {
        if !self.at_blank() {
            self.out.push('\n');
        }
    }

    pub fn write(&mut self, content: &str) {
        self.flush_close(2);
        if self.at_blank() {
            self.out.push_str(&self.delim);
        }
        self.out.push_str(content);
    }

    pub fn close_block(&mut self, node: &Node) {
        self.closed = Some(node.type_name().to_owned());
    }

    pub fn text(&mut self, text: &str, escape: bool) {
        for (index, line) in text.split('\n').enumerate() {
            if index != 0 {
                self.out.push('\n');
            }
            self.write("");
            if !escape && line.starts_with('[') && self.out.ends_with('!') && !self.out.ends_with("\\!") {
                let at = self.out.len() - 1;
                self.out.insert(at, '\\');
            }
            if escape {
                let escaped = self.esc(line, self.at_block_start);
                self.out.push_str(&escaped);
            } else {
                self.out.push_str(line);
            }
        }
    }

    pub fn render(&mut self, node: &Node, parent: &Node, index: usize) -> Result<(), MarkdownError> {
        if let Some(render) = self.nodes.get(node.type_name()) {
            return render(self, node, parent, index);
        }
        if self.options.strict != Some(false) {
            return Err(MarkdownError::UnsupportedNode(node.type_name().to_owned()));
        }
        if !node.is_leaf() {
            if node.inline_content() {
                self.render_inline(node, true)?;
            } else {
                self.render_content(node)?;
            }
            if node.is_block() {
                self.close_block(node);
            }
        }
        Ok(())
    }

    pub fn render_content(&mut self, parent: &Node) -> Result<(), MarkdownError> {
        for (index, node) in parent.content().children.iter().enumerate() {
            self.render(node, parent, index)?;
        }
        Ok(())
    }

    pub fn render_inline(&mut self, parent: &Node, from_block_start: bool) -> Result<(), MarkdownError> {
        self.at_block_start = from_block_start;
        let mut active = Vec::new();
        let mut trailing = String::new();
        for index in 0..=parent.child_count() {
            self.render_inline_node(parent, index, &mut active, &mut trailing)?;
        }
        self.at_block_start = false;
        Ok(())
    }

    fn render_inline_node(
        &mut self, parent: &Node, index: usize, active: &mut Vec<Mark>, trailing: &mut String,
    ) -> Result<(), MarkdownError> {
        let mut node = parent.maybe_child(index).cloned();
        let mut marks = node.as_ref().map_or_else(Vec::new, |node| node.marks().to_vec());
        if node.as_ref().is_some_and(|node| node.type_name() == self.hard_break_name()) {
            marks.retain(|mark| parent.maybe_child(index + 1).is_some_and(|next| {
                mark.is_in_set(next.marks()) && next.text().is_none_or(|text| text.chars().any(|ch| !js_whitespace(ch)))
            }));
        }

        let mut leading = std::mem::take(trailing);
        if let Some(value) = node.as_ref().and_then(Node::text) {
            if self.expels_whitespace(&marks, |mark| !mark.is_in_set(active))? {
                let (lead, rest) = split_leading(value);
                if !lead.is_empty() {
                    leading.push_str(lead);
                    node = inline_text(rest, &marks);
                    if node.is_none() {
                        marks.clone_from(active);
                    }
                }
            }
        }
        if let Some(value) = node.as_ref().and_then(Node::text) {
            if self.expels_whitespace(&marks, |mark| !self.is_mark_ahead(parent, index + 1, mark))? {
                let (rest, trail) = split_trailing(value);
                if !trail.is_empty() {
                    trailing.push_str(trail);
                    node = inline_text(rest, &marks);
                    if node.is_none() {
                        marks.clone_from(active);
                    }
                }
            }
        }

        let inner = marks.last().cloned();
        let no_escape = if let Some(mark) = &inner { !self.get_mark(&mark.type_name)?.escape } else { false };
        let length = marks.len() - usize::from(no_escape);
        self.align_mixable_marks(&mut marks[..length], active)?;
        let keep = active.iter().zip(&marks[..length]).take_while(|(left, right)| left == right).count();
        while active.len() > keep {
            let mark = active.pop().expect("there are marks to close");
            let delimiter = self.mark_string(&mark, false, parent, index)?;
            self.text(&delimiter, false);
        }
        if !leading.is_empty() {
            self.text(&leading, true);
        }
        if let Some(node) = node {
            for mark in &marks[active.len()..length] {
                active.push(mark.clone());
                let delimiter = self.mark_string(mark, true, parent, index)?;
                self.text(&delimiter, false);
                self.at_block_start = false;
            }
            if let (Some(mark), Some(value)) = (inner.filter(|_| no_escape), node.text()) {
                let mut rendered = self.mark_string(&mark, true, parent, index)?;
                rendered.push_str(value);
                rendered.push_str(&self.mark_string(&mark, false, parent, index + 1)?);
                self.text(&rendered, false);
            } else {
                self.render(&node, parent, index)?;
            }
            self.at_block_start = false;
        }
        Ok(())
    }

    fn expels_whitespace(&self, marks: &[Mark], predicate: impl Fn(&Mark) -> bool) -> Result<bool, MarkdownError> {
        for mark in marks {
            if self.get_mark(&mark.type_name)?.expel_enclosing_whitespace && predicate(mark) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn align_mixable_marks(&self, marks: &mut [Mark], active: &[Mark]) -> Result<(), MarkdownError> {
        for index in 0..marks.len() {
            if !self.get_mark(&marks[index].type_name)?.mixable {
                break;
            }
            for (previous_index, previous) in active.iter().enumerate() {
                if !self.get_mark(&previous.type_name)?.mixable {
                    break;
                }
                if marks[index] == *previous {
                    if index > previous_index {
                        marks[previous_index..=index].rotate_right(1);
                    } else if previous_index > index {
                        let stop = previous_index.min(marks.len());
                        marks[index..stop].rotate_left(1);
                    }
                    break;
                }
            }
        }
        Ok(())
    }

    pub fn render_list(
        &mut self, node: &Node, delim: &str, first_delim: impl Fn(usize) -> String,
    ) -> Result<(), MarkdownError> {
        if self.closed.as_deref() == Some(node.type_name()) {
            self.flush_close(3);
        } else if self.in_tight_list {
            self.flush_close(1);
        }
        let tight = node.attrs().and_then(|attrs| attrs.get("tight")).and_then(serde_json::Value::as_bool)
            .or(self.options.tight_lists).unwrap_or(false);
        let prev_tight = std::mem::replace(&mut self.in_tight_list, tight);
        let result = node.content().children.iter().enumerate().try_for_each(|(index, child)| {
            if index != 0 && tight {
                self.flush_close(1);
            }
            self.wrap_block(delim, Some(&first_delim(index)), node, |state| state.render(child, node, index))
        });
        self.in_tight_list = prev_tight;
        result
    }

    pub fn esc(&self, value: &str, start_of_line: bool) -> String {
        let mut escaped = String::with_capacity(value.len());
        for (index, current) in value.char_indices() {
            if matches!(current, '`' | '*' | '\\' | '~' | '[' | ']' | '_') {
                let internal_underscore = current == '_' && index > 0 && index + 1 < value.len()
                    && ascii_word(value.as_bytes()[index - 1]) && ascii_word(value.as_bytes()[index + 1]);
                if !internal_underscore {
                    escaped.push('\\');
                }
            }
            escaped.push(current);
        }
        if start_of_line {
            escape_block_start(&mut escaped);
        }
        if let Some(pattern) = &self.options.escape_extra_characters {
            escaped = pattern.replace_all(&escaped, |m: &regex::Captures<'_>| format!("\\{}", &m[0])).into_owned();
        }
        escaped
    }

    pub fn quote(value: &str) -> String {
        if !value.contains('"') {
            format!("\"{value}\"")
        } else if !value.contains('\'') {
            format!("'{value}'")
        } else {
            format!("({value})")
        }
    }

    pub fn mark_string(
        &mut self, mark: &Mark, open: bool, parent: &Node, index: usize,
    ) -> Result<String, MarkdownError> {
        let spec = self.get_mark(&mark.type_name)?;
        let delimiter = if open { &spec.open } else { &spec.close };
        match delimiter.clone() {
            MarkDelimiter::Literal(value) => Ok(value),
            MarkDelimiter::Dynamic(render) => Ok(render(self, mark, parent, index)),
        }
    }

    pub fn get_enclosing_whitespace(text: &str) -> (Option<&str>, Option<&str>) {
        let lead = text.len() - text.trim_start_matches(js_whitespace).len();
        let trail = text.trim_end_matches(js_whitespace).len();
        ((lead > 0).then_some(&text[..lead]), (trail < text.len()).then_some(&text[trail..]))
    }

    fn is_mark_ahead(&self, parent: &Node, mut index: usize, mark: &Mark) -> bool {
        while let Some(next) = parent.maybe_child(index) {
            if next.type_name() != self.hard_break_name() {
                return mark.is_in_set(next.marks());
            }
            index += 2;
        }
        false
    }

    fn hard_break_name(&self) -> &str {
        self.options.hard_break_node_name.as_deref().unwrap_or("hard_break")
    }
}

fn ascii_word(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_'
}

fn js_whitespace(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}'
        | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

fn escape_block_start(text: &mut String) {
    if text.starts_with(['-', '*', '>']) || text.starts_with("+ ") {
        text.insert(0, '\\');
        return;
    }
    let starting = text.trim_start_matches(js_whitespace);
    let start = text.len() - starting.len();
    let hashes = starting.bytes().take_while(|byte| *byte == b'#').count();
    if (1..=6).contains(&hashes) && starting[hashes..].chars().next().is_none_or(js_whitespace) {
        text.insert(start, '\\');
        return;
    }
    let digits = starting.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 {
        if let Some(whitespace) = starting[digits..].strip_prefix('.').and_then(|suffix| suffix.chars().next()).filter(|ch| js_whitespace(*ch)) {
            let at = start + digits;
            text.replace_range(at..at + 1 + whitespace.len_utf8(), "\\. ");
        }
    }
}

fn inline_text(text: &str, marks: &[Mark]) -> Option<Node> {
    (!text.is_empty()).then(|| Node::new_text(text, marks.to_vec()))
}

fn split_leading(text: &str) -> (&str, &str) {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| Regex::new(r"(?m)^([\s\x{feff}]*)([^\r\n\x{2028}\x{2029}]*)$").unwrap());
    let captured = pattern.captures(text).expect("every text has a first line");
    (captured.get(1).unwrap().as_str(), captured.get(2).unwrap().as_str())
}

fn split_trailing(text: &str) -> (&str, &str) {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| Regex::new(r"(?m)^([^\r\n\x{2028}\x{2029}]*?)([\s\x{feff}]*)$").unwrap());
    let captured = pattern.captures(text).expect("every text has a first line");
    (captured.get(1).unwrap().as_str(), captured.get(2).unwrap().as_str())
}

fn text_attr<'a>(node: &'a Node, key: &str) -> Option<&'a str> {
    node.attrs()?.get(key)?.as_str().filter(|value| !value.is_empty())
}

fn backtick_run(value: &str) -> usize {
    value.split(|ch| ch != '`').map(str::len).max().unwrap_or(0)
}

fn backticks_for(node: Option<&Node>, closing: bool) -> String {
    let longest = node.and_then(Node::text).map_or(0, backtick_run);
    let fence = "`".repeat(longest + 1);
    match (longest, closing) {
        (0, _) => fence,
        (_, true) => format!(" {fence}"),
        (_, false) => format!("{fence} "),
    }
}

fn is_plain_url(mark: &Mark, parent: &Node, index: usize) -> bool {
    let Some(attrs) = mark.attrs.as_ref() else { return false };
    let Some(href) = attrs.get("href").and_then(serde_json::Value::as_str) else { return false };
    let has_scheme = href.split_once(':').is_some_and(|(scheme, _)| {
        !scheme.is_empty() && scheme.bytes().all(ascii_word)
    });
    if attrs.get("title").and_then(serde_json::Value::as_str).is_some_and(|title| !title.is_empty()) || !has_scheme {
        return false;
    }
    parent.maybe_child(index).is_some_and(|content| {
        content.text() == Some(href) && content.marks().last() == Some(mark)
    }) && parent.maybe_child(index + 1).is_none_or(|next| !mark.is_in_set(next.marks()))
}

fn escape_selected(value: &str, selected: &[char]) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if selected.contains(&ch) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

fn title_suffix(title: Option<&str>) -> String {
    title.filter(|title| !title.is_empty()).map_or_else(String::new, |title| {
        format!(" \"{}\"", escape_selected(title, &['"']))
    })
}

/// Serializer for the CommonMark document schema.
pub fn default_markdown_serializer() -> &'static MarkdownSerializer {
    static SERIALIZER: OnceLock<MarkdownSerializer> = OnceLock::new();
    SERIALIZER.get_or_init(|| MarkdownSerializer::new(default_nodes(), default_marks(), MarkdownSerializerOptions::default()))
}

fn default_nodes() -> HashMap<String, NodeSerializer> {
    let mut nodes = HashMap::new();
    add_renderer(&mut nodes, "blockquote", |state, node, _, _| {
        state.wrap_block("> ", None, node, |state| state.render_content(node))
    });
    add_renderer(&mut nodes, "code_block", |state, node, _, _| {
        let text = node.text_content();
        let longest = backtick_run(&text);
        let fence = "`".repeat(if longest >= 3 { longest + 1 } else { 3 });
        state.write(&format!("{fence}{}\n", text_attr(node, "params").unwrap_or_default()));
        state.text(&text, false);
        state.write("\n");
        state.write(&fence);
        state.close_block(node);
        Ok(())
    });
    add_renderer(&mut nodes, "heading", |state, node, _, _| {
        let level = node.attrs().and_then(|attrs| attrs.get("level")).and_then(serde_json::Value::as_u64)
            .and_then(|level| usize::try_from(level).ok()).unwrap_or(0);
        state.write(&format!("{} ", "#".repeat(level)));
        state.render_inline(node, false)?;
        state.close_block(node);
        Ok(())
    });
    add_renderer(&mut nodes, "horizontal_rule", |state, node, _, _| {
        state.write(text_attr(node, "markup").unwrap_or("---"));
        state.close_block(node);
        Ok(())
    });
    add_renderer(&mut nodes, "bullet_list", |state, node, _, _| {
        let bullet = text_attr(node, "bullet").unwrap_or("*");
        state.render_list(node, "  ", |_| format!("{bullet} "))
    });
    add_renderer(&mut nodes, "ordered_list", |state, node, _, _| {
        let start = node.attrs().and_then(|attrs| attrs.get("order")).and_then(serde_json::Value::as_i64)
            .filter(|start| *start != 0).unwrap_or(1);
        let max_number = start.saturating_add(node.child_count() as i64).saturating_sub(1);
        let width = max_number.to_string().len();
        state.render_list(node, &" ".repeat(width + 2), |index| {
            let number = start.saturating_add(index as i64);
            format!("{number:>width$}. ")
        })
    });
    add_renderer(&mut nodes, "list_item", |state, node, _, _| state.render_content(node));
    add_renderer(&mut nodes, "paragraph", |state, node, _, _| {
        state.render_inline(node, true)?;
        state.close_block(node);
        Ok(())
    });
    add_renderer(&mut nodes, "image", |state, node, _, _| {
        let alt = state.esc(text_attr(node, "alt").unwrap_or_default(), false);
        let source = escape_selected(text_attr(node, "src").unwrap_or_default(), &['(', ')']);
        let title = title_suffix(text_attr(node, "title"));
        state.write(&format!("![{alt}]({source}{title})"));
        Ok(())
    });
    add_renderer(&mut nodes, "hard_break", |state, node, parent, index| {
        if parent.content().children.iter().skip(index + 1).any(|next| next.type_name() != node.type_name()) {
            state.write("\\\n");
        }
        Ok(())
    });
    add_renderer(&mut nodes, "text", |state, node, _, _| {
        state.text(node.text().unwrap_or_default(), !state.in_autolink);
        Ok(())
    });
    nodes
}

fn add_renderer(
    nodes: &mut HashMap<String, NodeSerializer>, name: &str,
    render: impl Fn(&mut MarkdownSerializerState<'_>, &Node, &Node, usize) -> Result<(), MarkdownError>
        + Send + Sync + 'static,
) {
    nodes.insert(name.to_owned(), Arc::new(render));
}

fn default_marks() -> HashMap<String, MarkSerializerSpec> {
    let emphasis = |delimiter| MarkSerializerSpec {
        mixable: true,
        expel_enclosing_whitespace: true,
        ..MarkSerializerSpec::new(delimiter, delimiter)
    };
    HashMap::from([
        ("em".to_owned(), emphasis("*")),
        ("strong".to_owned(), emphasis("**")),
        ("link".to_owned(), MarkSerializerSpec {
            open: MarkDelimiter::dynamic(|state, mark, parent, index| {
                state.in_autolink = is_plain_url(mark, parent, index);
                if state.in_autolink { "<".to_owned() } else { "[".to_owned() }
            }),
            close: MarkDelimiter::dynamic(|state, mark, _, _| {
                if std::mem::take(&mut state.in_autolink) {
                    return ">".to_owned();
                }
                let attrs = mark.attrs.as_ref();
                let href = attrs.and_then(|a| a.get("href")).and_then(serde_json::Value::as_str).unwrap_or_default();
                let title = attrs.and_then(|a| a.get("title")).and_then(serde_json::Value::as_str);
                format!("]({}{})", escape_selected(href, &['(', ')', '"']), title_suffix(title))
            }),
            mixable: true,
            expel_enclosing_whitespace: false,
            escape: true,
        }),
        ("code".to_owned(), MarkSerializerSpec {
            open: MarkDelimiter::dynamic(|_, _, parent, index| backticks_for(parent.maybe_child(index), false)),
            close: MarkDelimiter::dynamic(|_, _, parent, index| backticks_for(index.checked_sub(1).and_then(|i| parent.maybe_child(i)), true)),
            mixable: false,
            expel_enclosing_whitespace: false,
            escape: false,
        }),
    ])
}
