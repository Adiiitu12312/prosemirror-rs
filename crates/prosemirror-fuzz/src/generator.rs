use prosemirror_model::{Fragment, Mark, Node, Slice};
use prosemirror_test_builder::test_schema;
use prosemirror_transform::Step;

pub struct RandomGen {
    state: u64,
}

impl RandomGen {
    pub fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    pub fn next_u32(&mut self) -> u32 {
        // Simple Xorshift PRNG
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x as u32
    }

    pub fn gen_range(&mut self, min: usize, max: usize) -> usize {
        if min >= max {
            return min;
        }
        let diff = (max - min) as u32;
        min + (self.next_u32() % diff) as usize
    }

    pub fn random_word(&mut self) -> String {
        let words = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta"];
        words[self.gen_range(0, words.len())].to_string()
    }

    pub fn random_marks(&mut self) -> Vec<Mark> {
        let mut marks = Vec::new();
        if self.next_u32().is_multiple_of(3) {
            marks.push(test_schema().mark_type("em").unwrap().create(None));
        }
        if self.next_u32().is_multiple_of(4) {
            marks.push(test_schema().mark_type("strong").unwrap().create(None));
        }
        marks
    }

    pub fn random_doc(&mut self) -> Node {
        let num_paras = self.gen_range(1, 4);
        let mut paras = Vec::new();

        for _ in 0..num_paras {
            let num_texts = self.gen_range(1, 4);
            let mut texts = Vec::new();
            for _ in 0..num_texts {
                let w = self.random_word();
                let m = self.random_marks();
                texts.push(Node::new_text(w, m));
            }
            let p = test_schema()
                .node("paragraph", None, Some(Fragment::from_nodes(texts)), None)
                .unwrap();
            paras.push(p);
        }

        test_schema()
            .node("doc", None, Some(Fragment::from_nodes(paras)), None)
            .unwrap()
    }

    pub fn random_step(&mut self, doc: &Node) -> Step {
        let size = doc.content().size();
        if size == 0 {
            let word = self.random_word();
            let slice = Slice::new(
                Fragment::from_nodes(vec![Node::new_text(word, Vec::new())]),
                0,
                0,
            );
            return Step::replace(0, 0, slice);
        }

        let p1 = self.gen_range(0, size);
        let p2 = self.gen_range(p1, size + 1);

        match self.next_u32() % 3 {
            0 => {
                // Replace / Insert / Delete
                let word = self.random_word();
                let slice = if self.next_u32().is_multiple_of(3) {
                    Slice::empty()
                } else {
                    Slice::new(
                        Fragment::from_nodes(vec![Node::new_text(word, Vec::new())]),
                        0,
                        0,
                    )
                };
                Step::replace(p1, p2, slice)
            }
            1 => {
                let mark = test_schema().mark_type("em").unwrap().create(None);
                Step::add_mark(p1, p2, mark)
            }
            _ => {
                let mark = test_schema().mark_type("em").unwrap().create(None);
                Step::remove_mark(p1, p2, mark)
            }
        }
    }
}
