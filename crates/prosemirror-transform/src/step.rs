use serde::{Deserialize, Serialize};
use prosemirror_model::{replace_at_pos, Mark, Node, Slice};
use crate::map::StepMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepResult {
    Ok(Node),
    Failed(String),
}

impl StepResult {
    pub fn ok(doc: Node) -> Self {
        Self::Ok(doc)
    }

    pub fn fail(msg: impl Into<String>) -> Self {
        Self::Failed(msg.into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stepType")]
pub enum Step {
    #[serde(rename = "replace")]
    Replace {
        from: usize,
        to: usize,
        slice: Slice,
    },
    #[serde(rename = "addMark")]
    AddMark {
        from: usize,
        to: usize,
        mark: Mark,
    },
    #[serde(rename = "removeMark")]
    RemoveMark {
        from: usize,
        to: usize,
        mark: Mark,
    },
}

impl Step {
    pub fn replace(from: usize, to: usize, slice: Slice) -> Self {
        Self::Replace { from, to, slice }
    }

    pub fn add_mark(from: usize, to: usize, mark: Mark) -> Self {
        Self::AddMark { from, to, mark }
    }

    pub fn remove_mark(from: usize, to: usize, mark: Mark) -> Self {
        Self::RemoveMark { from, to, mark }
    }

    pub fn apply(&self, doc: &Node) -> StepResult {
        match self {
            Self::Replace { from, to, slice } => {
                let doc_size = doc.content().size();
                if *from > *to || *to > doc_size {
                    return StepResult::fail("Position out of bounds");
                }
                match replace_at_pos(doc, *from, *to, slice) {
                    Ok(new_doc) => StepResult::ok(new_doc),
                    Err(e) => StepResult::fail(e.to_string()),
                }
            }
            Self::AddMark { from, to, mark } => {
                let doc_size = doc.content().size();
                if *from > *to || *to > doc_size {
                    return StepResult::fail("Position out of bounds");
                }
                let new_doc = transform_marks(doc, *from, *to, |marks| {
                    Mark::add_to_set(marks, mark.clone())
                });
                StepResult::ok(new_doc)
            }
            Self::RemoveMark { from, to, mark } => {
                let doc_size = doc.content().size();
                if *from > *to || *to > doc_size {
                    return StepResult::fail("Position out of bounds");
                }
                let new_doc = transform_marks(doc, *from, *to, |marks| {
                    Mark::remove_from_set(marks, &mark.type_name)
                });
                StepResult::ok(new_doc)
            }
        }
    }

    pub fn get_map(&self) -> StepMap {
        match self {
            Self::Replace { from, to, slice } => {
                StepMap::single(*from, *to - *from, slice.size())
            }
            Self::AddMark { .. } | Self::RemoveMark { .. } => StepMap::empty(),
        }
    }

    pub fn invert(&self, doc: &Node) -> Self {
        match self {
            Self::Replace { from, to, slice } => {
                let doc_slice = doc.content().cut(*from, *to);
                Self::Replace {
                    from: *from,
                    to: *from + slice.size(),
                    slice: Slice::new(doc_slice, 0, 0),
                }
            }
            Self::AddMark { from, to, mark } => Self::RemoveMark {
                from: *from,
                to: *to,
                mark: mark.clone(),
            },
            Self::RemoveMark { from, to, mark } => Self::AddMark {
                from: *from,
                to: *to,
                mark: mark.clone(),
            },
        }
    }
}

fn transform_marks<F>(node: &Node, from: usize, to: usize, f: F) -> Node
where
    F: Fn(&[Mark]) -> Vec<Mark> + Copy,
{
    if node.is_text() {
        let new_marks = f(node.marks());
        return Node::new_text(node.text().unwrap_or(""), new_marks);
    }

    let mut new_children = Vec::new();
    let mut cur = 0;
    for child in &node.content().children {
        let end = cur + child.node_size();
        if end > from && cur < to {
            let child_from = if from > cur { from - cur } else { 0 };
            let child_to = if to < end { to - cur } else { child.node_size() };
            if child.is_text() {
                let text = child.text().unwrap_or("");
                let chars: Vec<char> = text.chars().collect();
                if child_from > 0 {
                    let pre: String = chars[..child_from].iter().collect();
                    new_children.push(Node::new_text(pre, child.marks().to_vec()));
                }
                let mid: String = chars[child_from..child_to].iter().collect();
                new_children.push(Node::new_text(mid, f(child.marks())));
                if child_to < chars.len() {
                    let post: String = chars[child_to..].iter().collect();
                    new_children.push(Node::new_text(post, child.marks().to_vec()));
                }
            } else {
                let inner_from = if child_from > 0 { child_from - 1 } else { 0 };
                let inner_to = if child_to < child.node_size() { child_to - 1 } else { child.content().size() };
                new_children.push(transform_marks(child, inner_from, inner_to, f));
            }
        } else {
            new_children.push(child.clone());
        }
        cur = end;
    }
    node.copy(Some(prosemirror_model::Fragment::from_nodes(new_children)))
}
