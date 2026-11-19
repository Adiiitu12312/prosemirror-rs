use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};

use serde_json::{Map, Value};

use crate::{ContentMatch, DomParseRule, Fragment, Mark, MarkToDom, ModelError, Node, NodeToDom, Slice};

#[derive(Clone, Debug, Default)]
pub struct AttributeSpec {
    pub default: Option<Value>,
    pub validate: Option<String>,
}

impl AttributeSpec {
    pub fn new(default: Option<Value>, validate: Option<&str>) -> Self {
        Self {
            default,
            validate: validate.map(str::to_owned),
        }
    }

    pub fn with_default(default: Value) -> Self {
        Self::new(Some(default), None)
    }

    pub fn required() -> Self {
        Self::default()
    }
}

#[derive(Clone, Debug)]
pub struct NodeSpec {
    pub content: Option<String>,
    pub marks: Option<String>,
    pub group: Option<String>,
    pub attrs: HashMap<String, AttributeSpec>,
    pub inline: bool,
    pub atom: bool,
    pub selectable: bool,
    pub draggable: bool,
    pub code: bool,
    pub whitespace: Option<String>,
    pub defining: bool,
    pub defining_as_context: bool,
    pub defining_for_content: bool,
    pub isolating: bool,
    pub linebreak_replacement: bool,
    pub leaf_text: Option<fn(&Node) -> String>,
    pub to_debug_string: Option<fn(&Node) -> String>,
    pub parse_dom: Vec<DomParseRule>,
    pub to_dom: Option<NodeToDom>,
}

impl Default for NodeSpec {
    fn default() -> Self {
        Self {
            content: None,
            marks: None,
            group: None,
            attrs: HashMap::new(),
            inline: false,
            atom: false,
            selectable: true,
            draggable: false,
            code: false,
            whitespace: None,
            defining: false,
            defining_as_context: false,
            defining_for_content: false,
            isolating: false,
            linebreak_replacement: false,
            leaf_text: None,
            to_debug_string: None,
            parse_dom: Vec::new(),
            to_dom: None,
        }
    }
}

impl NodeSpec {
    pub(crate) fn has_required_attrs(&self) -> bool {
        self.attrs.values().any(|spec| spec.default.is_none())
    }
}

#[derive(Clone, Debug)]
pub struct MarkSpec {
    /// Default values for legacy attribute declarations.
    pub attrs: Option<HashMap<String, Value>>,
    pub attributes: HashMap<String, AttributeSpec>,
    pub inclusive: bool,
    pub code: bool,
    pub excludes: Option<String>,
    pub group: Option<String>,
    pub spanning: bool,
    pub parse_dom: Vec<DomParseRule>,
    pub to_dom: Option<MarkToDom>,
}

impl Default for MarkSpec {
    fn default() -> Self {
        Self {
            attrs: None,
            attributes: HashMap::new(),
            inclusive: true,
            code: false,
            excludes: None,
            group: None,
            spanning: true,
            parse_dom: Vec::new(),
            to_dom: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NodeType {
    pub name: String,
    pub spec: NodeSpec,
    content_match: ContentMatch,
    allowed_marks: Option<HashSet<String>>,
    schema: Weak<SchemaInner>,
}

impl NodeType {
    pub fn new(name: impl Into<String>, spec: NodeSpec) -> Self {
        Self {
            name: name.into(),
            spec,
            content_match: ContentMatch::empty(),
            allowed_marks: None,
            schema: Weak::new(),
        }
    }

    pub fn create(
        &self,
        attrs: Option<Value>,
        content: Option<Fragment>,
        marks: Option<Vec<Mark>>,
    ) -> Node {
        self.try_create(attrs, content, marks)
            .expect("invalid node attributes or type")
    }

    pub fn try_create(
        &self,
        attrs: Option<Value>,
        content: Option<Fragment>,
        marks: Option<Vec<Mark>>,
    ) -> Result<Node, ModelError> {
        if self.is_text() {
            return Err(ModelError::InvalidContent(self.name.clone()));
        }
        let schema = self.schema.upgrade().map(Schema::from_inner);
        let attrs = compute_attrs(&self.spec.attrs, attrs.as_ref(), &self.name)?;
        let marks = marks.unwrap_or_default();
        let marks = if let Some(schema) = schema.as_ref() {
            schema.bind_marks(marks)?
        } else {
            Mark::set_from(marks)
        };
        Ok(Node::new_in_schema(
            self.name.clone(),
            attrs,
            content.unwrap_or_default(),
            marks,
            schema,
        ))
    }

    pub fn create_checked(
        &self,
        attrs: Option<Value>,
        content: Option<Fragment>,
        marks: Option<Vec<Mark>>,
    ) -> Result<Node, ModelError> {
        let content = content.unwrap_or_default();
        self.check_content(&content)?;
        self.try_create(attrs, Some(content), marks)
    }

    pub fn create_and_fill(
        &self,
        attrs: Option<Value>,
        content: Option<Fragment>,
        marks: Option<Vec<Mark>>,
    ) -> Result<Option<Node>, ModelError> {
        self.create_and_fill_with_stack(attrs, content, marks, &mut Vec::new())
    }

    pub(crate) fn create_and_fill_with_stack(
        &self,
        attrs: Option<Value>,
        content: Option<Fragment>,
        marks: Option<Vec<Mark>>,
        generating: &mut Vec<String>,
    ) -> Result<Option<Node>, ModelError> {
        let Some(schema) = self.schema.upgrade().map(Schema::from_inner) else {
            return self.try_create(attrs, content, marks).map(Some);
        };
        if generating.contains(&self.name) {
            return Ok(None);
        }
        let mut content = content.unwrap_or_default();
        generating.push(self.name.clone());
        let filled = (|| {
            if content.size() != 0 {
                let prefix = self.content_match.fill_before_with_stack(
                    &schema, &content, false, 0, generating,
                )?;
                content = prefix.append(&content);
            }
            let matched = self.content_match.match_fragment(&content, 0, content.child_count())?;
            let suffix = matched.fill_before_with_stack(
                &schema, &Fragment::empty(), true, 0, generating,
            )?;
            Some(content.append(&suffix))
        })();
        generating.pop();
        filled.map(|fragment| self.try_create(attrs, Some(fragment), marks)).transpose()
    }

    pub fn content_match(&self) -> &ContentMatch {
        &self.content_match
    }

    pub fn has_required_attrs(&self) -> bool {
        self.spec.has_required_attrs()
    }

    pub fn compatible_content(&self, other: &Self) -> bool {
        self.name == other.name || self.content_match.compatible(&other.content_match)
    }

    pub fn valid_content(&self, content: &Fragment) -> bool {
        self.content_match
            .match_fragment(content, 0, content.child_count())
            .is_some_and(|matched| matched.valid_end())
            && content.children.iter().all(|child| self.allows_marks(child.marks()))
    }

    pub fn check_content(&self, content: &Fragment) -> Result<(), ModelError> {
        if self.valid_content(content) {
            Ok(())
        } else {
            Err(ModelError::InvalidContent(self.name.clone()))
        }
    }

    pub fn allows_mark_type(&self, name: &str) -> bool {
        self.allowed_marks.as_ref().is_none_or(|allowed| allowed.contains(name))
    }

    pub fn allows_marks(&self, marks: &[Mark]) -> bool {
        marks.iter().all(|mark| self.allows_mark_type(&mark.type_name))
    }

    pub fn allowed_marks(&self, marks: &[Mark]) -> Vec<Mark> {
        marks.iter().filter(|mark| self.allows_mark_type(&mark.type_name)).cloned().collect()
    }

    pub fn is_block(&self) -> bool {
        !self.is_inline()
    }

    pub fn is_inline(&self) -> bool {
        self.spec.inline || self.is_text()
    }

    pub fn is_text(&self) -> bool {
        self.name == "text"
    }

    pub fn inline_content(&self) -> bool {
        self.content_match.inline_content()
    }

    pub fn is_textblock(&self) -> bool {
        self.is_block() && self.inline_content()
    }

    pub fn is_leaf(&self) -> bool {
        self.spec.content.as_deref().is_none_or(|content| content.trim().is_empty())
    }

    pub fn is_atom(&self) -> bool {
        self.is_leaf() || self.spec.atom
    }

    pub fn is_in_group(&self, group: &str) -> bool {
        self.spec.group.as_deref().is_some_and(|groups| {
            groups.split_whitespace().any(|name| name == group)
        })
    }

    pub fn whitespace(&self) -> &str {
        self.spec.whitespace.as_deref().unwrap_or(if self.spec.code { "pre" } else { "normal" })
    }
}

#[derive(Clone, Debug)]
pub struct MarkType {
    pub name: String,
    pub spec: MarkSpec,
    pub(crate) rank: usize,
    pub(crate) excluded: HashSet<String>,
    attributes: HashMap<String, AttributeSpec>,
}

impl MarkType {
    pub fn new(name: impl Into<String>, spec: MarkSpec) -> Self {
        let name = name.into();
        let mut attributes = spec.attrs.iter().flatten()
            .map(|(name, value)| (name.clone(), AttributeSpec::with_default(value.clone())))
            .collect::<HashMap<_, _>>();
        attributes.extend(spec.attributes.clone());
        let excluded = spec.excludes.as_deref().unwrap_or(&name)
            .split_whitespace().map(str::to_owned).collect();
        Self { name, spec, rank: 0, excluded, attributes }
    }

    pub fn create(&self, attrs: Option<Value>) -> Mark {
        self.try_create(attrs).expect("invalid mark attributes")
    }

    pub fn try_create(&self, attrs: Option<Value>) -> Result<Mark, ModelError> {
        let attrs = compute_attrs(&self.attributes, attrs.as_ref(), &self.name)?;
        Ok(Mark::from_type(self, attrs))
    }

    pub fn excludes(&self, other: &str) -> bool {
        self.excluded.contains(other) || self.excluded.contains("_")
    }

    pub fn is_in_set<'a>(&self, marks: &'a [Mark]) -> Option<&'a Mark> {
        marks.iter().find(|mark| mark.type_name == self.name)
    }

    pub fn remove_from_set(&self, marks: &[Mark]) -> Vec<Mark> {
        Mark::remove_from_set(marks, &self.name)
    }

    pub(crate) fn check_attrs(&self, attrs: Option<&Value>) -> Result<(), ModelError> {
        check_attrs(&self.attributes, attrs, &self.name)
    }
}

#[derive(Clone, Debug)]
pub struct Schema {
    inner: Arc<SchemaInner>,
}

#[derive(Debug)]
struct SchemaInner {
    nodes: HashMap<String, NodeType>,
    marks: HashMap<String, MarkType>,
    node_order: Vec<String>,
    mark_order: Vec<String>,
    top_node_type: String,
    linebreak_replacement: Option<String>,
}

impl Schema {
    pub fn new(mut nodes: Vec<(String, NodeSpec)>, marks: Vec<(String, MarkSpec)>) -> Self {
        // Retain the original constructor's defaults. The fallible constructor
        // accepts the explicit schema contract, including the top node name.
        if !nodes.iter().any(|(name, _)| name == "text") {
            nodes.push(("text".to_owned(), NodeSpec {
                inline: true,
                group: Some("inline".to_owned()),
                ..NodeSpec::default()
            }));
        }
        let top = nodes.iter().find(|(name, _)| name == "doc")
            .or_else(|| nodes.first())
            .map(|(name, _)| name.clone())
            .expect("the schema must contain a top node");
        Self::try_new(nodes, marks, Some(&top)).expect("invalid schema")
    }

    pub fn try_new(
        nodes: Vec<(String, NodeSpec)>,
        marks: Vec<(String, MarkSpec)>,
        top_node: Option<&str>,
    ) -> Result<Self, ModelError> {
        let top_node = top_node.unwrap_or("doc");
        if !nodes.iter().any(|(name, _)| name == top_node) {
            return Err(ModelError::InvalidSchema(format!("Missing top node type '{top_node}'")));
        }
        let Some((_, text)) = nodes.iter().find(|(name, _)| name == "text") else {
            return Err(ModelError::InvalidSchema("Every schema needs a 'text' type".to_owned()));
        };
        if !text.attrs.is_empty() {
            return Err(ModelError::InvalidSchema("The text node type should not have attributes".to_owned()));
        }
        let mut mark_types = HashMap::new();
        for (rank, (name, spec)) in marks.iter().enumerate() {
            if nodes.iter().any(|(node, _)| node == name) {
                return Err(ModelError::InvalidSchema(format!("{name} is both a node and a mark")));
            }
            let mut mark_type = MarkType::new(name, spec.clone());
            if let Some(excluded) = spec.excludes.as_deref() {
                mark_type.excluded = gather_marks(excluded, &marks)?;
            }
            mark_type.rank = rank;
            mark_types.insert(name.clone(), mark_type);
        }

        let mut content_matches = HashMap::<&str, ContentMatch>::new();
        let mut node_types = HashMap::new();
        let mut linebreak_replacement = None;
        for (name, spec) in &nodes {
            let expression = spec.content.as_deref().unwrap_or_default();
            let mut node_type = NodeType::new(name, spec.clone());
            node_type.content_match = if let Some(matched) = content_matches.get(expression) {
                matched.clone()
            } else {
                let matched = ContentMatch::parse(expression, &nodes)?;
                content_matches.insert(expression, matched.clone());
                matched
            };
            node_type.allowed_marks = match spec.marks.as_deref() {
                Some("_") => None,
                Some(expr) => Some(gather_marks(expr, &marks)?),
                None if node_type.inline_content() => None,
                None => Some(HashSet::new()),
            };
            if spec.linebreak_replacement {
                if linebreak_replacement.is_some() || !node_type.is_inline() || !node_type.is_leaf() {
                    return Err(ModelError::InvalidSchema(
                        "Linebreak replacement must be a unique inline leaf node".to_owned(),
                    ));
                }
                linebreak_replacement = Some(name.clone());
            }
            node_types.insert(name.clone(), node_type);
        }

        drop(content_matches);
        Ok(Self::from_inner(Arc::new_cyclic(|schema| {
            for node_type in node_types.values_mut() {
                node_type.schema = schema.clone();
            }
            SchemaInner {
                nodes: node_types,
                marks: mark_types,
                node_order: nodes.into_iter().map(|(name, _)| name).collect(),
                mark_order: marks.into_iter().map(|(name, _)| name).collect(),
                top_node_type: top_node.to_owned(),
                linebreak_replacement,
            }
        })))
    }

    fn from_inner(inner: Arc<SchemaInner>) -> Self {
        Self { inner }
    }

    pub fn node_type(&self, name: &str) -> Option<&NodeType> {
        self.inner.nodes.get(name)
    }

    pub fn mark_type(&self, name: &str) -> Option<&MarkType> {
        self.inner.marks.get(name)
    }

    pub fn node_types(&self) -> impl Iterator<Item = &NodeType> {
        self.inner.node_order.iter().map(|name| &self.inner.nodes[name])
    }

    pub fn mark_types(&self) -> impl Iterator<Item = &MarkType> {
        self.inner.mark_order.iter().map(|name| &self.inner.marks[name])
    }

    pub fn top_node_type(&self) -> &NodeType {
        &self.inner.nodes[&self.inner.top_node_type]
    }

    pub fn linebreak_replacement(&self) -> Option<&NodeType> {
        self.inner.linebreak_replacement.as_deref().and_then(|name| self.node_type(name))
    }

    pub fn node(
        &self,
        type_name: &str,
        attrs: Option<Value>,
        content: Option<Fragment>,
        marks: Option<Vec<Mark>>,
    ) -> Result<Node, ModelError> {
        self.require_node_type(type_name)?.create_checked(attrs, content, marks)
    }

    pub fn text(&self, text: &str, marks: Option<Vec<Mark>>) -> Node {
        self.try_text(text, marks).expect("invalid text node")
    }

    pub fn try_text(&self, text: &str, marks: Option<Vec<Mark>>) -> Result<Node, ModelError> {
        if text.is_empty() {
            return Err(ModelError::InvalidContent("text".to_owned()));
        }
        Ok(Node::new_text_in_schema(text, self.bind_marks(marks.unwrap_or_default())?, Some(self.clone())))
    }

    pub fn node_from_json(&self, value: &Value) -> Result<Node, ModelError> {
        let repr = value.as_object().ok_or_else(|| ModelError::Serialization("Invalid input for Node.fromJSON".to_owned()))?;
        let name = json_type(repr, "node")?;
        let marks = match repr.get("marks").filter(|marks| !marks.is_null()) {
            None => Vec::new(),
            Some(marks) => marks.as_array().ok_or_else(|| ModelError::Serialization("Invalid mark data".to_owned()))?
                .iter().map(|mark| self.mark_from_json(mark)).collect::<Result<_, _>>()?,
        };
        if name == "text" {
            let text = repr.get("text").and_then(Value::as_str).ok_or_else(|| {
                ModelError::Serialization("Invalid text node in JSON".to_owned())
            })?;
            return self.try_text(text, Some(marks));
        }
        let node_type = self.require_node_type(name)?;
        let content = self.fragment_from_json(repr.get("content"))?;
        let node = node_type.try_create(repr.get("attrs").cloned(), Some(content), Some(marks))?;
        check_attrs(&node_type.spec.attrs, node.attrs(), name)?;
        Ok(node)
    }

    pub fn mark_from_json(&self, value: &Value) -> Result<Mark, ModelError> {
        let repr = value.as_object().ok_or_else(|| ModelError::Serialization("Invalid input for Mark.fromJSON".to_owned()))?;
        let name = json_type(repr, "mark")?;
        let mark_type = self.mark_type(name).ok_or_else(|| ModelError::UnknownMarkType(name.to_owned()))?;
        let mark = mark_type.try_create(repr.get("attrs").cloned())?;
        mark_type.check_attrs(mark.attrs.as_ref())?;
        Ok(mark)
    }

    pub fn slice_from_json(&self, value: &Value) -> Result<Slice, ModelError> {
        if value.is_null() {
            return Ok(Slice::empty());
        }
        let repr = value.as_object().ok_or_else(|| ModelError::Serialization("Invalid input for Slice.fromJSON".to_owned()))?;
        let depth = |key| {
            repr.get(key).filter(|value| !value.is_null()).map_or(Ok(0), |value| {
                value.as_u64().and_then(|value| usize::try_from(value).ok())
                    .ok_or_else(|| ModelError::Serialization(format!("Invalid slice {key}")))
            })
        };
        Ok(Slice::new(
            self.fragment_from_json(repr.get("content"))?,
            depth("openStart")?,
            depth("openEnd")?,
        ))
    }

    fn fragment_from_json(&self, value: Option<&Value>) -> Result<Fragment, ModelError> {
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return Ok(Fragment::empty());
        };
        let children = value.as_array().ok_or_else(|| ModelError::Serialization("Invalid input for Fragment.fromJSON".to_owned()))?;
        let children = children.iter().map(|child| self.node_from_json(child)).collect::<Result<_, _>>()?;
        Ok(Fragment::from_children(children))
    }

    pub(crate) fn bind_marks(&self, mut marks: Vec<Mark>) -> Result<Vec<Mark>, ModelError> {
        for mark in &mut marks {
            let mark_type = self.mark_type(&mark.type_name)
                .ok_or_else(|| ModelError::UnknownMarkType(mark.type_name.clone()))?;
            let attrs = compute_attrs(&mark_type.attributes, mark.attrs.as_ref(), &mark.type_name)?;
            *mark = Mark::from_type(mark_type, attrs);
        }
        Ok(Mark::set_from(marks))
    }

    fn require_node_type(&self, name: &str) -> Result<&NodeType, ModelError> {
        self.node_type(name).ok_or_else(|| ModelError::UnknownNodeType(name.to_owned()))
    }
}

fn gather_marks(expr: &str, specs: &[(String, MarkSpec)]) -> Result<HashSet<String>, ModelError> {
    let mut result = HashSet::new();
    for token in expr.split_whitespace() {
        if specs.iter().any(|(name, _)| name == token) {
            result.insert(token.to_owned());
            continue;
        }
        let mut found = token == "_";
        for (name, spec) in specs {
            if token == "_" || spec.group.as_deref().is_some_and(|groups| groups.split_whitespace().any(|group| group == token)) {
                result.insert(name.clone());
                found = true;
            }
        }
        if !found {
            return Err(ModelError::InvalidSchema(format!("Unknown mark type or group '{token}'")));
        }
    }
    Ok(result)
}

fn json_type<'a>(repr: &'a Map<String, Value>, kind: &str) -> Result<&'a str, ModelError> {
    repr.get("type").and_then(Value::as_str)
        .ok_or_else(|| ModelError::Serialization(format!("Invalid {kind} type")))
}

fn compute_attrs(
    specs: &HashMap<String, AttributeSpec>,
    provided: Option<&Value>,
    name: &str,
) -> Result<Option<Value>, ModelError> {
    if specs.is_empty() {
        return Ok(None);
    }
    let mut attrs = Map::new();
    for (attribute, spec) in specs {
        let value = provided.and_then(|values| values.get(attribute)).or(spec.default.as_ref())
            .ok_or_else(|| ModelError::InvalidAttributes(format!("No value supplied for attribute {attribute} on type {name}")))?;
        attrs.insert(attribute.clone(), value.clone());
    }
    Ok(Some(Value::Object(attrs)))
}

pub(crate) fn check_attrs(
    specs: &HashMap<String, AttributeSpec>,
    attrs: Option<&Value>,
    name: &str,
) -> Result<(), ModelError> {
    for (attribute, spec) in specs {
        if let Some(allowed) = spec.validate.as_deref() {
            let kind = match attrs.and_then(|values| values.get(attribute)) {
                Some(Value::Null) => "null",
                Some(Value::Bool(_)) => "boolean",
                Some(Value::Number(_)) => "number",
                Some(Value::String(_)) => "string",
                Some(Value::Object(_) | Value::Array(_)) => "object",
                None => "undefined",
            };
            if !allowed.split('|').any(|item| item == kind) {
                return Err(ModelError::InvalidAttributes(format!(
                    "Expected value of type {allowed} for attribute {attribute} on type {name}, got {kind}"
                )));
            }
        }
    }
    if let Some(Value::Object(attrs)) = attrs {
        if let Some(attribute) = attrs.keys().find(|attribute| !specs.contains_key(*attribute)) {
            return Err(ModelError::InvalidAttributes(format!("Unsupported attribute {attribute} on type {name}")));
        }
    }
    Ok(())
}
