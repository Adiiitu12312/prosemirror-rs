use std::borrow::Cow;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::{ContentMatch, DiffEnd, Mark, ModelError, NodeType, ResolvedPos, Schema, Slice};

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

#[derive(Clone)]
pub struct Node {
    inner: Arc<NodeInner>,
    schema: Option<Schema>,
    unpaired_text: Option<Arc<[u16]>>,
    raw_leaf: bool,
    raw_inline_content: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NodeInner {
    pub type_name: String,
    pub attrs: Option<Value>,
    pub content: Fragment,
    pub marks: Vec<Mark>,
    pub text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Default)]
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
        let mut children: Vec<Node> = Vec::with_capacity(nodes.len());
        for node in nodes {
            if let Some(last) = children.last_mut() {
                if last.is_text() && last.same_markup(&node) {
                    *last = last.append_text(&node);
                    continue;
                }
            }
            children.push(node);
        }
        Self::from_children(children)
    }

    pub(crate) fn from_children(children: Vec<Node>) -> Self {
        Self {
            size: children.iter().map(Node::node_size).sum(),
            children,
        }
    }

    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    pub fn child(&self, index: usize) -> &Node {
        &self.children[index]
    }

    pub fn maybe_child(&self, index: usize) -> Option<&Node> {
        self.children.get(index)
    }

    pub fn first_child(&self) -> Option<&Node> {
        self.children.first()
    }

    pub fn last_child(&self) -> Option<&Node> {
        self.children.last()
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn append_node(&self, node: Node) -> Self {
        self.append(&Self::from_children(vec![node]))
    }

    pub fn append(&self, other: &Fragment) -> Self {
        let Some(first) = other.first_child() else {
            return self.clone();
        };
        let Some(last) = self.last_child() else {
            return other.clone();
        };
        let mut children = self.children.clone();
        let mut start = 0;
        if last.is_text() && last.same_markup(first) {
            let index = children.len() - 1;
            children[index] = last.append_text(first);
            start = 1;
        }
        children.extend_from_slice(&other.children[start..]);
        Self { children, size: self.size + other.size }
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
                if from <= cur && end <= to {
                    result.push(child.clone());
                } else if child.is_text() {
                    result.push(child.cut(from.saturating_sub(cur), (to - cur).min(child.node_size())));
                } else {
                    let inner_from = from.saturating_sub(cur + 1);
                    let inner_to = to.saturating_sub(cur + 1).min(child.content().size());
                    result.push(child.cut(inner_from, inner_to));
                }
            }
            cur = end;
        }
        Fragment::from_children(result)
    }

    pub fn cut_by_index(&self, from: usize, to: usize) -> Self {
        Self::from_children(self.children[from..to].to_vec())
    }

    pub fn add_to_start(&self, node: Node) -> Self {
        let size = self.size + node.node_size();
        let mut children = Vec::with_capacity(self.child_count() + 1);
        children.push(node);
        children.extend_from_slice(&self.children);
        Self { children, size }
    }

    pub fn add_to_end(&self, node: Node) -> Self {
        let size = self.size + node.node_size();
        let mut children = self.children.clone();
        children.push(node);
        Self { children, size }
    }

    pub fn replace_child(&self, index: usize, node: Node) -> Self {
        let size = self.size - self.children[index].node_size() + node.node_size();
        let mut children = self.children.clone();
        children[index] = node;
        Self { children, size }
    }

    pub fn for_each(&self, mut visit: impl FnMut(&Node, usize, usize)) {
        let mut pos = 0;
        for (index, node) in self.children.iter().enumerate() {
            visit(node, pos, index);
            pos += node.node_size();
        }
    }

    pub fn nodes_between(
        &self,
        from: usize,
        to: usize,
        mut visit: impl FnMut(&Node, usize, Option<&Node>, usize) -> bool,
    ) {
        self.visit_between(from, to, 0, None, &mut visit);
    }

    pub fn descendants(&self, visit: impl FnMut(&Node, usize, Option<&Node>, usize) -> bool) {
        self.nodes_between(0, self.size, visit);
    }

    fn visit_between(
        &self,
        from: usize,
        to: usize,
        base: usize,
        parent: Option<&Node>,
        visit: &mut impl FnMut(&Node, usize, Option<&Node>, usize) -> bool,
    ) {
        let mut pos = 0;
        for (index, child) in self.children.iter().enumerate() {
            if pos >= to {
                break;
            }
            let end = pos + child.node_size();
            if end > from && visit(child, base + pos, parent, index) && child.content().size() > 0 {
                let start = pos + 1;
                child.content().visit_between(
                    from.saturating_sub(start),
                    to.saturating_sub(start).min(child.content().size()),
                    base + start,
                    Some(child),
                    visit,
                );
            }
            pos = end;
        }
    }

    pub fn text_between(
        &self,
        from: usize,
        to: usize,
        block_separator: Option<&str>,
        leaf_text: Option<&dyn Fn(&Node) -> String>,
    ) -> String {
        let mut result = String::new();
        let mut first = true;
        self.nodes_between(from, to, |node, pos, _, _| {
            let text = if let Some(text) = node.text() {
                slice_utf16(text, from.saturating_sub(pos), to.saturating_sub(pos))
            } else if node.is_leaf() {
                leaf_text.map_or_else(|| node.leaf_text(), |text| text(node))
            } else {
                String::new()
            };
            if node.is_block() && (node.is_textblock() || node.is_leaf() && !text.is_empty()) {
                if let Some(separator) = block_separator {
                    if !first {
                        result.push_str(separator);
                    }
                    first = false;
                }
            }
            result.push_str(&text);
            true
        });
        result
    }

    pub fn find_diff_start(&self, other: &Self) -> Option<usize> {
        crate::diff::find_diff_start(self, other, 0)
    }

    pub fn find_diff_start_at(&self, other: &Self, pos: usize) -> Option<usize> {
        crate::diff::find_diff_start(self, other, pos)
    }

    pub fn find_diff_end(&self, other: &Self) -> Option<DiffEnd> {
        self.find_diff_end_at(other, self.size(), other.size())
    }

    pub fn find_diff_end_at(&self, other: &Self, pos_a: usize, pos_b: usize) -> Option<DiffEnd> {
        crate::diff::find_diff_end(self, other, pos_a, pos_b)
    }
}

impl Node {
    pub fn new(
        type_name: impl Into<String>,
        attrs: Option<Value>,
        content: Fragment,
        marks: Vec<Mark>,
    ) -> Self {
        Self::new_in_schema(type_name.into(), attrs, content, marks, None)
    }

    pub(crate) fn new_in_schema(
        type_name: String,
        attrs: Option<Value>,
        content: Fragment,
        marks: Vec<Mark>,
        schema: Option<Schema>,
    ) -> Self {
        let raw_textblock = matches!(type_name.as_str(), "paragraph" | "heading" | "code_block");
        let raw_leaf = content.children.is_empty()
            && !raw_textblock
            && !matches!(
                type_name.as_str(),
                "doc" | "blockquote" | "ordered_list" | "bullet_list" | "list_item"
                    | "table" | "table_row" | "table_cell" | "table_header"
            );
        let raw_inline_content = raw_textblock || content.first_child().is_some_and(Node::is_inline);
        Self {
            inner: Arc::new(NodeInner {
                type_name,
                attrs,
                content,
                marks,
                text: None,
            }),
            schema,
            unpaired_text: None,
            raw_leaf,
            raw_inline_content,
        }
    }

    pub fn new_text(text: impl Into<String>, marks: Vec<Mark>) -> Self {
        Self::new_text_in_schema(text, marks, None)
    }

    pub(crate) fn new_text_in_schema(
        text: impl Into<String>,
        marks: Vec<Mark>,
        schema: Option<Schema>,
    ) -> Self {
        Self {
            inner: Arc::new(NodeInner {
                type_name: "text".to_string(),
                attrs: None,
                content: Fragment::empty(),
                marks,
                text: Some(text.into()),
            }),
            schema,
            unpaired_text: None,
            raw_leaf: true,
            raw_inline_content: false,
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

    /// Returns the original UTF-16 code units, even for a text fragment
    /// containing an unpaired surrogate.
    pub fn text_utf16(&self) -> Option<Cow<'_, [u16]>> {
        if let Some(unpaired) = &self.unpaired_text {
            return Some(Cow::Borrowed(unpaired.as_ref()));
        }
        self.text().map(|text| Cow::Owned(text.encode_utf16().collect()))
    }

    pub fn schema(&self) -> Option<&Schema> {
        self.schema.as_ref()
    }

    pub fn node_type(&self) -> Option<&NodeType> {
        self.schema()?.node_type(self.type_name())
    }

    pub fn is_text(&self) -> bool {
        self.inner.type_name == "text"
    }

    pub fn is_leaf(&self) -> bool {
        self.node_type().map_or(self.raw_leaf, NodeType::is_leaf)
    }

    pub fn is_block(&self) -> bool {
        !self.is_inline()
    }

    pub fn is_inline(&self) -> bool {
        self.node_type().map_or(
            matches!(self.type_name(), "text" | "image" | "hard_break"),
            NodeType::is_inline,
        )
    }

    pub fn inline_content(&self) -> bool {
        self.node_type().map_or(self.raw_inline_content, NodeType::inline_content)
    }

    pub fn is_textblock(&self) -> bool {
        self.is_block() && self.inline_content()
    }

    pub fn is_atom(&self) -> bool {
        self.node_type().map_or(self.raw_leaf, NodeType::is_atom)
    }

    pub fn is_selectable(&self) -> bool {
        !self.is_text() && self.node_type().map_or(self.type_name() != "hard_break", |kind| kind.spec.selectable)
    }

    pub fn is_isolating(&self) -> bool {
        self.node_type().is_some_and(|kind| kind.spec.isolating)
    }

    pub fn node_size(&self) -> usize {
        if self.is_text() {
            // ProseMirror counts positions in UTF-16 code units
            self.unpaired_text.as_ref().map_or_else(
                || self.text().map_or(0, |text| text.encode_utf16().count()),
                |units| units.len(),
            )
        } else if self.is_leaf() {
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

    pub fn maybe_child(&self, index: usize) -> Option<&Node> {
        self.content().maybe_child(index)
    }

    pub fn first_child(&self) -> Option<&Node> {
        self.content().first_child()
    }

    pub fn last_child(&self) -> Option<&Node> {
        self.content().last_child()
    }

    pub fn node_at(&self, mut pos: usize) -> Option<&Node> {
        let mut current = self;
        loop {
            let (index, offset) = current.content().find_index(pos);
            current = current.maybe_child(index)?;
            if offset == pos || current.is_text() {
                return Some(current);
            }
            pos -= offset + 1;
        }
    }

    pub fn child_after(&self, pos: usize) -> (Option<&Node>, usize, usize) {
        let (index, offset) = self.content().find_index(pos);
        (self.maybe_child(index), index, offset)
    }

    pub fn child_before(&self, pos: usize) -> (Option<&Node>, usize, usize) {
        if pos == 0 {
            return (None, 0, 0);
        }
        let (index, offset) = self.content().find_index(pos);
        if offset < pos {
            return (self.maybe_child(index), index, offset);
        }
        let node = self.child(index - 1);
        (Some(node), index - 1, offset - node.node_size())
    }

    pub fn for_each(&self, visit: impl FnMut(&Node, usize, usize)) {
        self.content().for_each(visit);
    }

    pub fn nodes_between(
        &self,
        from: usize,
        to: usize,
        mut visit: impl FnMut(&Node, usize, Option<&Node>, usize) -> bool,
    ) {
        self.content().visit_between(from, to, 0, Some(self), &mut visit);
    }

    pub fn descendants(&self, visit: impl FnMut(&Node, usize, Option<&Node>, usize) -> bool) {
        self.nodes_between(0, self.content().size(), visit);
    }

    pub fn resolve(&self, pos: usize) -> Result<ResolvedPos, ModelError> {
        ResolvedPos::resolve(self, pos)
    }

    pub fn copy(&self, content: Option<Fragment>) -> Self {
        if self.is_text() {
            return self.clone();
        }
        let mut node = self.clone();
        Arc::make_mut(&mut node.inner).content = content.unwrap_or_default();
        node
    }

    pub fn with_marks(&self, marks: Vec<Mark>) -> Self {
        let mut node = self.clone();
        Arc::make_mut(&mut node.inner).marks = marks;
        node
    }

    pub fn with_attrs(&self, attrs: Option<Value>) -> Self {
        let mut node = self.clone();
        Arc::make_mut(&mut node.inner).attrs = attrs;
        node
    }

    pub(crate) fn with_text(&self, text: String) -> Self {
        let mut node = self.clone();
        Arc::make_mut(&mut node.inner).text = Some(text);
        node.unpaired_text = None;
        node
    }

    fn with_text_utf16(&self, units: &[u16]) -> Self {
        match String::from_utf16(units) {
            Ok(text) => self.with_text(text),
            Err(_) => {
                let mut node = self.with_text(String::from_utf16_lossy(units));
                node.unpaired_text = Some(Arc::from(units));
                node
            }
        }
    }

    pub(crate) fn append_text(&self, other: &Node) -> Self {
        if self.unpaired_text.is_none() && other.unpaired_text.is_none() {
            let mut text = self.text().unwrap_or_default().to_owned();
            text.push_str(other.text().unwrap_or_default());
            return self.with_text(text);
        }
        let mut units = self.text_utf16().unwrap_or_default().into_owned();
        units.extend_from_slice(&other.text_utf16().unwrap_or_default());
        self.with_text_utf16(&units)
    }

    pub fn cut(&self, from: usize, to: usize) -> Self {
        if let Some(units) = self.text_utf16() {
            let end = to.min(units.len());
            self.with_text_utf16(&units[from.min(end)..end])
        } else if from == 0 && to == self.content().size() {
            self.clone()
        } else {
            self.copy(Some(self.content().cut(from, to)))
        }
    }

    pub fn same_markup(&self, other: &Node) -> bool {
        self.type_name() == other.type_name()
            && same_attrs(self.attrs(), other.attrs())
            && Mark::same_set(self.marks(), other.marks())
    }

    pub fn slice(&self, from: usize, to: usize, include_parents: bool) -> Result<Slice, ModelError> {
        if from == to {
            return Ok(Slice::empty());
        }
        let start = self.resolve(from)?;
        let end = self.resolve(to)?;
        let depth = if include_parents { 0 } else { start.shared_depth(to) };
        let offset = start.start(depth);
        let content = start.node(depth).content().cut(from - offset, to - offset);
        Ok(Slice::new(content, start.depth - depth, end.depth - depth))
    }

    pub fn replace(&self, from: usize, to: usize, slice: &Slice) -> Result<Self, ModelError> {
        crate::replace_at_pos(self, from, to, slice)
    }

    pub fn text_between(&self, from: usize, to: usize) -> String {
        self.text_between_with(from, to, None, None)
    }

    pub fn text_between_with(
        &self,
        from: usize,
        to: usize,
        block_separator: Option<&str>,
        leaf_text: Option<&dyn Fn(&Node) -> String>,
    ) -> String {
        if let Some(text) = self.text() {
            slice_utf16(text, from, to)
        } else {
            self.content().text_between(from, to, block_separator, leaf_text)
        }
    }

    pub fn text_content(&self) -> String {
        if let Some(text) = self.text() {
            text.to_string()
        } else if self.is_leaf() {
            self.leaf_text()
        } else {
            self.text_between(0, self.content().size())
        }
    }

    fn leaf_text(&self) -> String {
        self.node_type().and_then(|kind| kind.spec.leaf_text)
            .map_or_else(String::new, |text| text(self))
    }

    pub fn range_has_mark(&self, from: usize, to: usize, mark_type: &str) -> bool {
        let mut found = false;
        if from < to {
            self.nodes_between(from, to, |node, _, _, _| {
                found |= node.marks().iter().any(|mark| mark.type_name == mark_type);
                !found
            });
        }
        found
    }

    pub fn allows_mark_type(&self, mark: &str) -> bool {
        self.node_type().map_or(self.type_name() != "code_block", |kind| kind.allows_mark_type(mark))
    }

    pub fn content_match_at(&self, index: usize) -> Option<ContentMatch> {
        self.node_type()?.content_match().match_fragment(self.content(), 0, index)
    }

    pub fn can_replace(&self, from: usize, to: usize, replacement: &Fragment) -> bool {
        self.can_replace_range(from, to, replacement, 0, replacement.child_count())
    }

    pub fn can_replace_range(
        &self, from: usize, to: usize, replacement: &Fragment, start: usize, end: usize,
    ) -> bool {
        let Some(children) = replacement.children.get(start..end) else { return false };
        if from > to || to > self.child_count() || children.iter()
            .any(|node| node.marks().iter().any(|mark| !self.allows_mark_type(&mark.type_name)))
        {
            return false;
        }
        if self.node_type().is_none() {
            return children.is_empty() || !self.is_leaf()
                && children.iter().all(|node| node.is_inline() == self.inline_content());
        }
        self.content_match_at(from)
            .and_then(|match_start| match_start.match_fragment(replacement, start, end))
            .and_then(|matched| matched.match_fragment(self.content(), to, self.child_count()))
            .is_some_and(|matched| matched.valid_end())
    }

    pub fn can_replace_with(&self, from: usize, to: usize, type_name: &str, marks: &[Mark]) -> bool {
        if from > to || to > self.child_count() || marks.iter().any(|mark| !self.allows_mark_type(&mark.type_name)) {
            return false;
        }
        if self.node_type().is_none() {
            return !self.is_leaf()
                && matches!(type_name, "text" | "image" | "hard_break") == self.inline_content();
        }
        self.content_match_at(from)
            .and_then(|match_start| match_start.match_type(type_name))
            .and_then(|matched| matched.match_fragment(self.content(), to, self.child_count()))
            .is_some_and(|matched| matched.valid_end())
    }

    pub fn can_append(&self, other: &Node) -> bool {
        if other.content().size() > 0 {
            self.can_replace(self.child_count(), self.child_count(), other.content())
        } else {
            self.compatible_content(other)
        }
    }

    pub fn compatible_content(&self, other: &Node) -> bool {
        if self.type_name() == other.type_name() {
            return true;
        }
        let kind = |node: &Node| node.node_type().cloned()
            .or_else(|| self.schema()?.node_type(node.type_name()).cloned())
            .or_else(|| other.schema()?.node_type(node.type_name()).cloned());
        match (kind(self), kind(other)) {
            (Some(a), Some(b)) => a.compatible_content(&b),
            _ => !self.is_leaf() && !other.is_leaf() && self.inline_content() == other.inline_content(),
        }
    }

    pub fn check(&self) -> Result<(), ModelError> {
        self.check_with_schema(self.schema())
    }

    fn check_with_schema(&self, schema: Option<&Schema>) -> Result<(), ModelError> {
        if let Some(schema) = schema {
            let node_type = schema.node_type(self.type_name())
                .ok_or_else(|| ModelError::UnknownNodeType(self.type_name().to_owned()))?;
            node_type.check_content(self.content())?;
            crate::schema::check_attrs(&node_type.spec.attrs, self.attrs(), self.type_name())?;
        }
        let mut checked = Vec::new();
        for mark in self.marks() {
            if let Some(schema) = schema {
                schema.mark_type(&mark.type_name)
                    .ok_or_else(|| ModelError::UnknownMarkType(mark.type_name.clone()))?
                    .check_attrs(mark.attrs.as_ref())?;
            }
            checked = Mark::add_to_set(&checked, mark.clone());
        }
        if !Mark::same_set(&checked, self.marks()) {
            return Err(ModelError::InvalidMarks(self.type_name().to_owned()));
        }
        for child in &self.content().children {
            child.check_with_schema(schema.or_else(|| child.schema()))?;
        }
        Ok(())
    }
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner) || (
            self.same_markup(other) && self.text() == other.text() && self.content() == other.content()
                && self.unpaired_text == other.unpaired_text
        )
    }
}

impl std::fmt::Debug for Node {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.inner, formatter)
    }
}

impl std::fmt::Display for Node {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(format_node) = self.node_type().and_then(|kind| kind.spec.to_debug_string) {
            return formatter.write_str(&format_node(self));
        }
        for mark in self.marks() {
            write!(formatter, "{}(", mark.type_name)?;
        }
        if let Some(text) = self.text() {
            write!(formatter, "{}", serde_json::Value::String(text.to_owned()))?;
        } else {
            formatter.write_str(self.type_name())?;
            if self.content().size() > 0 {
                formatter.write_str("(")?;
                self.content().fmt_contents(formatter)?;
                formatter.write_str(")")?;
            }
        }
        for _ in self.marks() {
            formatter.write_str(")")?;
        }
        Ok(())
    }
}

impl Fragment {
    fn fmt_contents(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, child) in self.children.iter().enumerate() {
            if index != 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "{child}")?;
        }
        Ok(())
    }
}

impl std::fmt::Display for Fragment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("<")?;
        self.fmt_contents(formatter)?;
        formatter.write_str(">")
    }
}

pub(crate) fn empty_attrs(attrs: &Option<Value>) -> bool {
    attrs.as_ref().is_none_or(|value| value.is_null() || value.as_object().is_some_and(serde_json::Map::is_empty))
}

pub(crate) fn same_attrs(a: Option<&Value>, b: Option<&Value>) -> bool {
    fn nonempty(value: &&Value) -> bool {
        !value.is_null() && value.as_object().is_none_or(|map| !map.is_empty())
    }
    match (a.filter(nonempty), b.filter(nonempty)) {
        (Some(a), Some(b)) => crate::compare_deep::compare_deep(a, b),
        (None, None) => true,
        _ => false,
    }
}

impl Serialize for Node {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("type", self.type_name())?;
        if !empty_attrs(&self.inner.attrs) {
            map.serialize_entry("attrs", &self.inner.attrs)?;
        }
        if let Some(ref text) = self.inner.text {
            map.serialize_entry("text", text)?;
        }
        if self.inner.content.size() != 0 {
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
        if raw.type_name == "text" {
            let text = raw.text.filter(|text| !text.is_empty())
                .ok_or_else(|| serde::de::Error::custom("Invalid text node in JSON"))?;
            Ok(Node::new_text(text, marks))
        } else {
            let content = match raw.content {
                Some(nodes) => Fragment::from_children(nodes),
                None => Fragment::empty(),
            };
            Ok(Node::new(raw.type_name, raw.attrs, content, marks))
        }
    }
}
