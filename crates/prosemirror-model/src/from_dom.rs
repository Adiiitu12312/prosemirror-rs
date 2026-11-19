use std::borrow::Cow;
use std::ops::Range;

use serde_json::Value;

use crate::{
    ContentMatch, DomElement, DomNode, Fragment, Mark, ModelError, Node, NodeType,
    ResolvedPos, Schema, Slice,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Whitespace {
    #[default]
    Collapse,
    Preserve,
    Full,
}

#[derive(Clone, Debug)]
pub enum DomRuleKind {
    Tag { selector: String, namespace: Option<String> },
    Style(String),
}

#[derive(Clone, Debug)]
pub enum ParseAttrs {
    Accept(Option<Value>),
    Reject,
}

pub type ParseContent = fn(&DomElement, &Schema) -> Fragment;
pub type ContentElementPath = fn(&DomElement) -> Option<Vec<usize>>;

#[derive(Clone, Debug)]
pub struct DomParseRule {
    pub kind: DomRuleKind,
    pub priority: i32,
    pub consuming: bool,
    pub context: Option<String>,
    pub node: Option<String>,
    pub mark: Option<String>,
    pub ignore: bool,
    pub close_parent: bool,
    pub skip: bool,
    pub attrs: Option<Value>,
    pub get_tag_attrs: Option<fn(&DomElement) -> ParseAttrs>,
    pub get_style_attrs: Option<fn(&str) -> ParseAttrs>,
    pub clear_mark: Option<fn(&Mark) -> bool>,
    pub content_element: Option<String>,
    pub content_element_path: Option<ContentElementPath>,
    pub get_content: Option<ParseContent>,
    pub preserve_whitespace: Option<Whitespace>,
}

impl DomParseRule {
    fn new(kind: DomRuleKind) -> Self {
        Self {
            kind,
            priority: 50,
            consuming: true,
            context: None,
            node: None,
            mark: None,
            ignore: false,
            close_parent: false,
            skip: false,
            attrs: None,
            get_tag_attrs: None,
            get_style_attrs: None,
            clear_mark: None,
            content_element: None,
            content_element_path: None,
            get_content: None,
            preserve_whitespace: None,
        }
    }

    pub fn tag(selector: impl Into<String>) -> Self {
        Self::new(DomRuleKind::Tag { selector: selector.into(), namespace: None })
    }

    pub fn style(style: impl Into<String>) -> Self {
        Self::new(DomRuleKind::Style(style.into()))
    }

    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        if let DomRuleKind::Tag { namespace: value, .. } = &mut self.kind {
            *value = Some(namespace.into());
        }
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomPosition {
    /// Child indices from the DOM container to the requested element or text node.
    pub path: Vec<usize>,
    pub offset: usize,
    pub pos: Option<usize>,
}

#[derive(Default)]
pub struct DomParseOptions<'a> {
    pub preserve_whitespace: Option<Whitespace>,
    pub find_positions: Option<&'a mut [DomPosition]>,
    pub from: Option<usize>,
    pub to: Option<usize>,
    pub top_node: Option<&'a Node>,
    pub top_match: Option<&'a ContentMatch>,
    pub context: Option<&'a ResolvedPos>,
    pub rule_from_node: Option<fn(&DomElement) -> Option<DomParseRule>>,
    pub top_open: bool,
}

#[derive(Clone, Debug)]
pub struct DomParser {
    pub schema: Schema,
    rules: Vec<DomParseRule>,
    normalize_lists: bool,
    styles: Vec<String>,
}

impl DomParser {
    pub fn new(schema: Schema, rules: Vec<DomParseRule>) -> Self {
        let mut styles = Vec::new();
        let mut normalize_lists = true;
        for rule in &rules {
            match &rule.kind {
                DomRuleKind::Style(style) => {
                    let property = style.split_once('=').map_or(style.as_str(), |(name, _)| name);
                    if !styles.iter().any(|known| known == property) {
                        styles.push(property.to_owned());
                    }
                }
                DomRuleKind::Tag { selector, .. } => {
                    let tag = selector.split(|ch: char| !ch.is_ascii_alphanumeric()).next().unwrap_or_default();
                    if is_list(tag) && rule.node.as_deref()
                        .and_then(|name| schema.node_type(name))
                        .is_some_and(|node| node.content_match().match_type(&node.name).is_some())
                    {
                        normalize_lists = false;
                    }
                }
            }
        }
        Self { schema, rules, normalize_lists, styles }
    }

    pub fn schema_rules(schema: &Schema) -> Vec<DomParseRule> {
        let mut rules = Vec::new();
        for kind in schema.mark_types() {
            for mut rule in kind.spec.parse_dom.clone() {
                if rule.mark.is_none() && !rule.ignore && rule.clear_mark.is_none() {
                    rule.mark = Some(kind.name.clone());
                }
                rules.push(rule);
            }
        }
        for kind in schema.node_types() {
            for mut rule in kind.spec.parse_dom.clone() {
                if rule.node.is_none() && !rule.ignore && rule.mark.is_none() {
                    rule.node = Some(kind.name.clone());
                }
                rules.push(rule);
            }
        }
        rules.sort_by_key(|rule| std::cmp::Reverse(rule.priority));
        rules
    }

    pub fn from_schema(schema: &Schema) -> Self {
        Self::new(schema.clone(), Self::schema_rules(schema))
    }

    pub fn rules(&self) -> &[DomParseRule] {
        &self.rules
    }

    pub fn parse(&self, dom: &DomNode, options: DomParseOptions<'_>) -> Result<Node, ModelError> {
        let mut context = ParseContext::new(self, options, false)?;
        context.parse_dom(dom)?;
        let top = context.finish()?;
        Ok(top.0.expect("document parsing always creates a top node"))
    }

    pub fn parse_slice(&self, dom: &DomNode, options: DomParseOptions<'_>) -> Result<Slice, ModelError> {
        let mut context = ParseContext::new(self, options, true)?;
        context.parse_dom(dom)?;
        let (node, content) = context.finish()?;
        Ok(Slice::max_open(node.map_or(content, |top| top.content().clone()), true))
    }
}

struct NodeContext {
    id: usize,
    type_name: Option<String>,
    attrs: Option<Value>,
    marks: Vec<Mark>,
    solid: bool,
    matched: Option<ContentMatch>,
    whitespace: Whitespace,
    open_left: bool,
    content: Vec<Node>,
}

impl NodeContext {
    fn find_wrapping(&mut self, node: &Node, schema: &Schema) -> Option<Vec<String>> {
        if self.matched.is_none() {
            let Some(name) = self.type_name.as_deref() else {
                return Some(Vec::new());
            };
            let start = schema.node_type(name)?.content_match();
            if let Some(fill) = start.fill_before(schema, &Fragment::from_nodes(vec![node.clone()]), false, 0) {
                self.matched = start.match_fragment(&fill, 0, fill.child_count());
            } else {
                let wrap = start.find_wrapping(schema, node.type_name())?;
                self.matched = Some(start.clone());
                return Some(wrap);
            }
        }
        self.matched.as_ref()?.find_wrapping(schema, node.type_name())
    }

    fn finish(mut self, schema: &Schema, open_end: bool) -> Result<(Option<Node>, Fragment), ModelError> {
        if self.whitespace == Whitespace::Collapse {
            if let Some(last) = self.content.last_mut() {
                if let Some(text) = last.text() {
                    let trimmed = text.trim_end_matches(html_space);
                    if trimmed.is_empty() {
                        self.content.pop();
                    } else if trimmed != text {
                        *last = last.with_text(trimmed.to_owned());
                    }
                }
            }
        }
        let mut content = Fragment::from_nodes(self.content);
        if !open_end {
            if let Some(matched) = self.matched {
                let filler = matched.fill_before(schema, &Fragment::empty(), true, 0)
                    .ok_or_else(|| ModelError::Dom("Could not complete parsed content".to_owned()))?;
                content = content.append(&filler);
            }
        }
        if let Some(name) = self.type_name {
            let kind = schema.node_type(&name).ok_or(ModelError::UnknownNodeType(name))?;
            let node = kind.try_create(self.attrs, Some(content), Some(self.marks))?;
            Ok((Some(node), Fragment::empty()))
        } else {
            Ok((None, content))
        }
    }
}

struct ParseContext<'a, 'b> {
    parser: &'a DomParser,
    options: DomParseOptions<'b>,
    is_open: bool,
    open: usize,
    next_id: usize,
    nodes: Vec<NodeContext>,
    needs_block: bool,
    local_preserve: bool,
}

impl<'a, 'b> ParseContext<'a, 'b> {
    fn new(parser: &'a DomParser, options: DomParseOptions<'b>, is_open: bool) -> Result<Self, ModelError> {
        let type_name = options.top_node.map(|node| node.type_name().to_owned())
            .or_else(|| (!is_open).then(|| parser.schema.top_node_type().name.clone()));
        let matched = match type_name.as_deref() {
            Some(name) => {
                let kind = parser.schema.node_type(name).ok_or_else(|| ModelError::UnknownNodeType(name.to_owned()))?;
                Some(options.top_match.unwrap_or(kind.content_match()).clone())
            }
            None => None,
        };
        let top = NodeContext {
            id: 0,
            type_name,
            attrs: options.top_node.and_then(|node| node.attrs().cloned()),
            marks: Vec::new(),
            solid: true,
            matched,
            whitespace: options.preserve_whitespace.unwrap_or_default(),
            open_left: is_open,
            content: Vec::new(),
        };
        Ok(Self {
            parser, options, is_open, open: 0, next_id: 1, nodes: vec![top],
            needs_block: false, local_preserve: false,
        })
    }

    fn parse_dom(&mut self, dom: &DomNode) -> Result<(), ModelError> {
        let start = self.options.from.unwrap_or(0);
        let end = self.options.to.unwrap_or(dom.children().len());
        if start > end || end > dom.children().len() {
            return Err(ModelError::Dom("DOM child range is out of bounds".to_owned()));
        }
        self.add_children(dom.children(), dom.as_element().map(|el| el.name.as_str()), &[], &[], start..end)
    }

    fn top(&self) -> &NodeContext {
        &self.nodes[self.open]
    }

    fn top_type(&self) -> Option<&NodeType> {
        self.top().type_name.as_deref().and_then(|name| self.parser.schema.node_type(name))
    }

    fn add_children(
        &mut self, children: &[DomNode], parent_name: Option<&str>, path: &[usize], marks: &[Mark],
        range: Range<usize>,
    ) -> Result<(), ModelError> {
        let mut child_path = path.to_vec();
        let end = range.end;
        for index in range {
            self.find_at_point(path, index)?;
            child_path.push(index);
            let previous = index.checked_sub(1)
                .and_then(|index| children[index].as_element())
                .map(|element| element.name.as_str());
            match &children[index] {
                DomNode::Text(text) => self.add_text(text, &child_path, parent_name, previous, marks)?,
                DomNode::Element(element) => self.add_element(element, &child_path, marks, None)?,
                DomNode::Fragment(fragment) => self.add_children(fragment, None, &child_path, marks, 0..fragment.len())?,
                DomNode::Comment(_) => (),
            }
            child_path.pop();
        }
        self.find_at_point(path, end)
    }

    fn add_text(
        &mut self, text: &str, path: &[usize], parent: Option<&str>, previous: Option<&str>, marks: &[Mark],
    ) -> Result<(), ModelError> {
        let whitespace = match (self.top().whitespace, self.local_preserve) {
            (Whitespace::Collapse, true) => Whitespace::Preserve,
            (mode, _) => mode,
        };
        let inline_context = self.top_type().map_or_else(
            || self.top().content.first().map_or_else(|| parent.is_some_and(|tag| !is_block(tag)), Node::is_inline),
            NodeType::inline_content,
        );
        if whitespace == Whitespace::Full || inline_context || text.chars().any(|ch| !html_space(ch)) {
            let value = match whitespace {
                Whitespace::Full => text.replace("\r\n", "\n").replace('\r', "\n"),
                Whitespace::Preserve => text.replace("\r\n", " ").replace(['\r', '\n'], " "),
                Whitespace::Collapse => {
                    let mut value = collapse_whitespace(text);
                    if self.open == self.nodes.len() - 1 && value.starts_with(' ') {
                        let before = self.top().content.last();
                        if before.is_none() || previous == Some("br")
                            || before.and_then(Node::text).is_some_and(|text| text.ends_with(html_space))
                        {
                            value.remove(0);
                        }
                    }
                    value
                }
            };
            if !value.is_empty() {
                let node = self.parser.schema.try_text(&value, None)?;
                self.insert_node(node, marks)?;
            }
            self.find_in_text(path, text)
        } else {
            Ok(())
        }
    }

    fn add_element(
        &mut self, element: &DomElement, path: &[usize], marks: &[Mark], after: Option<usize>,
    ) -> Result<(), ModelError> {
        let preserve = self.local_preserve;
        if element.name.eq_ignore_ascii_case("pre") || element.style("white-space").is_some_and(|style| style.contains("pre")) {
            self.local_preserve = true;
        }
        let result = self.add_element_inner(element, path, marks, after);
        self.local_preserve = preserve;
        result
    }

    fn add_element_inner(
        &mut self, element: &DomElement, path: &[usize], marks: &[Mark], after: Option<usize>,
    ) -> Result<(), ModelError> {
        let name = element.name.to_ascii_lowercase();
        let normalized = if self.parser.normalize_lists && is_list(&name) {
            normalize_list(element)
        } else {
            Cow::Borrowed(element)
        };
        let element = normalized.as_ref();
        let matched = if after.is_none() {
            self.options.rule_from_node.and_then(|rule| rule(element)).map(|rule| (None, rule))
        } else {
            None
        }.or_else(|| self.match_tag(element, after));

        let rule = matched.as_ref().map(|(_, rule)| rule);
        if rule.map_or_else(|| is_ignored(&name), |rule| rule.ignore) {
            self.find_inside(path)?;
            if name == "br" && !self.top_type().is_some_and(NodeType::inline_content) {
                let text = self.parser.schema.try_text("-", None)?;
                self.find_place(&text, marks)?;
            }
            return Ok(());
        }

        if rule.is_none_or(|rule| rule.skip || rule.close_parent) {
            if rule.is_some_and(|rule| rule.close_parent) {
                self.open = self.open.saturating_sub(1);
            }
            let old_needs_block = self.needs_block;
            let sync = if is_block(&name) {
                if self.open > 0 && self.top().content.first().is_some_and(Node::is_inline) {
                    self.open -= 1;
                }
                if self.top().type_name.is_none() {
                    self.needs_block = true;
                }
                Some(self.top().id)
            } else if element.children.is_empty() {
                return self.leaf_fallback(&name, path, marks);
            } else {
                None
            };
            let inner = if rule.is_some_and(|rule| rule.skip) {
                Some(marks.to_vec())
            } else {
                self.read_styles(element, marks)?
            };
            if let Some(inner) = inner {
                self.add_children(&element.children, Some(&element.name), path, &inner, 0..element.children.len())?;
            }
            if let Some(id) = sync {
                self.sync(id);
            }
            self.needs_block = old_needs_block;
        } else if let Some((index, rule)) = matched {
            if let Some(inner) = self.read_styles(element, marks)? {
                self.apply_rule(element, path, rule, inner, index)?;
            }
        }
        Ok(())
    }

    fn match_tag(&self, element: &DomElement, after: Option<usize>) -> Option<(Option<usize>, DomParseRule)> {
        for (index, rule) in self.parser.rules.iter().enumerate().skip(after.map_or(0, |index| index + 1)) {
            let DomRuleKind::Tag { selector, namespace } = &rule.kind else { continue };
            if !element.matches(selector) || namespace.as_ref().is_some_and(|namespace| Some(namespace) != element.namespace.as_ref())
                || !self.rule_context(rule)
            {
                continue;
            }
            let mut rule = rule.clone();
            if let Some(get_attrs) = rule.get_tag_attrs {
                match get_attrs(element) {
                    ParseAttrs::Reject => continue,
                    ParseAttrs::Accept(attrs) => rule.attrs = attrs,
                }
            }
            return Some((Some(index), rule));
        }
        None
    }

    fn rule_context(&self, rule: &DomParseRule) -> bool {
        rule.context.as_deref().is_none_or(|context| self.matches_context(context))
    }

    fn read_styles(&self, element: &DomElement, marks: &[Mark]) -> Result<Option<Vec<Mark>>, ModelError> {
        let mut marks = marks.to_vec();
        for property in &self.parser.styles {
            let Some(value) = element.style(property) else { continue };
            for rule in &self.parser.rules {
                let DomRuleKind::Style(style) = &rule.kind else { continue };
                let (name, exact) = style.split_once('=').map_or((style.as_str(), None), |(name, value)| (name, Some(value)));
                if name != property || exact.is_some_and(|exact| exact != value) || !self.rule_context(rule) {
                    continue;
                }
                let attrs = if let Some(parse) = rule.get_style_attrs {
                    match parse(value) {
                        ParseAttrs::Reject => continue,
                        ParseAttrs::Accept(attrs) => attrs,
                    }
                } else {
                    rule.attrs.clone()
                };
                if rule.ignore {
                    return Ok(None);
                }
                if let Some(clear) = rule.clear_mark {
                    marks.retain(|mark| !clear(mark));
                } else if let Some(name) = rule.mark.as_deref() {
                    marks.push(self.create_mark(name, attrs)?);
                }
                if rule.consuming {
                    break;
                }
            }
        }
        Ok(Some(marks))
    }

    fn apply_rule(
        &mut self, element: &DomElement, path: &[usize], rule: DomParseRule, mut marks: Vec<Mark>,
        index: Option<usize>,
    ) -> Result<(), ModelError> {
        let mut sync = None;
        if let Some(name) = rule.node.as_deref() {
            let kind = self.parser.schema.node_type(name).ok_or_else(|| ModelError::UnknownNodeType(name.to_owned()))?;
            let node = kind.try_create(rule.attrs.clone(), None, None)?;
            if kind.is_leaf() {
                if !self.insert_node(node, &marks)? {
                    self.leaf_fallback(&element.name, path, &marks)?;
                }
                return self.find_inside(path);
            }
            if let Some(inner) = self.find_place(&node, &marks)? {
                marks = self.enter_inner(name, rule.attrs.clone(), inner, true, rule.preserve_whitespace)?;
                sync = Some(self.top().id);
            }
        } else if let Some(mark) = rule.mark.as_deref() {
            marks.push(self.create_mark(mark, rule.attrs.clone())?);
        }

        if let Some(index) = index.filter(|_| !rule.consuming) {
            self.add_element(element, path, &marks, Some(index))?;
        } else if let Some(get_content) = rule.get_content {
            self.find_inside(path)?;
            for node in get_content(element, &self.parser.schema).children {
                self.insert_node(node, &marks)?;
            }
        } else {
            let inner_path = rule.content_element_path.and_then(|f| f(element))
                .or_else(|| rule.content_element.as_deref().and_then(|selector| element.find_path(selector)));
            let mut content_path = path.to_vec();
            let (content, name) = if let Some(inner_path) = inner_path.filter(|path| !path.is_empty()) {
                let dom = take_at_path(&element.children, &inner_path)
                    .ok_or_else(|| ModelError::Dom("Invalid content element path".to_owned()))?;
                content_path.extend(inner_path);
                (dom.children(), dom.as_element().map(|el| el.name.as_str()))
            } else {
                (element.children.as_slice(), Some(element.name.as_str()))
            };
            self.find_around(path, &content_path, true)?;
            self.add_children(content, name, &content_path, &marks, 0..content.len())?;
            self.find_around(path, &content_path, false)?;
        }
        if let Some(id) = sync {
            if self.sync(id) {
                self.open -= 1;
            }
        }
        Ok(())
    }

    fn leaf_fallback(&mut self, name: &str, path: &[usize], marks: &[Mark]) -> Result<(), ModelError> {
        if name.eq_ignore_ascii_case("br") && self.top_type().is_some_and(NodeType::inline_content) {
            self.add_text("\n", path, None, None, marks)?;
        }
        Ok(())
    }

    fn create_mark(&self, name: &str, attrs: Option<Value>) -> Result<Mark, ModelError> {
        self.parser.schema.mark_type(name)
            .ok_or_else(|| ModelError::UnknownMarkType(name.to_owned()))?
            .try_create(attrs)
    }

    fn find_place(&mut self, node: &Node, marks: &[Mark]) -> Result<Option<Vec<Mark>>, ModelError> {
        let mut route: Option<(Vec<String>, usize)> = None;
        for depth in (0..=self.open).rev() {
            let context = &mut self.nodes[depth];
            if let Some(wrap) = context.find_wrapping(node, &self.parser.schema) {
                let direct = wrap.is_empty();
                if route.as_ref().is_none_or(|(known, _)| known.len() > wrap.len()) {
                    route = Some((wrap, context.id));
                    if direct {
                        break;
                    }
                }
            }
            if context.solid {
                break;
            }
        }
        let Some((route, target)) = route else { return Ok(None) };
        self.sync(target);
        let mut marks = marks.to_vec();
        for wrapper in route {
            marks = self.enter_inner(&wrapper, None, marks, false, None)?;
        }
        Ok(Some(marks))
    }

    fn allows_mark(&self, name: &str, mark: &Mark) -> bool {
        self.top_type().map_or_else(
            || self.parser.schema.node_types().any(|parent| {
                parent.allows_mark_type(&mark.type_name) && parent.content_match().can_match_anywhere(name)
            }),
            |parent| parent.allows_mark_type(&mark.type_name),
        )
    }

    fn insert_node(&mut self, node: Node, marks: &[Mark]) -> Result<bool, ModelError> {
        let mut known_marks = marks.to_vec();
        if node.is_inline() && self.needs_block && self.top().type_name.is_none() {
            if let Some(block) = self.textblock_from_context() {
                known_marks = self.enter_inner(&block, None, known_marks, false, None)?;
            }
        }
        let Some(mut inner) = self.find_place(&node, &known_marks)? else { return Ok(false) };
        self.close_extra(false)?;
        inner.extend(node.marks().iter().cloned());
        let mut node_marks = Vec::new();
        for mark in inner {
            if self.allows_mark(node.type_name(), &mark) {
                node_marks = Mark::add_to_set(&node_marks, mark);
            }
        }
        let top = &mut self.nodes[self.open];
        top.matched = top.matched.as_ref().and_then(|matched| matched.match_type(node.type_name()));
        top.content.push(node.with_marks(node_marks));
        Ok(true)
    }

    fn enter_inner(
        &mut self, name: &str, attrs: Option<Value>, mut marks: Vec<Mark>, solid: bool,
        preserve: Option<Whitespace>,
    ) -> Result<Vec<Mark>, ModelError> {
        let kind = self.parser.schema.node_type(name).cloned().ok_or_else(|| ModelError::UnknownNodeType(name.to_owned()))?;
        self.close_extra(false)?;
        let mut apply_marks = Vec::new();
        marks.retain(|mark| {
            if self.allows_mark(name, mark) {
                apply_marks = Mark::add_to_set(&apply_marks, mark.clone());
                false
            } else {
                true
            }
        });
        let top = &mut self.nodes[self.open];
        top.matched = top.matched.as_ref().and_then(|matched| matched.match_type(name));
        let whitespace = preserve.unwrap_or(if kind.whitespace() == "pre" { Whitespace::Full } else { top.whitespace });
        let open_left = top.open_left && top.content.is_empty();
        self.nodes.push(NodeContext {
            id: self.next_id,
            type_name: Some(name.to_owned()),
            attrs,
            marks: apply_marks,
            solid,
            matched: (!open_left).then(|| kind.content_match().clone()),
            whitespace,
            open_left,
            content: Vec::new(),
        });
        self.open += 1;
        self.next_id += 1;
        Ok(marks)
    }

    fn close_extra(&mut self, open_end: bool) -> Result<(), ModelError> {
        while self.nodes.len() > self.open + 1 {
            let context = self.nodes.pop().expect("the stack contains an extra context");
            let (node, _) = context.finish(&self.parser.schema, open_end)?;
            if let Some(node) = node {
                self.nodes.last_mut().expect("the root context is present").content.push(node);
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Result<(Option<Node>, Fragment), ModelError> {
        self.open = 0;
        self.close_extra(self.is_open)?;
        self.nodes.pop().expect("the root context is present")
            .finish(&self.parser.schema, self.is_open || self.options.top_open)
    }

    fn sync(&mut self, id: usize) -> bool {
        for index in (0..=self.open).rev() {
            let context = &mut self.nodes[index];
            if context.id == id {
                self.open = index;
                return true;
            }
            if self.local_preserve && context.whitespace == Whitespace::Collapse {
                context.whitespace = Whitespace::Preserve;
            }
        }
        false
    }

    fn current_pos(&mut self) -> Result<usize, ModelError> {
        self.close_extra(false)?;
        let size: usize = self.nodes.iter().flat_map(|node| &node.content).map(Node::node_size).sum();
        Ok(size + self.open)
    }

    fn update_positions(
        &mut self, matches: impl Fn(&DomPosition) -> bool, project: impl Fn(&DomPosition, usize) -> usize,
    ) -> Result<(), ModelError> {
        if !self.options.find_positions.as_deref().is_some_and(|points| points.iter().any(&matches)) {
            return Ok(());
        }
        let current = self.current_pos()?;
        if let Some(points) = self.options.find_positions.as_deref_mut() {
            for point in points.iter_mut().filter(|point| matches(point)) {
                point.pos = Some(project(point, current));
            }
        }
        Ok(())
    }

    fn find_at_point(&mut self, path: &[usize], index: usize) -> Result<(), ModelError> {
        self.update_positions(|point| point.path == path && point.offset == index, |_, current| current)
    }

    fn find_inside(&mut self, path: &[usize]) -> Result<(), ModelError> {
        self.update_positions(|point| point.pos.is_none() && point.path.starts_with(path), |_, current| current)
    }

    fn find_around(&mut self, path: &[usize], content_path: &[usize], before: bool) -> Result<(), ModelError> {
        if path == content_path {
            return Ok(());
        }
        self.update_positions(|point| {
            point.pos.is_none() && point.path.starts_with(path) && !point.path.starts_with(content_path)
                && if before { point.path.as_slice() < content_path } else { point.path.as_slice() > content_path }
        }, |_, current| current)
    }

    fn find_in_text(&mut self, path: &[usize], original: &str) -> Result<(), ModelError> {
        let length = original.encode_utf16().count();
        self.update_positions(|point| point.path == path, |point, current| current.saturating_sub(length.saturating_sub(point.offset)))
    }

    fn matches_context(&self, context: &str) -> bool {
        let external = self.options.context;
        let use_root = !self.is_open && external.is_none_or(|pos| {
            self.nodes[0].type_name.as_deref() == Some(pos.parent().type_name())
        });
        let mut ancestors = Vec::new();
        if let Some(external) = external {
            ancestors.extend(external.path.iter().map(|entry| entry.node.type_name()));
        }
        for node in self.nodes[..=self.open].iter().skip(usize::from(!use_root)) {
            if let Some(name) = node.type_name.as_deref() {
                ancestors.push(name);
            }
        }
        context.split('|').any(|branch| {
            let mut path: Vec<_> = branch.trim().split('/').collect();
            if path.last() == Some(&"") {
                path.pop();
            }
            if path.first() == Some(&"") {
                path.remove(0);
            }
            match_ancestors(&ancestors, &path, &self.parser.schema)
        })
    }

    fn textblock_from_context(&self) -> Option<String> {
        if let Some(context) = self.options.context {
            for depth in (0..=context.depth).rev() {
                if let Some(matched) = context.node(depth).content_match_at(context.index_after(depth)) {
                    if let Some(name) = matched.default_type() {
                        if self.parser.schema.node_type(name).is_some_and(NodeType::is_textblock) {
                            return Some(name.to_owned());
                        }
                    }
                }
            }
        }
        self.parser.schema.node_types().find(|kind| kind.is_textblock() && !kind.has_required_attrs())
            .map(|kind| kind.name.clone())
    }
}

fn match_ancestors(ancestors: &[&str], path: &[&str], schema: &Schema) -> bool {
    let Some((part, prefix)) = path.split_last() else { return true };
    if part.is_empty() {
        return (0..=ancestors.len()).rev().any(|len| match_ancestors(&ancestors[..len], prefix, schema));
    }
    let Some((kind, parents)) = ancestors.split_last() else { return false };
    (kind == part || schema.node_type(kind).is_some_and(|kind| kind.is_in_group(part)))
        && match_ancestors(parents, prefix, schema)
}

fn take_at_path<'a>(children: &'a [DomNode], path: &[usize]) -> Option<&'a DomNode> {
    let (&index, remainder) = path.split_first()?;
    let mut dom = children.get(index)?;
    for &index in remainder {
        dom = dom.children().get(index)?;
    }
    Some(dom)
}

fn html_space(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\r' | '\n' | '\u{000c}')
}

fn collapse_whitespace(text: &str) -> String {
    let mut result = String::new();
    let mut was_space = false;
    for ch in text.chars() {
        let space = html_space(ch);
        if !space {
            result.push(ch);
        } else if !was_space {
            result.push(' ');
        }
        was_space = space;
    }
    result
}

fn is_list(tag: &str) -> bool {
    matches!(tag, "ol" | "ul")
}

fn normalize_list(element: &DomElement) -> Cow<'_, DomElement> {
    let mut previous_is_item = false;
    let malformed = element.children.iter().filter_map(DomNode::as_element).any(|child| {
        if previous_is_item && is_list(&child.name) {
            return true;
        }
        previous_is_item = child.name == "li";
        false
    });
    if !malformed {
        return Cow::Borrowed(element);
    }
    let mut result = element.clone();
    let mut children: Vec<DomNode> = Vec::with_capacity(result.children.len());
    let mut previous_item: Option<usize> = None;
    for child in std::mem::take(&mut result.children) {
        if let Some(name) = child.as_element().map(|element| element.name.to_ascii_lowercase()) {
            if is_list(&name) {
                if let Some(previous) = previous_item {
                    children[previous].children_mut().expect("list item has children").push(child);
                    continue;
                }
            }
            previous_item = (name == "li").then_some(children.len());
        }
        children.push(child);
    }
    result.children = children;
    Cow::Owned(result)
}

fn is_block(tag: &str) -> bool {
    matches!(tag,
        "address" | "article" | "aside" | "blockquote" | "canvas" | "dd" | "div" | "dl"
        | "fieldset" | "figcaption" | "figure" | "footer" | "form" | "h1" | "h2" | "h3" | "h4"
        | "h5" | "h6" | "header" | "hgroup" | "hr" | "li" | "noscript" | "ol" | "output"
        | "p" | "pre" | "section" | "table" | "tfoot" | "ul"
    )
}

fn is_ignored(tag: &str) -> bool {
    matches!(tag, "head" | "noscript" | "object" | "script" | "style" | "title")
}
