use crate::error::ModelError;
use crate::mark::Mark;
use crate::node::Node;

#[derive(Clone, Debug)]
pub struct PathEntry {
    pub node: Node,
    pub index: usize,
    pub offset: usize,
}

#[derive(Clone, Debug)]
pub struct ResolvedPos {
    pub pos: usize,
    pub depth: usize,
    pub path: Vec<PathEntry>,
    pub parent_offset: usize,
}

impl ResolvedPos {
    pub fn resolve(doc: &Node, pos: usize) -> Result<Self, ModelError> {
        let doc_size = doc.content().size();
        if pos > doc_size {
            return Err(ModelError::OutOfBounds(pos, doc_size));
        }

        let mut path = Vec::new();
        let mut start = 0;
        let mut parent_offset = pos;
        let mut cur_node = doc.clone();

        loop {
            let (index, offset) = cur_node.content().find_index(parent_offset);
            let rem = parent_offset - offset;
            path.push(PathEntry {
                node: cur_node.clone(),
                index,
                offset: start + offset,
            });

            if rem == 0 {
                break;
            }

            if index >= cur_node.child_count() {
                break;
            }

            let next_child = cur_node.child(index).clone();
            if next_child.is_text() {
                break;
            }

            cur_node = next_child;
            parent_offset = rem - 1;
            start += offset + 1;
        }

        let depth = path.len().saturating_sub(1);
        Ok(Self {
            pos,
            depth,
            path,
            parent_offset,
        })
    }

    pub fn node(&self, depth: usize) -> &Node {
        let d = depth.min(self.depth);
        &self.path[d].node
    }

    pub fn parent(&self) -> &Node {
        self.node(self.depth)
    }

    pub fn doc(&self) -> &Node {
        self.node(0)
    }

    pub fn index(&self, depth: usize) -> usize {
        let d = depth.min(self.depth);
        self.path[d].index
    }

    pub fn index_after(&self, depth: usize) -> usize {
        let d = depth.min(self.depth);
        self.index(d) + if d == self.depth && self.text_offset() == 0 { 0 } else { 1 }
    }

    pub fn start(&self, depth: usize) -> usize {
        let d = depth.min(self.depth);
        if d == 0 {
            0
        } else {
            self.path[d - 1].offset + 1
        }
    }

    pub fn end(&self, depth: usize) -> usize {
        let d = depth.min(self.depth);
        self.start(d) + self.node(d).content().size()
    }

    pub fn before(&self, depth: usize) -> usize {
        if depth == self.depth + 1 {
            return self.pos;
        }
        let d = depth.min(self.depth);
        if d == 0 {
            0
        } else {
            self.path[d - 1].offset
        }
    }

    pub fn after(&self, depth: usize) -> usize {
        if depth == self.depth + 1 {
            return self.pos;
        }
        let d = depth.min(self.depth);
        if d == 0 {
            self.doc().content().size()
        } else {
            self.path[d - 1].offset + self.path[d].node.node_size()
        }
    }

    pub fn text_offset(&self) -> usize {
        let last_offset = self.path.last().map(|e| e.offset).unwrap_or(0);
        self.pos.saturating_sub(last_offset)
    }

    pub fn node_after(&self) -> Option<Node> {
        let parent = self.parent();
        let idx = self.index(self.depth);
        if idx >= parent.child_count() {
            return None;
        }
        let d_off = self.text_offset();
        let child = parent.child(idx);
        if d_off > 0 {
            Some(child.cut(d_off, child.node_size()))
        } else {
            Some(child.clone())
        }
    }

    pub fn node_before(&self) -> Option<Node> {
        let parent = self.parent();
        let idx = self.index(self.depth);
        let d_off = self.text_offset();
        if d_off > 0 && idx < parent.child_count() {
            let child = parent.child(idx);
            return Some(child.cut(0, d_off));
        }
        if idx > 0 {
            Some(parent.child(idx - 1).clone())
        } else {
            None
        }
    }

    pub fn pos_at_index(&self, index: usize, depth: usize) -> usize {
        self.start(depth)
            + self.node(depth).content().children[..index].iter().map(Node::node_size).sum::<usize>()
    }

    pub fn shared_depth(&self, pos: usize) -> usize {
        for depth in (1..=self.depth).rev() {
            if self.start(depth) <= pos && self.end(depth) >= pos {
                return depth;
            }
        }
        0
    }

    pub fn marks(&self) -> Vec<Mark> {
        let parent = self.parent();
        let idx = self.index(self.depth);
        if parent.child_count() == 0 {
            return Vec::new();
        }
        if self.text_offset() > 0 && idx < parent.child_count() {
            return parent.child(idx).marks().to_vec();
        }
        let before = idx.checked_sub(1).and_then(|index| parent.maybe_child(index));
        let after = parent.maybe_child(idx);
        let (main, other) = if before.is_some() {
            (before, after)
        } else {
            (after, before)
        };
        main.map_or_else(Vec::new, |main| Self::shared_marks(main, other))
    }

    pub fn marks_across(&self, end: &Self) -> Option<Vec<Mark>> {
        let after = self.parent().maybe_child(self.index(self.depth)).filter(|node| node.is_inline())?;
        let next = end.parent().maybe_child(end.index(end.depth));
        Some(Self::shared_marks(after, next))
    }

    fn shared_marks(main: &Node, other: Option<&Node>) -> Vec<Mark> {
        main.marks().iter().filter(|mark| {
            mark.is_inclusive() || other.is_some_and(|node| mark.is_in_set(node.marks()))
        }).cloned().collect()
    }

    pub fn same_depth(&self, other: &ResolvedPos) -> usize {
        let max_d = self.depth.min(other.depth);
        for d in 0..=max_d {
            if self.start(d) != other.start(d) {
                return d.saturating_sub(1);
            }
        }
        max_d
    }

    pub fn same_parent(&self, other: &ResolvedPos) -> bool {
        self.pos.saturating_sub(self.parent_offset) == other.pos.saturating_sub(other.parent_offset)
    }

    pub fn block_range(&self, other: &ResolvedPos) -> Option<NodeRange> {
        self.block_range_with(other, |_| true)
    }

    pub fn block_range_with(
        &self,
        other: &ResolvedPos,
        mut predicate: impl FnMut(&Node) -> bool,
    ) -> Option<NodeRange> {
        let (from, to) = if self.pos <= other.pos { (self, other) } else { (other, self) };
        let start_depth = from.depth.checked_sub(usize::from(from.parent().inline_content() || from.pos == to.pos))?;
        for depth in (0..=start_depth).rev() {
            if to.pos <= from.end(depth) && predicate(from.node(depth)) {
                return Some(NodeRange {
                    start: from.before(depth + 1),
                    end: to.after(depth + 1),
                    depth,
                    start_index: from.index(depth),
                    end_index: to.index_after(depth),
                });
            }
        }
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeRange {
    pub start: usize,
    pub end: usize,
    pub depth: usize,
    pub start_index: usize,
    pub end_index: usize,
}
