//! Portable DOM values shared by the document parser and serializer.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomNode {
    Text(String),
    Element(DomElement),
    Fragment(Vec<DomNode>),
    Comment(String),
}

impl DomNode {
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    pub fn element(name: impl Into<String>, children: Vec<Self>) -> Self {
        Self::Element(DomElement::new(name, children))
    }

    pub fn children(&self) -> &[Self] {
        match self {
            Self::Element(element) => &element.children,
            Self::Fragment(children) => children,
            Self::Text(_) | Self::Comment(_) => &[],
        }
    }

    pub fn as_element(&self) -> Option<&DomElement> {
        match self {
            Self::Element(element) => Some(element),
            _ => None,
        }
    }

    pub(crate) fn children_mut(&mut self) -> Option<&mut Vec<Self>> {
        match self {
            Self::Element(element) => Some(&mut element.children),
            Self::Fragment(children) => Some(children),
            Self::Text(_) | Self::Comment(_) => None,
        }
    }

    pub(crate) fn at_path_mut(&mut self, path: &[usize]) -> Option<&mut Self> {
        let mut node = self;
        for &index in path {
            node = node.children_mut()?.get_mut(index)?;
        }
        Some(node)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomElement {
    pub name: String,
    pub namespace: Option<String>,
    pub attributes: Vec<DomAttribute>,
    pub children: Vec<DomNode>,
}

impl DomElement {
    pub fn new(name: impl Into<String>, children: Vec<DomNode>) -> Self {
        Self {
            name: name.into(),
            namespace: None,
            attributes: Vec::new(),
            children,
        }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|attribute| attribute.name == name && attribute.namespace.is_none())
            .map(|attribute| attribute.value.as_str())
    }

    pub fn style(&self, property: &str) -> Option<&str> {
        self.attr("style")?
            .rsplit(';')
            .filter_map(|item| item.split_once(':'))
            .find(|(name, _)| name.trim().eq_ignore_ascii_case(property))
            .map(|(_, value)| value.trim().trim_end_matches("!important").trim_end())
            .filter(|value| !value.is_empty())
    }

    /// Match a comma-separated list of compound selectors. Each compound can
    /// include a tag name, classes, an ID, and attribute conditions.
    pub fn matches(&self, selectors: &str) -> bool {
        selectors.split(',').any(|selector| self.matches_compound(selector.trim()))
    }

    fn matches_compound(&self, mut selector: &str) -> bool {
        let suffix = selector.find(['.', '#', '[']).unwrap_or(selector.len());
        let tag = &selector[..suffix];
        if !tag.is_empty() && tag != "*" && !tag.eq_ignore_ascii_case(&self.name) {
            return false;
        }
        selector = &selector[suffix..];
        while !selector.is_empty() {
            if let Some(attribute) = selector.strip_prefix('[') {
                let Some((clause, rest)) = attribute.split_once(']') else { return false };
                let matched = if let Some((name, required)) = clause.split_once('=') {
                    self.attr(name.trim()).is_some_and(|actual| {
                        actual == required.trim().trim_matches(['\'', '"'])
                    })
                } else {
                    self.attr(clause.trim()).is_some()
                };
                if !matched {
                    return false;
                }
                selector = rest;
            } else {
                let prefix = selector.as_bytes()[0];
                if prefix != b'.' && prefix != b'#' {
                    return false;
                }
                let next = selector[1..].find(['.', '#', '[']).map_or(selector.len(), |index| index + 1);
                let name = &selector[1..next];
                let matches = if prefix == b'.' {
                    self.attr("class").is_some_and(|classes| classes.split_whitespace().any(|class| class == name))
                } else {
                    self.attr("id") == Some(name)
                };
                if !matches {
                    return false;
                }
                selector = &selector[next..];
            }
        }
        true
    }

    pub(crate) fn find_path(&self, selector: &str) -> Option<Vec<usize>> {
        for (index, child) in self.children.iter().enumerate() {
            if let Some(element) = child.as_element() {
                if element.matches(selector) {
                    return Some(vec![index]);
                }
                if let Some(path) = element.find_path(selector) {
                    let mut full = vec![index];
                    full.extend(path);
                    return Some(full);
                }
            }
        }
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomAttribute {
    pub name: String,
    pub value: String,
    pub namespace: Option<String>,
}

impl DomAttribute {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            namespace: None,
        }
    }
}
