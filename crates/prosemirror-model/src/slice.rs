use serde::{Deserialize, Serialize};
use crate::node::Fragment;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slice {
    pub content: Fragment,
    #[serde(rename = "openStart", default)]
    pub open_start: usize,
    #[serde(rename = "openEnd", default)]
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
        let full = self.content.size();
        if full >= self.open_start + self.open_end {
            full - self.open_start - self.open_end
        } else {
            0
        }
    }
}

// Custom Fragment serialization for slice/content
impl Serialize for Fragment {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.children.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Fragment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let children = Vec::<crate::node::Node>::deserialize(deserializer)?;
        Ok(Fragment::from_nodes(children))
    }
}
