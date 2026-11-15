use std::sync::Arc;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use crate::error::ModelError;
use crate::mark::Mark;

pub fn slice_utf16(s: &str, start_u16: usize, end_u16: usize) -> String {
    let u16_units: Vec<u16> = s.encode_utf16().collect();
    let start = start_u16.min(u16_units.len());
    let end = end_u16.min(u16_units.len());
    if start >= end {
        String::new()
    } else {
        String::from_utf16_lossy(&u16_units[start..end])
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Node {
    inner: Arc<NodeInner>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeInner {
    pub type_name: String,
    pub attrs: Option<Value>,
    pub content: Fragment,
    pub marks: Vec<Mark>,
    pub text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct Fragment {
    pub children: Vec<Node>,
    pub size: usize,
}

impl Fragment {
    pub fn empty() -> Self {
        Self {
            children: Vec::new(),
            size: 0,
        }
    }

    pub fn from_nodes(nodes: Vec<Node>) -> Self {
        let mut fragment = Fragment::empty();
        for node in nodes {
            fragment = fragment.append_node(node);
        }
        fragment
    }

    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    pub fn child(&self, index: usize) -> &Node {
        &self.children[index]
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn append_node(&self, node: Node) -> Self {
        let mut children = self.children.clone();
        if let Some(last) = children.last_mut() {
            if last.is_text() && node.is_text() && Mark::same_set(last.marks(), node.marks()) {
                let merged_text = format!("{}{}", last.text().unwrap_or(""), node.text().unwrap_or(""));
                *last = Node::new_text(merged_text, last.marks().to_vec());
                let total_size = children.iter().map(|c| c.node_size()).sum();
                return Self { children, size: total_size };
            }
        }
        children.push(node);
        let total_size = children.iter().map(|c| c.node_size()).sum();
        Self { children, size: total_size }
    }

    pub fn append(&self, other: &Fragment) -> Self {
        let mut result = self.clone();
        for child in &other.children {
            result = result.append_node(child.clone());
        }
        result
    }

    pub fn find_index(&self, pos: usize) -> (usize, usize) {
        self.find_index_round(pos, -1)
    }

    pub fn find_index_round(&self, pos: usize, round: i32) -> (usize, usize) {
        if pos == 0 {
            return (0, 0);
        }
        if pos >= self.size {
            return (self.children.len(), self.size);
        }
        let mut cur_pos = 0;
        for (i, child) in self.children.iter().enumerate() {
            let end = cur_pos + child.node_size();
            if end >= pos {
                if end == pos || round > 0 {
                    return (i + 1, end);
                }
                return (i, cur_pos);
            }
            cur_pos = end;
        }
        (self.children.len(), self.size)
    }

    pub fn cut(&self, from: usize, to: usize) -> Self {
        if from >= to || self.children.is_empty() {
            return Fragment::empty();
        }
        let mut result = Vec::new();
        let mut cur = 0;
        for child in &self.children {
            let end = cur + child.node_size();
            if end > from && cur < to {
                let start_offset = if from > cur { from - cur } else { 0 };
                let end_offset = if to < end { to - cur } else { child.node_size() };
                if start_offset == 0 && end_offset == child.node_size() {
                    result.push(child.clone());
                } else if child.is_text() {
                    let text = child.text().unwrap_or("");
                    let sliced = slice_utf16(text, start_offset, end_offset);
                    if !sliced.is_empty() {
                        result.push(Node::new_text(sliced, child.marks().to_vec()));
                    }
                } else {
                    let inner_from = if start_offset > 0 { start_offset - 1 } else { 0 };
                    let inner_to = if end_offset < child.node_size() { end_offset - 1 } else { child.content().size() };
                    let inner_content = child.content().cut(inner_from, inner_to);
                    result.push(child.copy(Some(inner_content)));
                }
            }
            cur = end;
        }
        Fragment::from_nodes(result)
    }

    pub fn replace_child(&self, index: usize, node: Node) -> Self {
        let mut children = self.children.clone();
        children[index] = node;
        let total_size = children.iter().map(|c| c.node_size()).sum();
        Self { children, size: total_size }
    }
}

impl Node {
    pub fn new(
        type_name: impl Into<String>,
        attrs: Option<Value>,
        content: Fragment,
        marks: Vec<Mark>,
    ) -> Self {
        Self {
            inner: Arc::new(NodeInner {
                type_name: type_name.into(),
                attrs,
                content,
                marks,
                text: None,
            }),
        }
    }

    pub fn new_text(text: impl Into<String>, marks: Vec<Mark>) -> Self {
        let t = text.into();
        Self {
            inner: Arc::new(NodeInner {
                type_name: "text".to_string(),
                attrs: None,
                content: Fragment::empty(),
                marks,
                text: Some(t),
            }),
        }
    }

    pub fn type_name(&self) -> &str {
        &self.inner.type_name
    }

    pub fn attrs(&self) -> Option<&Value> {
        self.inner.attrs.as_ref()
    }

    pub fn content(&self) -> &Fragment {
        &self.inner.content
    }

    pub fn marks(&self) -> &[Mark] {
        &self.inner.marks
    }

    pub fn text(&self) -> Option<&str> {
        self.inner.text.as_deref()
    }

    pub fn is_text(&self) -> bool {
        self.inner.type_name == "text"
    }

    pub fn is_leaf(&self) -> bool {
        self.inner.content.children.is_empty() && !self.is_text()
    }

    pub fn is_block(&self) -> bool {
        // Blocks include doc, paragraph, heading, blockquote, code_block, list_item, etc.
        !self.is_text() && self.type_name() != "hard_break"
    }

    pub fn node_size(&self) -> usize {
        if self.is_text() {
            // ProseMirror counts positions in UTF-16 code units
            self.text().map(|t| t.encode_utf16().count()).unwrap_or(0)
        } else if self.is_leaf() && self.type_name() != "doc" {
            1
        } else {
            2 + self.inner.content.size()
        }
    }

    pub fn child_count(&self) -> usize {
        self.inner.content.child_count()
    }

    pub fn child(&self, index: usize) -> &Node {
        self.inner.content.child(index)
    }

    pub fn copy(&self, content: Option<Fragment>) -> Self {
        Self {
            inner: Arc::new(NodeInner {
                type_name: self.inner.type_name.clone(),
                attrs: self.inner.attrs.clone(),
                content: content.unwrap_or_else(|| self.inner.content.clone()),
                marks: self.inner.marks.clone(),
                text: self.inner.text.clone(),
            }),
        }
    }

    pub fn cut(&self, from: usize, to: usize) -> Self {
        if from == 0 && to == self.node_size() {
            return self.clone();
        }
        if self.is_text() {
            let text = self.text().unwrap_or("");
            let sliced = slice_utf16(text, from, to);
            Node::new_text(sliced, self.marks().to_vec())
        } else {
            let inner_from = if from > 0 { from - 1 } else { 0 };
            let inner_to = if to < self.node_size() { to - 1 } else { self.content().size() };
            self.copy(Some(self.content().cut(inner_from, inner_to)))
        }
    }

    pub fn same_markup(&self, other: &Node) -> bool {
        self.type_name() == other.type_name()
            && self.attrs() == other.attrs()
            && Mark::same_set(self.marks(), other.marks())
    }

    pub fn text_between(&self, from: usize, to: usize) -> String {
        let mut result = String::new();
        let mut cur = 0;
        if self.is_text() {
            if let Some(t) = self.text() {
                result.push_str(&slice_utf16(t, from, to));
            }
            return result;
        }
        for child in &self.content().children {
            let end = cur + child.node_size();
            if end > from && cur < to {
                let child_from = if from > cur { from - cur } else { 0 };
                let child_to = if to < end { to - cur } else { child.node_size() };
                if child.is_text() {
                    result.push_str(&child.text_between(child_from, child_to));
                } else {
                    let inner_from = if child_from > 0 { child_from - 1 } else { 0 };
                    let inner_to = if child_to < child.node_size() { child_to - 1 } else { child.content().size() };
                    result.push_str(&child.text_between(inner_from, inner_to));
                }
            }
            cur = end;
        }
        result
    }
}

// JSON Serialization matching ProseMirror 100%
impl Serialize for Node {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("type", self.type_name())?;
        if let Some(ref attrs) = self.inner.attrs {
            map.serialize_entry("attrs", attrs)?;
        }
        if let Some(ref text) = self.inner.text {
            map.serialize_entry("text", text)?;
        }
        if !self.inner.content.children.is_empty() {
            map.serialize_entry("content", &self.inner.content.children)?;
        }
        if !self.inner.marks.is_empty() {
            map.serialize_entry("marks", &self.inner.marks)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawNode {
            #[serde(rename = "type")]
            type_name: String,
            attrs: Option<Value>,
            content: Option<Vec<Node>>,
            marks: Option<Vec<Mark>>,
            text: Option<String>,
        }

        let raw = RawNode::deserialize(deserializer)?;
        let marks = raw.marks.unwrap_or_default();
        if let Some(text) = raw.text {
            Ok(Node::new_text(text, marks))
        } else {
            let content = match raw.content {
                Some(nodes) => Fragment::from_nodes(nodes),
                None => Fragment::empty(),
            };
            Ok(Node::new(raw.type_name, raw.attrs, content, marks))
        }
    }
}
