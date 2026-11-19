use std::collections::HashMap;
use std::ops::Deref;
use prosemirror_model::{Fragment, Mark, ModelError, Node, Schema};
use serde_json::Value;
use crate::schema::test_schema;

#[derive(Clone, Debug, PartialEq)]
pub struct TaggedDoc {
    pub node: Node,
    pub tags: HashMap<String, usize>,
}

impl Deref for TaggedDoc {
    type Target = Node;
    fn deref(&self) -> &Self::Target {
        &self.node
    }
}

impl TaggedDoc {
    pub fn tag(&self, name: &str) -> usize {
        *self.tags.get(name).unwrap_or_else(|| {
            panic!("Tag <{}> not found in TaggedDoc. Available: {:?}", name, self.tags.keys());
        })
    }

    pub fn get_tag(&self, name: &str) -> Option<usize> {
        self.tags.get(name).copied()
    }
}

pub struct BuilderContext {
    pub tags: HashMap<String, usize>,
    pub cur_pos: usize,
}

impl Default for BuilderContext {
    fn default() -> Self {
        Self::new()
    }
}

impl BuilderContext {
    pub fn new() -> Self {
        Self {
            tags: HashMap::new(),
            cur_pos: 0,
        }
    }

    pub fn parse_text(&mut self, text: &str, marks: Vec<Mark>) -> Node {
        let clean_text = parse_tags(text, &mut self.cur_pos, &mut self.tags);
        if clean_text.is_empty() {
            Node::new_text(clean_text, marks)
        } else {
            test_schema().text(&clean_text, Some(marks))
        }
    }

    pub fn wrap_block<F>(&mut self, type_name: &str, f: F) -> Node
    where
        F: FnOnce(&mut BuilderContext) -> Vec<Node>,
    {
        // Opening tag adds 1 to position (unless root doc)
        let is_doc = type_name == "doc";
        if !is_doc {
            self.cur_pos += 1;
        }

        let mut children = f(self);
        children.retain(|child| !child.is_text() || child.node_size() != 0);
        let fragment = Fragment::from_nodes(children);

        if !is_doc {
            self.cur_pos += 1;
        }

        test_schema().node_type(type_name).unwrap().create(None, Some(fragment), None)
    }
}

pub trait IntoBuilderNode {
    fn build(self, ctx: &mut BuilderContext) -> Node;
}

impl IntoBuilderNode for Node {
    fn build(self, ctx: &mut BuilderContext) -> Node {
        ctx.cur_pos += self.node_size();
        self
    }
}

fn parse_tags(text: &str, pos: &mut usize, tags: &mut HashMap<String, usize>) -> String {
    let mut result = String::new();
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if ch == '<' {
            let start = index + 1;
            let rest = &text[start..];
            let end = rest.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
            if end > 0 && rest.as_bytes().get(end) == Some(&b'>') {
                tags.insert(rest[..end].to_owned(), *pos);
                while chars.next_if(|(at, _)| *at <= start + end).is_some() {}
                continue;
            }
        }
        result.push(ch);
        *pos += ch.len_utf16();
    }
    result
}

/// A string, a tagged node, or a flattened group produced by a mark builder.
pub enum ChildSpec {
    Text(String),
    Node(TaggedDoc),
    Flat {
        nodes: Vec<Node>,
        tags: HashMap<String, usize>,
    },
}

impl From<&str> for ChildSpec {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<Node> for ChildSpec {
    fn from(node: Node) -> Self {
        TaggedDoc { node, tags: HashMap::new() }.into()
    }
}

impl From<TaggedDoc> for ChildSpec {
    fn from(node: TaggedDoc) -> Self {
        Self::Node(node)
    }
}

pub struct BuilderAlias {
    pub type_name: String,
    pub attrs: Option<Value>,
}

pub struct Builders<'a> {
    schema: &'a Schema,
    aliases: HashMap<String, BuilderAlias>,
}

pub fn builders(schema: &Schema, aliases: HashMap<String, BuilderAlias>) -> Builders<'_> {
    Builders { schema, aliases }
}

impl Builders<'_> {
    pub fn node(
        &self,
        name: &str,
        attrs: Option<Value>,
        children: Vec<ChildSpec>,
    ) -> Result<TaggedDoc, ModelError> {
        let (type_name, attrs) = self.resolve_alias(name, attrs);
        let node_type = self.schema.node_type(type_name)
            .ok_or_else(|| ModelError::UnknownNodeType(type_name.to_owned()))?;
        let (nodes, tags) = self.flatten(children, |node| node);
        let node = node_type.create(attrs, Some(Fragment::from_nodes(nodes)), None);
        Ok(TaggedDoc { node, tags })
    }

    pub fn mark(
        &self,
        name: &str,
        attrs: Option<Value>,
        children: Vec<ChildSpec>,
    ) -> Result<ChildSpec, ModelError> {
        let (type_name, attrs) = self.resolve_alias(name, attrs);
        let mark_type = self.schema.mark_type(type_name)
            .ok_or_else(|| ModelError::UnknownMarkType(type_name.to_owned()))?;
        let mark = mark_type.create(attrs);
        let (nodes, tags) = self.flatten(children, |node| {
            let marks = Mark::add_to_set(node.marks(), mark.clone());
            if marks.len() > node.marks().len() {
                node.with_marks(marks)
            } else {
                node
            }
        });
        Ok(ChildSpec::Flat { nodes, tags })
    }

    fn resolve_alias<'a>(&'a self, name: &'a str, mut attrs: Option<Value>) -> (&'a str, Option<Value>) {
        let Some(alias) = self.aliases.get(name) else {
            return (name, attrs);
        };
        if let Some(defaults) = alias.attrs.as_ref().and_then(Value::as_object) {
            let mut merged = defaults.clone();
            if let Some(overrides) = attrs.take().and_then(|value| value.as_object().cloned()) {
                merged.extend(overrides);
            }
            attrs = Some(Value::Object(merged));
        }
        (&alias.type_name, attrs)
    }

    fn flatten(
        &self,
        children: Vec<ChildSpec>,
        map: impl Fn(Node) -> Node,
    ) -> (Vec<Node>, HashMap<String, usize>) {
        let mut nodes = Vec::new();
        let mut tags = HashMap::new();
        let mut pos = 0;
        for child in children {
            match child {
                ChildSpec::Text(text) => {
                    let text = parse_tags(&text, &mut pos, &mut tags);
                    if !text.is_empty() {
                        nodes.push(map(self.schema.text(&text, None)));
                    }
                }
                ChildSpec::Node(child) => {
                    let start = pos + usize::from(!child.node.is_text());
                    tags.extend(child.tags.into_iter().map(|(name, offset)| (name, start + offset)));
                    let node = map(child.node);
                    pos += node.node_size();
                    nodes.push(node);
                }
                ChildSpec::Flat { nodes: children, tags: child_tags } => {
                    tags.extend(child_tags.into_iter().map(|(name, offset)| (name, pos + offset)));
                    for child in children {
                        let node = map(child);
                        pos += node.node_size();
                        nodes.push(node);
                    }
                }
            }
        }
        (nodes, tags)
    }
}

impl IntoBuilderNode for &str {
    fn build(self, ctx: &mut BuilderContext) -> Node {
        ctx.parse_text(self, Vec::new())
    }
}

impl IntoBuilderNode for String {
    fn build(self, ctx: &mut BuilderContext) -> Node {
        ctx.parse_text(&self, Vec::new())
    }
}

impl<F> IntoBuilderNode for F
where
    F: FnOnce(&mut BuilderContext) -> Node,
{
    fn build(self, ctx: &mut BuilderContext) -> Node {
        self(ctx)
    }
}

pub fn build_doc<F>(f: F) -> TaggedDoc
where
    F: FnOnce(&mut BuilderContext) -> Node,
{
    let mut ctx = BuilderContext::new();
    let node = f(&mut ctx);
    TaggedDoc {
        node,
        tags: ctx.tags,
    }
}

pub fn p<T: IntoBuilderNode>(children: Vec<T>) -> impl FnOnce(&mut BuilderContext) -> Node {
    move |ctx: &mut BuilderContext| {
        ctx.wrap_block("paragraph", |inner_ctx| {
            children.into_iter().map(|c| c.build(inner_ctx)).collect()
        })
    }
}

pub fn blockquote<T: IntoBuilderNode>(children: Vec<T>) -> impl FnOnce(&mut BuilderContext) -> Node {
    move |ctx: &mut BuilderContext| {
        ctx.wrap_block("blockquote", |inner_ctx| {
            children.into_iter().map(|c| c.build(inner_ctx)).collect()
        })
    }
}

pub fn h1<T: IntoBuilderNode>(children: Vec<T>) -> impl FnOnce(&mut BuilderContext) -> Node {
    move |ctx: &mut BuilderContext| {
        ctx.wrap_block("heading", |inner_ctx| {
            children.into_iter().map(|c| c.build(inner_ctx)).collect()
        })
    }
}

pub fn text(t: &str) -> impl FnOnce(&mut BuilderContext) -> Node + '_ {
    move |ctx: &mut BuilderContext| ctx.parse_text(t, Vec::new())
}

pub fn em_text(t: &str) -> impl FnOnce(&mut BuilderContext) -> Node + '_ {
    move |ctx: &mut BuilderContext| {
        let mark = test_schema().mark_type("em").unwrap().create(None);
        ctx.parse_text(t, vec![mark])
    }
}

pub fn strong_text(t: &str) -> impl FnOnce(&mut BuilderContext) -> Node + '_ {
    move |ctx: &mut BuilderContext| {
        let mark = test_schema().mark_type("strong").unwrap().create(None);
        ctx.parse_text(t, vec![mark])
    }
}

pub fn hr() -> Node {
    test_schema().node("horizontal_rule", None, None, None).unwrap()
}

#[macro_export]
macro_rules! doc {
    ($($child:expr),* $(,)?) => {{
        $crate::builder::build_doc(|ctx| {
            ctx.wrap_block("doc", |inner_ctx| {
                vec![$( $crate::builder::IntoBuilderNode::build($child, inner_ctx) ),*]
            })
        })
    }};
}

#[macro_export]
macro_rules! p {
    ($($child:expr),* $(,)?) => {{
        move |ctx: &mut $crate::builder::BuilderContext| {
            ctx.wrap_block("paragraph", |inner_ctx| {
                vec![$( $crate::builder::IntoBuilderNode::build($child, inner_ctx) ),*]
            })
        }
    }};
}

#[macro_export]
macro_rules! blockquote {
    ($($child:expr),* $(,)?) => {{
        move |ctx: &mut $crate::builder::BuilderContext| {
            ctx.wrap_block("blockquote", |inner_ctx| {
                vec![$( $crate::builder::IntoBuilderNode::build($child, inner_ctx) ),*]
            })
        }
    }};
}

#[macro_export]
macro_rules! h1 {
    ($($child:expr),* $(,)?) => {{
        move |ctx: &mut $crate::builder::BuilderContext| {
            ctx.wrap_block("heading", |inner_ctx| {
                vec![$( $crate::builder::IntoBuilderNode::build($child, inner_ctx) ),*]
            })
        }
    }};
}
