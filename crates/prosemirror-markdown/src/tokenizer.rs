use std::collections::HashMap;

use markdown_it::parser::linkfmt::{LinkFormatter, MDLinkFormatter};
use markdown_it::parser::inline::{Text, TextSpecial};
use markdown_it::plugins::cmark::{self, block, inline};
use markdown_it::{MarkdownIt, Node as AstNode};
use mdurl::urlencode;
use serde_json::Value;

use crate::MarkdownError;

/// A Markdown-it-compatible token consumed by the configurable parser.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Token {
    pub kind: String,
    pub tag: String,
    pub attributes: HashMap<String, String>,
    pub content: String,
    pub children: Vec<Token>,
    pub info: String,
    pub markup: String,
    pub hidden: bool,
}

impl Token {
    pub fn new(kind: impl Into<String>, tag: impl Into<String>) -> Self {
        Self { kind: kind.into(), tag: tag.into(), ..Self::default() }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }

    fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    fn with_attr(mut self, name: &str, value: impl Into<String>) -> Self {
        self.attributes.insert(name.to_owned(), value.into());
        self
    }
}

pub trait MarkdownTokenizer: Send + Sync {
    fn tokenize(&self, source: &str, environment: &mut Value) -> Result<Vec<Token>, MarkdownError>;
}

impl<F> MarkdownTokenizer for F
where
    F: Fn(&str, &mut Value) -> Result<Vec<Token>, MarkdownError> + Send + Sync,
{
    fn tokenize(&self, source: &str, environment: &mut Value) -> Result<Vec<Token>, MarkdownError> {
        self(source, environment)
    }
}

/// An unextended CommonMark tokenizer with HTML parsing disabled.
#[derive(Debug)]
pub struct CommonMarkTokenizer {
    parser: MarkdownIt,
}

impl Default for CommonMarkTokenizer {
    fn default() -> Self {
        let mut parser = MarkdownIt::new();
        cmark::add(&mut parser);
        parser.link_formatter = Box::new(CommonMarkLinkFormatter);
        Self { parser }
    }
}

#[derive(Debug)]
struct CommonMarkLinkFormatter;

impl LinkFormatter for CommonMarkLinkFormatter {
    fn validate_link(&self, url: &str) -> Option<()> {
        MDLinkFormatter::new().validate_link(url.trim())
    }

    fn normalize_link(&self, url: &str) -> String {
        let mut parsed = mdurl::parse_url(url);
        recode_hostname(&mut parsed, |label| {
            if label.is_ascii() {
                Some(label.to_owned())
            } else {
                idna::punycode::encode_str(label).map(|encoded| format!("xn--{encoded}"))
            }
        });
        urlencode::encode(&parsed.to_string(), urlencode::ENCODE_DEFAULT_CHARS, true).into_owned()
    }

    fn normalize_link_text(&self, url: &str) -> String {
        let mut parsed = mdurl::parse_url(url);
        recode_hostname(&mut parsed, |label| {
            label.strip_prefix("xn--").map_or_else(
                || Some(label.to_owned()),
                |encoded| idna::punycode::decode_to_string(&encoded.to_ascii_lowercase()),
            )
        });
        urlencode::decode(&parsed.to_string(), urlencode::DECODE_DEFAULT_CHARS.add(b'%')).into_owned()
    }
}

fn recode_hostname(url: &mut mdurl::Url, recode: impl Fn(&str) -> Option<String>) {
    if !matches!(url.protocol.as_deref(), None | Some("http:" | "https:" | "mailto:")) {
        return;
    }
    if let Some(hostname) = &mut url.hostname {
        if let Some(labels) = hostname.split(['.', '\u{3002}', '\u{ff0e}', '\u{ff61}'])
            .map(recode).collect::<Option<Vec<_>>>()
        {
            *hostname = labels.join(".");
        }
    }
}

impl MarkdownTokenizer for CommonMarkTokenizer {
    fn tokenize(&self, source: &str, _environment: &mut Value) -> Result<Vec<Token>, MarkdownError> {
        let root = self.parser.parse(source);
        let mut converter = Converter { source, tokens: Vec::new() };
        converter.emit_children(&root.children)?;
        Ok(converter.tokens)
    }
}

struct Converter<'a> {
    source: &'a str,
    tokens: Vec<Token>,
}

impl Converter<'_> {
    fn emit_children(&mut self, children: &[AstNode]) -> Result<(), MarkdownError> {
        for child in children {
            self.emit(child)?;
        }
        Ok(())
    }

    fn emit(&mut self, node: &AstNode) -> Result<(), MarkdownError> {
        if let Some(text) = node.cast::<Text>() {
            self.tokens.push(Token::new("text", "").with_content(&text.content));
        } else if let Some(text) = node.cast::<TextSpecial>() {
            self.tokens.push(Token::new("text", "").with_content(&text.content));
        } else if node.is::<block::paragraph::Paragraph>() {
            self.emit_inline_block("paragraph", "p", &node.children, false)?;
        } else if node.is::<block::blockquote::Blockquote>() {
            self.emit_container("blockquote", "blockquote", &node.children)?;
        } else if let Some(heading) = node.cast::<block::heading::ATXHeading>() {
            self.emit_inline_block("heading", &format!("h{}", heading.level), &node.children, false)?;
        } else if let Some(heading) = node.cast::<block::lheading::SetextHeader>() {
            self.emit_inline_block("heading", &format!("h{}", heading.level), &node.children, false)?;
        } else if let Some(list) = node.cast::<block::list::OrderedList>() {
            let mut open = Token::new("ordered_list_open", "ol");
            open.markup.push(list.marker);
            if list.start != 1 {
                open = open.with_attr("start", list.start.to_string());
            }
            self.emit_list(open, "ordered_list_close", &node.children)?;
        } else if let Some(list) = node.cast::<block::list::BulletList>() {
            let mut open = Token::new("bullet_list_open", "ul");
            open.markup.push(list.marker);
            self.emit_list(open, "bullet_list_close", &node.children)?;
        } else if let Some(code) = node.cast::<block::code::CodeBlock>() {
            self.tokens.push(Token::new("code_block", "code").with_content(&code.content));
        } else if let Some(fence) = node.cast::<block::fence::CodeFence>() {
            let mut token = Token::new("fence", "code").with_content(&fence.content);
            token.info = fence.info.clone();
            token.markup = fence.marker.to_string().repeat(fence.marker_len);
            self.tokens.push(token);
        } else if node.is::<block::hr::ThematicBreak>() {
            self.tokens.push(Token::new("hr", "hr"));
        } else if node.is::<block::reference::Definition>() {
            // CommonMark uses definitions to resolve links rather than emitting content.
        } else if node.is::<inline::newline::Hardbreak>() {
            self.tokens.push(Token::new("hardbreak", "br"));
        } else if node.is::<inline::newline::Softbreak>() {
            self.tokens.push(Token::new("softbreak", "br"));
        } else if node.is::<inline::emphasis::Em>() {
            self.emit_container("em", "em", &node.children)?;
        } else if node.is::<inline::emphasis::Strong>() {
            self.emit_container("strong", "strong", &node.children)?;
        } else if let Some(code) = node.cast::<inline::backticks::CodeInline>() {
            let content = self.inline_code_content(node, code.marker_len);
            let mut token = Token::new("code_inline", "code").with_content(content);
            token.markup = "`".repeat(code.marker_len);
            self.tokens.push(token);
        } else if let Some(link) = node.cast::<inline::link::Link>() {
            self.emit_link(&link.url, link.title.as_deref(), &node.children)?;
        } else if let Some(link) = node.cast::<inline::autolink::Autolink>() {
            self.emit_link(&link.url, None, &node.children)?;
        } else if let Some(image) = node.cast::<inline::image::Image>() {
            let mut token = Token::new("image", "img").with_attr("src", &image.url).with_attr("alt", "");
            token.children = self.inline_tokens(&node.children, false)?;
            if let Some(title) = &image.title {
                token = token.with_attr("title", title);
            }
            self.tokens.push(token);
        } else {
            return Err(MarkdownError::UnsupportedToken(node.name().to_owned()));
        }
        Ok(())
    }

    fn emit_container(&mut self, kind: &str, tag: &str, children: &[AstNode]) -> Result<(), MarkdownError> {
        self.tokens.push(Token::new(format!("{kind}_open"), tag));
        self.emit_children(children)?;
        self.tokens.push(Token::new(format!("{kind}_close"), tag));
        Ok(())
    }

    fn emit_link(&mut self, url: &str, title: Option<&str>, children: &[AstNode]) -> Result<(), MarkdownError> {
        let mut open = Token::new("link_open", "a").with_attr("href", url);
        if let Some(title) = title {
            open = open.with_attr("title", title);
        }
        self.tokens.push(open);
        self.emit_children(children)?;
        self.tokens.push(Token::new("link_close", "a"));
        Ok(())
    }

    fn emit_inline_block(&mut self, kind: &str, tag: &str, children: &[AstNode], hidden: bool) -> Result<(), MarkdownError> {
        let mut open = Token::new(format!("{kind}_open"), tag);
        open.hidden = hidden;
        let mut close = Token::new(format!("{kind}_close"), tag);
        close.hidden = hidden;
        let mut inline = Token::new("inline", "");
        inline.children = self.inline_tokens(children, true)?;
        self.tokens.extend([open, inline, close]);
        Ok(())
    }

    fn inline_tokens(&self, children: &[AstNode], merge_text: bool) -> Result<Vec<Token>, MarkdownError> {
        let mut child_parser = Self { source: self.source, tokens: Vec::new() };
        child_parser.emit_children(children)?;
        if !merge_text {
            return Ok(child_parser.tokens);
        }
        let mut tokens: Vec<Token> = Vec::new();
        for token in child_parser.tokens {
            if let Some(previous) = tokens.last_mut() {
                if previous.kind == "text" && token.kind == "text" {
                    previous.content.push_str(&token.content);
                    continue;
                }
            }
            tokens.push(token);
        }
        Ok(tokens)
    }

    fn emit_list(&mut self, open: Token, close_kind: &str, items: &[AstNode]) -> Result<(), MarkdownError> {
        let mut close = Token::new(close_kind, &open.tag);
        close.markup.clone_from(&open.markup);
        self.tokens.push(open);
        for item in items {
            self.tokens.push(Token::new("list_item_open", "li"));
            let mut index = 0;
            while index < item.children.len() {
                let child = &item.children[index];
                if is_block(child) {
                    self.emit(child)?;
                    index += 1;
                } else {
                    // The native tokenizer flattens paragraphs in tight lists.
                    // Markdown-it keeps hidden paragraph wrappers around them.
                    let count = item.children[index..].iter().take_while(|child| !is_block(child)).count();
                    self.emit_inline_block("paragraph", "p", &item.children[index..index + count], true)?;
                    index += count;
                }
            }
            self.tokens.push(Token::new("list_item_close", "li"));
        }
        self.tokens.push(close);
        Ok(())
    }

    fn inline_code_content(&self, node: &AstNode, marker_length: usize) -> String {
        let text = node.children.first().and_then(|child| child.cast::<Text>())
            .map_or_else(String::new, |text| text.content.clone());
        if !text.bytes().all(|byte| byte == b' ') {
            return text;
        }
        // CommonMark leaves an all-space span intact. The native tokenizer
        // normalizes surrounding spaces before exposing the AST.
        let Some((from, to)) = node.srcmap.as_ref().map(markdown_it::common::sourcemap::SourcePos::get_byte_offsets) else {
            return text;
        };
        self.source.get(from + marker_length..to.saturating_sub(marker_length))
            .filter(|raw| raw.bytes().all(|byte| byte == b' '))
            .map_or(text, str::to_owned)
    }
}

fn is_block(node: &AstNode) -> bool {
    node.is::<block::paragraph::Paragraph>()
        || node.is::<block::blockquote::Blockquote>()
        || node.is::<block::heading::ATXHeading>()
        || node.is::<block::lheading::SetextHeader>()
        || node.is::<block::list::OrderedList>()
        || node.is::<block::list::BulletList>()
        || node.is::<block::code::CodeBlock>()
        || node.is::<block::fence::CodeFence>()
        || node.is::<block::hr::ThematicBreak>()
        || node.is::<block::reference::Definition>()
}
