use serde::{Deserialize, Serialize};
use prosemirror_model::Node;
use prosemirror_transform::Mapping;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Selection {
    #[serde(rename = "text")]
    Text {
        anchor: usize,
        head: usize,
    },
    #[serde(rename = "node")]
    Node {
        anchor: usize,
    },
    #[serde(rename = "all")]
    All,
}

impl Selection {
    pub fn cursor(pos: usize) -> Self {
        Self::Text {
            anchor: pos,
            head: pos,
        }
    }

    pub fn text(anchor: usize, head: usize) -> Self {
        Self::Text { anchor, head }
    }

    pub fn node(pos: usize) -> Self {
        Self::Node { anchor: pos }
    }

    pub fn all() -> Self {
        Self::All
    }

    pub fn from(&self) -> usize {
        match self {
            Self::Text { anchor, head } => (*anchor).min(*head),
            Self::Node { anchor } => *anchor,
            Self::All => 0,
        }
    }

    pub fn to(&self, doc: &Node) -> usize {
        match self {
            Self::Text { anchor, head } => (*anchor).max(*head),
            Self::Node { anchor } => {
                // Node size
                if let Ok(rpos) = doc.resolve(*anchor) {
                    let parent = rpos.parent();
                    let idx = rpos.index(rpos.depth);
                    if idx < parent.child_count() {
                        *anchor + parent.child(idx).node_size()
                    } else {
                        *anchor + 1
                    }
                } else {
                    *anchor + 1
                }
            }
            Self::All => doc.content().size(),
        }
    }

    pub fn anchor(&self) -> usize {
        match self {
            Self::Text { anchor, .. } => *anchor,
            Self::Node { anchor } => *anchor,
            Self::All => 0,
        }
    }

    pub fn head(&self, doc: &Node) -> usize {
        match self {
            Self::Text { head, .. } => *head,
            Self::Node { anchor } => *anchor,
            Self::All => doc.content().size(),
        }
    }

    pub fn empty(&self) -> bool {
        match self {
            Self::Text { anchor, head } => anchor == head,
            Self::Node { .. } => false,
            Self::All => false,
        }
    }

    pub fn map(&self, doc: &Node, mapping: &Mapping) -> Self {
        match self {
            Self::Text { anchor, head } => {
                let mapped_anchor = mapping.map(*anchor, 1);
                let mapped_head = mapping.map(*head, 1);
                let max = doc.content().size();
                Self::Text {
                    anchor: mapped_anchor.min(max),
                    head: mapped_head.min(max),
                }
            }
            Self::Node { anchor } => {
                let mapped = mapping.map(*anchor, 1);
                let max = doc.content().size();
                Self::Node {
                    anchor: mapped.min(max),
                }
            }
            Self::All => Self::All,
        }
    }
}
