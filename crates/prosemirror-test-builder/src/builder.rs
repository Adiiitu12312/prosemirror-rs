use std::collections::HashMap;
use prosemirror_model::{Fragment, Mark, Node};
use crate::schema::test_schema;

#[derive(Clone, Debug, PartialEq)]
pub struct TaggedDoc {
    pub node: Node,
    pub tags: HashMap<String, usize>,
}

pub struct BuilderContext {
    pub tags: HashMap<String, usize>,
    pub cur_pos: usize,
}

impl BuilderContext {
    pub fn new() -> Self {
        Self {
            tags: HashMap::new(),
            cur_pos: 0,
        }
    }

    pub fn parse_text(&mut self, text: &str, marks: Vec<Mark>) -> Node {
        let mut clean_text = String::new();
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '<' {
                let mut tag_name = String::new();
                let mut is_tag = false;
                for inner_ch in chars.by_ref() {
                    if inner_ch == '>' {
                        is_tag = true;
                        break;
                    }
                    tag_name.push(inner_ch);
                }
                if is_tag && !tag_name.is_empty() {
                    self.tags.insert(tag_name, self.cur_pos);
                    continue;
                }
            }
            clean_text.push(ch);
            self.cur_pos += 1;
        }

        Node::new_text(clean_text, marks)
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

        let children = f(self);
        let fragment = Fragment::from_nodes(children);

        if !is_doc {
            self.cur_pos += 1;
        }

        test_schema().node(type_name, None, Some(fragment), None).unwrap()
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

pub fn p(children: Vec<Node>) -> impl FnOnce(&mut BuilderContext) -> Node {
    move |ctx: &mut BuilderContext| {
        ctx.wrap_block("paragraph", |_| children)
    }
}

pub fn blockquote(children: Vec<Node>) -> impl FnOnce(&mut BuilderContext) -> Node {
    move |ctx: &mut BuilderContext| {
        ctx.wrap_block("blockquote", |_| children)
    }
}

pub fn h1(children: Vec<Node>) -> impl FnOnce(&mut BuilderContext) -> Node {
    move |ctx: &mut BuilderContext| {
        ctx.wrap_block("heading", |_| children)
    }
}

pub fn text(t: &str) -> Node {
    let mut ctx = BuilderContext::new();
    ctx.parse_text(t, Vec::new())
}

pub fn em_text(t: &str) -> Node {
    let mut ctx = BuilderContext::new();
    let mark = test_schema().mark_type("em").unwrap().create(None);
    ctx.parse_text(t, vec![mark])
}

pub fn strong_text(t: &str) -> Node {
    let mut ctx = BuilderContext::new();
    let mark = test_schema().mark_type("strong").unwrap().create(None);
    ctx.parse_text(t, vec![mark])
}

pub fn hr() -> Node {
    test_schema().node("horizontal_rule", None, None, None).unwrap()
}

#[macro_export]
macro_rules! doc {
    ($($child:expr),* $(,)?) => {{
        $crate::builder::build_doc(|ctx| {
            ctx.wrap_block("doc", |inner_ctx| {
                vec![$( $child(inner_ctx) ),*]
            })
        })
    }};
}
