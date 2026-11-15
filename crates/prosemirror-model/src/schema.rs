use std::collections::HashMap;
use std::sync::Arc;
use serde_json::Value;
use crate::error::ModelError;
use crate::mark::Mark;
use crate::node::{Fragment, Node};

#[derive(Clone, Debug)]
pub struct NodeSpec {
    pub content: Option<String>,
    pub marks: Option<String>,
    pub group: Option<String>,
    pub inline: bool,
    pub atom: bool,
    pub selectable: bool,
    pub draggable: bool,
    pub code: bool,
    pub defining: bool,
    pub isolating: bool,
}

impl Default for NodeSpec {
    fn default() -> Self {
        Self {
            content: None,
            marks: None,
            group: None,
            inline: false,
            atom: false,
            selectable: true,
            draggable: false,
            code: false,
            defining: false,
            isolating: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MarkSpec {
    pub attrs: Option<HashMap<String, Value>>,
    pub inclusive: bool,
    pub excludes: Option<String>,
    pub group: Option<String>,
    pub spanning: bool,
}

#[derive(Clone, Debug)]
pub struct NodeType {
    pub name: String,
    pub spec: NodeSpec,
}

impl NodeType {
    pub fn new(name: impl Into<String>, spec: NodeSpec) -> Self {
        Self {
            name: name.into(),
            spec,
        }
    }

    pub fn create(
        &self,
        attrs: Option<Value>,
        content: Option<Fragment>,
        marks: Option<Vec<Mark>>,
    ) -> Node {
        Node::new(
            self.name.clone(),
            attrs,
            content.unwrap_or_else(Fragment::empty),
            marks.unwrap_or_default(),
        )
    }

    pub fn is_block(&self) -> bool {
        !self.spec.inline && self.name != "text"
    }

    pub fn is_inline(&self) -> bool {
        self.spec.inline || self.name == "text"
    }

    pub fn is_text(&self) -> bool {
        self.name == "text"
    }
}

#[derive(Clone, Debug)]
pub struct MarkType {
    pub name: String,
    pub spec: MarkSpec,
}

impl MarkType {
    pub fn new(name: impl Into<String>, spec: MarkSpec) -> Self {
        Self {
            name: name.into(),
            spec,
        }
    }

    pub fn create(&self, attrs: Option<Value>) -> Mark {
        Mark::new(self.name.clone(), attrs)
    }
}

#[derive(Clone, Debug)]
pub struct Schema {
    inner: Arc<SchemaInner>,
}

#[derive(Clone, Debug)]
struct SchemaInner {
    nodes: HashMap<String, NodeType>,
    marks: HashMap<String, MarkType>,
    top_node_type: String,
}

impl Schema {
    pub fn new(
        nodes: Vec<(String, NodeSpec)>,
        marks: Vec<(String, MarkSpec)>,
    ) -> Self {
        let mut node_map = HashMap::new();
        for (name, spec) in nodes {
            node_map.insert(name.clone(), NodeType::new(name, spec));
        }
        // Ensure "text" type exists
        if !node_map.contains_key("text") {
            node_map.insert(
                "text".to_string(),
                NodeType::new("text", NodeSpec { inline: true, ..Default::default() }),
            );
        }

        let mut mark_map = HashMap::new();
        for (name, spec) in marks {
            mark_map.insert(name.clone(), MarkType::new(name, spec));
        }

        let top_node_type = if node_map.contains_key("doc") {
            "doc".to_string()
        } else {
            node_map.keys().next().cloned().unwrap_or_else(|| "doc".to_string())
        };

        Self {
            inner: Arc::new(SchemaInner {
                nodes: node_map,
                marks: mark_map,
                top_node_type,
            }),
        }
    }

    pub fn node_type(&self, name: &str) -> Option<&NodeType> {
        self.inner.nodes.get(name)
    }

    pub fn mark_type(&self, name: &str) -> Option<&MarkType> {
        self.inner.marks.get(name)
    }

    pub fn node(
        &self,
        type_name: &str,
        attrs: Option<Value>,
        content: Option<Fragment>,
        marks: Option<Vec<Mark>>,
    ) -> Result<Node, ModelError> {
        let nt = self.node_type(type_name).ok_or_else(|| {
            ModelError::UnknownNodeType(type_name.to_string())
        })?;
        Ok(nt.create(attrs, content, marks))
    }

    pub fn text(&self, text: &str, marks: Option<Vec<Mark>>) -> Node {
        Node::new_text(text, marks.unwrap_or_default())
    }

    pub fn node_from_json(&self, val: &Value) -> Result<Node, ModelError> {
        serde_json::from_value(val.clone()).map_err(|e| ModelError::Serialization(e.to_string()))
    }
}
