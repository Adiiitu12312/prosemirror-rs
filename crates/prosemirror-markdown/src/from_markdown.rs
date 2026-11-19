use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use prosemirror_model::{Fragment, Mark, ModelError, Node, Schema};
use serde_json::{json, Value};

use crate::{schema, CommonMarkTokenizer, MarkdownError, MarkdownTokenizer, Token};

/// Computes the attributes for a token from its full stream and index.
pub type GetAttrs = Arc<dyn Fn(&Token, &[Token], usize) -> Option<Value> + Send + Sync>;

#[derive(Clone)]
pub struct ParseSpec {
    target: ParseTarget,
    pub attrs: Option<Value>,
    pub get_attrs: Option<GetAttrs>,
    pub no_close_token: bool,
}

#[derive(Clone, Debug)]
enum ParseTarget {
    Node(String),
    Block(String),
    Mark(String),
    Ignore,
}

impl ParseSpec {
    pub fn node(name: impl Into<String>) -> Self {
        Self::for_target(ParseTarget::Node(name.into()))
    }

    pub fn block(name: impl Into<String>) -> Self {
        Self::for_target(ParseTarget::Block(name.into()))
    }

    pub fn mark(name: impl Into<String>) -> Self {
        Self::for_target(ParseTarget::Mark(name.into()))
    }

    pub fn ignore() -> Self {
        Self::for_target(ParseTarget::Ignore)
    }

    pub fn with_get_attrs(
        mut self,
        get_attrs: impl Fn(&Token, &[Token], usize) -> Option<Value> + Send + Sync + 'static,
    ) -> Self {
        self.get_attrs = Some(Arc::new(get_attrs));
        self
    }

    pub fn with_attrs(mut self, attrs: Value) -> Self {
        self.attrs = Some(attrs);
        self
    }

    pub fn without_close_token(mut self) -> Self {
        self.no_close_token = true;
        self
    }

    fn for_target(target: ParseTarget) -> Self {
        Self { target, attrs: None, get_attrs: None, no_close_token: false }
    }

    fn attributes(&self, token: &Token, stream: &[Token], index: usize) -> Option<Value> {
        match &self.get_attrs {
            Some(get) => get(token, stream, index),
            None => self.attrs.clone(),
        }
    }
}

#[derive(Clone)]
enum Handler {
    Node(String, ParseSpec),
    OpenBlock(String, ParseSpec),
    CloseBlock,
    BlockContent(String, ParseSpec),
    OpenMark(String, ParseSpec),
    CloseMark(String),
    MarkContent(String, ParseSpec),
    Ignore,
    Text,
    Inline,
    SoftBreak,
}

/// Parses tokens from a CommonMark or custom tokenizer into a ProseMirror schema.
pub struct MarkdownParser {
    pub schema: Schema,
    pub tokenizer: Arc<dyn MarkdownTokenizer>,
    specs: Vec<(String, ParseSpec)>,
    handlers: HashMap<String, Handler>,
}

impl MarkdownParser {
    pub fn new(
        schema: Schema,
        tokenizer: impl MarkdownTokenizer + 'static,
        tokens: impl IntoIterator<Item = (String, ParseSpec)>,
    ) -> Result<Self, MarkdownError> {
        let specs: Vec<_> = tokens.into_iter().collect();
        let mut handlers = HashMap::new();
        for (kind, spec) in &specs {
            let no_close_token = spec.no_close_token || matches!(kind.as_str(), "code_inline" | "code_block" | "fence");
            let (open, close) = match &spec.target {
                ParseTarget::Node(name) => {
                    require_node(&schema, name)?;
                    (Handler::Node(name.clone(), spec.clone()), None)
                }
                ParseTarget::Block(name) => {
                    require_node(&schema, name)?;
                    if no_close_token {
                        (Handler::BlockContent(name.clone(), spec.clone()), None)
                    } else {
                        (Handler::OpenBlock(name.clone(), spec.clone()), Some(Handler::CloseBlock))
                    }
                }
                ParseTarget::Mark(name) => {
                    require_mark(&schema, name)?;
                    if no_close_token {
                        (Handler::MarkContent(name.clone(), spec.clone()), None)
                    } else {
                        (Handler::OpenMark(name.clone(), spec.clone()), Some(Handler::CloseMark(name.clone())))
                    }
                }
                ParseTarget::Ignore if no_close_token => (Handler::Ignore, None),
                ParseTarget::Ignore => (Handler::Ignore, Some(Handler::Ignore)),
            };
            if let Some(close) = close {
                handlers.insert(format!("{kind}_open"), open);
                handlers.insert(format!("{kind}_close"), close);
            } else {
                handlers.insert(kind.clone(), open);
            }
        }
        handlers.insert("text".to_owned(), Handler::Text);
        handlers.insert("inline".to_owned(), Handler::Inline);
        handlers.entry("softbreak".to_owned()).or_insert(Handler::SoftBreak);
        Ok(Self { schema, tokenizer: Arc::new(tokenizer), specs, handlers })
    }

    pub fn tokens(&self) -> &[(String, ParseSpec)] {
        &self.specs
    }

    pub fn parse(&self, source: &str) -> Result<Node, MarkdownError> {
        self.parse_with_environment(source, &mut json!({}))
    }

    pub fn parse_with_environment(&self, source: &str, env: &mut Value) -> Result<Node, MarkdownError> {
        self.parse_tokens(&self.tokenizer.tokenize(source, env)?)
    }

    pub fn parse_tokens(&self, tokens: &[Token]) -> Result<Node, MarkdownError> {
        let mut state = ParseState {
            schema: &self.schema,
            handlers: &self.handlers,
            stack: Vec::new(),
        };
        state.open_node(&self.schema.top_node_type().name, None);
        state.parse(tokens)?;
        let mut doc = None;
        while !state.stack.is_empty() {
            doc = state.close_node()?;
        }
        if let Some(doc) = doc {
            return Ok(doc);
        }
        self.schema.top_node_type().create_and_fill(None, None, None)?
            .ok_or(MarkdownError::MissingDocument)
    }
}

struct Level {
    type_name: String,
    attrs: Option<Value>,
    content: Vec<Node>,
    marks: Vec<Mark>,
}

struct ParseState<'a> {
    schema: &'a Schema,
    handlers: &'a HashMap<String, Handler>,
    stack: Vec<Level>,
}

impl ParseState<'_> {
    fn parse(&mut self, tokens: &[Token]) -> Result<(), MarkdownError> {
        for (index, token) in tokens.iter().enumerate() {
            let handler = self.handlers.get(&token.kind)
                .ok_or_else(|| MarkdownError::UnsupportedToken(token.kind.clone()))?;
            match handler {
                Handler::Text => self.add_text(&token.content)?,
                Handler::Inline => self.parse(&token.children)?,
                Handler::SoftBreak => self.add_text(" ")?,
                Handler::Ignore => (),
                Handler::Node(name, spec) => {
                    self.add_node(name, spec.attributes(token, tokens, index), Vec::new())?;
                }
                Handler::OpenBlock(name, spec) => self.open_node(name, spec.attributes(token, tokens, index)),
                Handler::CloseBlock => {
                    if self.stack.len() <= 1 {
                        return Err(MarkdownError::UnmatchedClose);
                    }
                    self.close_node()?;
                }
                Handler::BlockContent(name, spec) => {
                    self.open_node(name, spec.attributes(token, tokens, index));
                    self.add_text(strip_final_newline(&token.content))?;
                    self.close_node()?;
                }
                Handler::OpenMark(name, spec) => self.open_mark(name, spec.attributes(token, tokens, index))?,
                Handler::CloseMark(name) => self.close_mark(name)?,
                Handler::MarkContent(name, spec) => {
                    self.open_mark(name, spec.attributes(token, tokens, index))?;
                    self.add_text(strip_final_newline(&token.content))?;
                    self.close_mark(name)?;
                }
            }
        }
        Ok(())
    }

    fn add_text(&mut self, text: &str) -> Result<(), MarkdownError> {
        if !text.is_empty() {
            let top = self.stack.last_mut().ok_or(MarkdownError::UnmatchedClose)?;
            top.content.push(self.schema.try_text(text, Some(top.marks.clone()))?);
        }
        Ok(())
    }

    fn open_node(&mut self, name: &str, attrs: Option<Value>) {
        self.stack.push(Level { type_name: name.to_owned(), attrs, content: Vec::new(), marks: Vec::new() });
    }

    fn close_node(&mut self) -> Result<Option<Node>, MarkdownError> {
        let top = self.stack.pop().ok_or(MarkdownError::UnmatchedClose)?;
        self.add_node(&top.type_name, top.attrs, top.content)
    }

    fn add_node(&mut self, name: &str, attrs: Option<Value>, children: Vec<Node>) -> Result<Option<Node>, MarkdownError> {
        let kind = self.schema.node_type(name).ok_or_else(|| ModelError::UnknownNodeType(name.to_owned()))?;
        let marks = self.stack.last().map(|level| level.marks.clone());
        let node = kind.create_and_fill(attrs, Some(Fragment::from_nodes(children)), marks)?;
        if let (Some(parent), Some(child)) = (self.stack.last_mut(), node.as_ref()) {
            parent.content.push(child.clone());
        }
        Ok(node)
    }

    fn open_mark(&mut self, name: &str, attrs: Option<Value>) -> Result<(), MarkdownError> {
        let kind = self.schema.mark_type(name).ok_or_else(|| ModelError::UnknownMarkType(name.to_owned()))?;
        let top = self.stack.last_mut().ok_or(MarkdownError::UnmatchedClose)?;
        top.marks = Mark::add_to_set(&top.marks, kind.try_create(attrs)?);
        Ok(())
    }

    fn close_mark(&mut self, name: &str) -> Result<(), MarkdownError> {
        let top = self.stack.last_mut().ok_or(MarkdownError::UnmatchedClose)?;
        top.marks.retain(|mark| mark.type_name != name);
        Ok(())
    }
}

fn require_node(schema: &Schema, name: &str) -> Result<(), ModelError> {
    schema.node_type(name).map(|_| ()).ok_or_else(|| ModelError::UnknownNodeType(name.to_owned()))
}

fn require_mark(schema: &Schema, name: &str) -> Result<(), ModelError> {
    schema.mark_type(name).map(|_| ()).ok_or_else(|| ModelError::UnknownMarkType(name.to_owned()))
}

fn strip_final_newline(content: &str) -> &str {
    content.strip_suffix('\n').unwrap_or(content)
}

fn tight_list(tokens: &[Token], index: usize) -> bool {
    tokens.iter().skip(index + 1).find(|token| token.kind != "list_item_open")
        .is_some_and(|token| token.hidden)
}

/// Parses unextended CommonMark with HTML syntax disabled.
pub fn default_markdown_parser() -> &'static MarkdownParser {
    static PARSER: OnceLock<MarkdownParser> = OnceLock::new();
    PARSER.get_or_init(|| MarkdownParser::new(
        schema().clone(),
        CommonMarkTokenizer::default(),
        [
            ("blockquote", ParseSpec::block("blockquote")),
            ("paragraph", ParseSpec::block("paragraph")),
            ("list_item", ParseSpec::block("list_item")),
            ("bullet_list", ParseSpec::block("bullet_list").with_get_attrs(|_, tokens, index| {
                Some(json!({"tight": tight_list(tokens, index)}))
            })),
            ("ordered_list", ParseSpec::block("ordered_list").with_get_attrs(|token, tokens, index| {
                let start = token.attr("start").and_then(|start| start.parse::<u32>().ok())
                    .filter(|start| *start != 0).unwrap_or(1);
                Some(json!({"order": start, "tight": tight_list(tokens, index)}))
            })),
            ("heading", ParseSpec::block("heading").with_get_attrs(|token, _, _| {
                let level = token.tag.strip_prefix('h').and_then(|level| level.parse::<u8>().ok());
                Some(json!({"level": level}))
            })),
            ("code_block", ParseSpec::block("code_block")),
            ("fence", ParseSpec::block("code_block").with_get_attrs(|token, _, _| {
                Some(json!({"params": token.info}))
            })),
            ("hr", ParseSpec::node("horizontal_rule")),
            ("image", ParseSpec::node("image").with_get_attrs(|token, _, _| Some(json!({
                "src": token.attr("src"),
                "title": token.attr("title").filter(|title| !title.is_empty()),
                "alt": token.children.first().map(|token| token.content.as_str()).filter(|alt| !alt.is_empty()),
            })))),
            ("hardbreak", ParseSpec::node("hard_break")),
            ("em", ParseSpec::mark("em")),
            ("strong", ParseSpec::mark("strong")),
            ("link", ParseSpec::mark("link").with_get_attrs(|token, _, _| Some(json!({
                "href": token.attr("href"),
                "title": token.attr("title").filter(|title| !title.is_empty()),
            })))),
            ("code_inline", ParseSpec::mark("code")),
        ].into_iter().map(|(name, spec)| (name.to_owned(), spec)),
    ).expect("the built-in Markdown specification and schema must agree"))
}
