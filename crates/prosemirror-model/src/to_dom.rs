use std::collections::HashMap;

use crate::{DomAttribute, DomElement, DomNode, Fragment, Mark, ModelError, Node, Schema};

/// An owned DOM output description. A content hole is represented explicitly
/// so that document attributes cannot be interpreted as executable markup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomOutputSpec {
    Text(String),
    Element {
        name: String,
        namespace: Option<String>,
        attributes: Vec<DomAttribute>,
        children: Vec<DomOutputSpec>,
    },
    Existing(RenderedDom),
    Hole,
}

impl DomOutputSpec {
    pub fn element(name: impl Into<String>, children: Vec<Self>) -> Self {
        Self::element_with_attrs(name, Vec::new(), children)
    }

    pub fn element_with_attrs(
        name: impl Into<String>,
        attributes: Vec<DomAttribute>,
        children: Vec<Self>,
    ) -> Self {
        Self::Element { name: name.into(), namespace: None, attributes, children }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedDom {
    pub dom: DomNode,
    /// Child indices from the root to the content element.
    pub content_path: Option<Vec<usize>>,
}

pub type NodeToDom = fn(&Node) -> DomOutputSpec;
pub type MarkToDom = fn(&Mark, bool) -> DomOutputSpec;

#[derive(Clone, Debug, Default)]
pub struct DomSerializer {
    pub nodes: HashMap<String, NodeToDom>,
    pub marks: HashMap<String, MarkToDom>,
}

impl DomSerializer {
    pub fn new(nodes: HashMap<String, NodeToDom>, marks: HashMap<String, MarkToDom>) -> Self {
        Self { nodes, marks }
    }

    pub fn from_schema(schema: &Schema) -> Self {
        Self::new(
            schema.node_types().filter_map(|kind| kind.spec.to_dom.map(|serialize| (kind.name.clone(), serialize))).collect(),
            schema.mark_types().filter_map(|kind| kind.spec.to_dom.map(|serialize| (kind.name.clone(), serialize))).collect(),
        )
    }

    pub fn serialize_fragment(&self, fragment: &Fragment) -> Result<DomNode, ModelError> {
        let mut root = DomNode::Fragment(Vec::new());
        self.append_fragment(fragment, &mut root)?;
        Ok(root)
    }

    pub fn append_fragment(&self, fragment: &Fragment, target: &mut DomNode) -> Result<(), ModelError> {
        let mut active: Vec<(&Mark, usize)> = Vec::new();
        let mut path = Vec::new();
        for node in &fragment.children {
            let mut keep = 0;
            let mut marks = node.marks().iter().filter(|mark| self.marks.contains_key(&mark.type_name)).peekable();
            while let (Some((active_mark, _)), Some(mark)) = (active.get(keep), marks.peek()) {
                if mark != active_mark || !mark.is_spanning() {
                    break;
                }
                keep += 1;
                marks.next();
            }
            if keep < active.len() {
                path.truncate(active[keep].1);
                active.truncate(keep);
            }
            for mark in marks {
                let rendered = self.serialize_mark(mark, node.is_inline())?
                    .expect("the mark was checked against the serializer");
                let children = children_at(target, &path)?;
                active.push((mark, path.len()));
                path.push(children.len());
                if let Some(content_path) = rendered.content_path {
                    path.extend(content_path);
                }
                children.push(rendered.dom);
            }
            children_at(target, &path)?.push(self.serialize_node_inner(node)?);
        }
        Ok(())
    }

    pub fn serialize_node(&self, node: &Node) -> Result<DomNode, ModelError> {
        let mut dom = self.serialize_node_inner(node)?;
        for mark in node.marks().iter().rev() {
            if let Some(mut wrapped) = self.serialize_mark(mark, node.is_inline())? {
                children_at(&mut wrapped.dom, wrapped.content_path.as_deref().unwrap_or_default())?.push(dom);
                dom = wrapped.dom;
            }
        }
        Ok(dom)
    }

    pub fn serialize_mark(&self, mark: &Mark, inline: bool) -> Result<Option<RenderedDom>, ModelError> {
        self.marks.get(&mark.type_name)
            .map(|serialize| Self::render_element_spec(serialize(mark, inline), None))
            .transpose()
    }

    fn serialize_node_inner(&self, node: &Node) -> Result<DomNode, ModelError> {
        if let Some(text) = node.text() {
            return Ok(DomNode::Text(text.to_owned()));
        }
        let serialize = self.nodes.get(node.type_name()).ok_or_else(|| {
            ModelError::Dom(format!("Missing DOM serializer for {}", node.type_name()))
        })?;
        let mut rendered = Self::render_element_spec(serialize(node), None)?;
        if let Some(path) = rendered.content_path {
            if node.is_leaf() {
                return Err(ModelError::Dom("Content hole not allowed in a leaf node spec".to_owned()));
            }
            let target = rendered.dom.at_path_mut(&path).ok_or_else(invalid_content_path)?;
            self.append_fragment(node.content(), target)?;
        }
        Ok(rendered.dom)
    }

    pub fn render_spec(spec: DomOutputSpec) -> Result<RenderedDom, ModelError> {
        Self::render_spec_with_namespace(spec, None)
    }

    pub fn render_spec_with_namespace(
        spec: DomOutputSpec,
        inherited_namespace: Option<&str>,
    ) -> Result<RenderedDom, ModelError> {
        if let DomOutputSpec::Text(value) = spec {
            return Ok(RenderedDom { dom: DomNode::Text(value), content_path: None });
        }
        Self::render_element_spec(spec, inherited_namespace)
    }

    fn render_element_spec(
        spec: DomOutputSpec,
        inherited_namespace: Option<&str>,
    ) -> Result<RenderedDom, ModelError> {
        match spec {
            DomOutputSpec::Existing(rendered) if matches!(rendered.dom, DomNode::Element(_)) => Ok(rendered),
            DomOutputSpec::Text(_) | DomOutputSpec::Existing(_) => {
                Err(ModelError::Dom("A DOM output specification must describe an element".to_owned()))
            }
            DomOutputSpec::Hole => Err(ModelError::Dom("Content hole requires a parent element".to_owned())),
            DomOutputSpec::Element { name, namespace, attributes, children } => {
                let (name, namespace) = if let Some((ns, tag)) = name.split_once(' ') {
                    (tag.to_owned(), Some(ns.to_owned()))
                } else {
                    (name, namespace.or_else(|| inherited_namespace.map(str::to_owned)))
                };
                let mut element = DomElement { name, namespace, attributes, children: Vec::with_capacity(children.len()) };
                let mut content_path = None;
                if children.as_slice() == [DomOutputSpec::Hole] {
                    content_path = Some(Vec::new());
                } else {
                    for (index, child) in children.into_iter().enumerate() {
                        let rendered = match child {
                            DomOutputSpec::Hole => return Err(ModelError::Dom(
                                "Content hole must be the only child of its parent node".to_owned(),
                            )),
                            DomOutputSpec::Text(value) => {
                                element.children.push(DomNode::Text(value));
                                continue;
                            }
                            child => Self::render_element_spec(child, element.namespace.as_deref())?,
                        };
                        if let Some(inner_path) = rendered.content_path {
                            if content_path.is_some() {
                                return Err(ModelError::Dom("Multiple content holes".to_owned()));
                            }
                            let mut path = vec![index];
                            path.extend(inner_path);
                            content_path = Some(path);
                        }
                        element.children.push(rendered.dom);
                    }
                }
                Ok(RenderedDom { dom: DomNode::Element(element), content_path })
            }
        }
    }
}

fn invalid_content_path() -> ModelError {
    ModelError::Dom("Content hole must identify a DOM container".to_owned())
}

fn children_at<'a>(target: &'a mut DomNode, path: &[usize]) -> Result<&'a mut Vec<DomNode>, ModelError> {
    target.at_path_mut(path).and_then(DomNode::children_mut).ok_or_else(invalid_content_path)
}
