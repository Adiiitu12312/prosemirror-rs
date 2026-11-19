use prosemirror_model::{Fragment, Node, Slice};
use prosemirror_transform::{ReplaceMapBias, Step, StepMap};

#[test]
fn insertion_bias_controls_mapping_at_a_simultaneous_insertion() {
    struct RestoreBias(ReplaceMapBias);
    impl Drop for RestoreBias {
        fn drop(&mut self) {
            Step::set_replace_map_bias(self.0);
        }
    }

    let step = Step::replace(5, 5, Slice::new(Fragment::from_nodes(vec![Node::new_text("x", Vec::new())]), 0, 0));
    let mapping = StepMap::single(5, 0, 3);
    assert!(matches!(step.map(&mapping), Some(Step::Replace { from: 8, to: 8, .. })));

    let _restore = RestoreBias(Step::set_replace_map_bias(ReplaceMapBias::Before));
    assert!(matches!(step.map(&mapping), Some(Step::Replace { from: 5, to: 5, .. })));
}
