use prosemirror_model::{Fragment, Mark, Node, Slice};
use crate::map::Mapping;
use crate::step::{Step, StepResult};

#[derive(Clone, Debug)]
pub struct Transform {
    pub doc: Node,
    pub steps: Vec<Step>,
    pub docs: Vec<Node>,
    pub mapping: Mapping,
}

impl Transform {
    pub fn new(doc: Node) -> Self {
        Self {
            doc,
            steps: Vec::new(),
            docs: Vec::new(),
            mapping: Mapping::new(),
        }
    }

    pub fn step(&mut self, step: Step) -> Result<(), String> {
        match step.apply(&self.doc) {
            StepResult::Ok(new_doc) => {
                self.docs.push(self.doc.clone());
                self.mapping.append_map(step.get_map(), None);
                self.steps.push(step);
                self.doc = new_doc;
                Ok(())
            }
            StepResult::Failed(err) => Err(err),
        }
    }

    pub fn maybe_step(&mut self, step: Step) -> &mut Self {
        let _ = self.step(step);
        self
    }

    pub fn replace(&mut self, from: usize, to: usize, slice: Slice) -> &mut Self {
        let step = Step::replace(from, to, slice);
        self.maybe_step(step)
    }

    pub fn replace_with(&mut self, from: usize, to: usize, content: Fragment) -> &mut Self {
        self.replace(from, to, Slice::new(content, 0, 0))
    }

    pub fn delete(&mut self, from: usize, to: usize) -> &mut Self {
        self.replace(from, to, Slice::empty())
    }

    pub fn insert(&mut self, pos: usize, content: Fragment) -> &mut Self {
        self.replace_with(pos, pos, content)
    }

    pub fn add_mark(&mut self, from: usize, to: usize, mark: Mark) -> &mut Self {
        let step = Step::add_mark(from, to, mark);
        self.maybe_step(step)
    }

    pub fn remove_mark(&mut self, from: usize, to: usize, mark: Mark) -> &mut Self {
        let step = Step::remove_mark(from, to, mark);
        self.maybe_step(step)
    }

    pub fn split(&mut self, pos: usize, depth: usize) -> &mut Self {
        if depth == 0 {
            return self;
        }
        let rpos = match self.doc.resolve(pos) {
            Ok(r) => r,
            Err(_) => return self,
        };
        // Creating split boundary
        let parent = rpos.parent();
        let slice = Slice::new(
            Fragment::from_nodes(vec![
                parent.copy(Some(Fragment::empty())),
                parent.copy(Some(Fragment::empty())),
            ]),
            1,
            1,
        );
        self.replace(pos, pos, slice)
    }

    pub fn join(&mut self, pos: usize, _depth: usize) -> &mut Self {
        // Joining merges two sibling blocks by deleting the boundary (2 units: closing tag of left + opening tag of right)
        if pos >= 1 {
            self.delete(pos - 1, pos + 1);
        }
        self
    }
}
