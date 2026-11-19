use serde::{Deserialize, Serialize};

use crate::{Fragment, ModelError};

#[derive(Clone, Debug, PartialEq)]
pub struct Slice {
    pub content: Fragment,
    pub open_start: usize,
    pub open_end: usize,
}

impl Slice {
    pub fn empty() -> Self {
        Self {
            content: Fragment::empty(),
            open_start: 0,
            open_end: 0,
        }
    }

    pub fn new(content: Fragment, open_start: usize, open_end: usize) -> Self {
        Self {
            content,
            open_start,
            open_end,
        }
    }

    pub fn size(&self) -> usize {
        self.content.size().saturating_sub(self.open_start).saturating_sub(self.open_end)
    }

    pub fn max_open(content: Fragment, open_isolating: bool) -> Self {
        fn open(mut child: Option<&crate::Node>, isolating: bool, first: bool) -> usize {
            let mut depth = 0;
            while let Some(node) = child {
                if node.is_leaf() || !isolating && node.is_isolating() {
                    break;
                }
                depth += 1;
                child = if first { node.first_child() } else { node.last_child() };
            }
            depth
        }
        let start = open(content.first_child(), open_isolating, true);
        let end = open(content.last_child(), open_isolating, false);
        Self::new(content, start, end)
    }

    pub fn insert_at(&self, pos: usize, fragment: &Fragment) -> Option<Self> {
        insert_into(&self.content, pos.checked_add(self.open_start)?, fragment)
            .map(|content| Self::new(content, self.open_start, self.open_end))
    }

    pub fn remove_between(&self, from: usize, to: usize) -> Result<Self, ModelError> {
        let invalid = || ModelError::ReplaceError("Removing non-flat range".to_owned());
        let from = from.checked_add(self.open_start).ok_or_else(invalid)?;
        let to = to.checked_add(self.open_start).ok_or_else(invalid)?;
        remove_range(&self.content, from, to)
            .map(|content| Self::new(content, self.open_start, self.open_end))
    }
}

fn insert_into(content: &Fragment, pos: usize, insert: &Fragment) -> Option<Fragment> {
    if pos > content.size() {
        return None;
    }
    let (index, offset) = content.find_index(pos);
    let child = content.maybe_child(index);
    if offset == pos || child.is_some_and(crate::Node::is_text) {
        return Some(content.cut(0, pos).append(insert).append(&content.cut(pos, content.size())));
    }
    let child = child?;
    let inner = insert_into(child.content(), pos - offset - 1, insert)?;
    Some(content.replace_child(index, child.copy(Some(inner))))
}

fn remove_range(content: &Fragment, from: usize, to: usize) -> Result<Fragment, ModelError> {
    let invalid = || ModelError::ReplaceError("Removing non-flat range".to_owned());
    if from > to || to > content.size() {
        return Err(invalid());
    }
    let (index, offset) = content.find_index(from);
    let (end_index, end_offset) = content.find_index(to);
    let child = content.maybe_child(index);
    if offset == from || child.is_some_and(crate::Node::is_text) {
        if end_offset != to && !content.child(end_index).is_text() {
            return Err(invalid());
        }
        return Ok(content.cut(0, from).append(&content.cut(to, content.size())));
    }
    if index != end_index {
        return Err(invalid());
    }
    let child = child.ok_or_else(invalid)?;
    let inner = remove_range(child.content(), from - offset - 1, to - offset - 1)?;
    Ok(content.replace_child(index, child.copy(Some(inner))))
}

impl std::fmt::Display for Slice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}({},{})", self.content, self.open_start, self.open_end)
    }
}

impl Serialize for Slice {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        if self.content.size() == 0 {
            return serializer.serialize_none();
        }
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("content", &self.content)?;
        if self.open_start != 0 {
            map.serialize_entry("openStart", &self.open_start)?;
        }
        if self.open_end != 0 {
            map.serialize_entry("openEnd", &self.open_end)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Slice {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawSlice {
            #[serde(default)]
            content: Fragment,
            open_start: Option<usize>,
            open_end: Option<usize>,
        }

        Ok(Option::<RawSlice>::deserialize(deserializer)?.map_or_else(Self::empty, |slice| {
            Self::new(slice.content, slice.open_start.unwrap_or(0), slice.open_end.unwrap_or(0))
        }))
    }
}

impl Serialize for Fragment {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.children.is_empty() {
            serializer.serialize_none()
        } else {
            self.children.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Fragment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let children = Option::<Vec<crate::Node>>::deserialize(deserializer)?.unwrap_or_default();
        Ok(Fragment::from_children(children))
    }
}
