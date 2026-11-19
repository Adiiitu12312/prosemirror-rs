use std::sync::Arc;

use prosemirror_model::Node;
use prosemirror_transform::{can_join, find_wrapping};
use serde_json::Value;

use crate::{InputMatch, InputRule, InputRuleOptions};

type AttributeBuilder = dyn Fn(&InputMatch<'_>) -> Option<Value> + Send + Sync;

pub enum Attrs {
    Fixed(Option<Value>),
    Dynamic(Arc<AttributeBuilder>),
}

impl Default for Attrs {
    fn default() -> Self {
        Self::Fixed(None)
    }
}

impl Attrs {
    pub fn dynamic(handler: impl Fn(&InputMatch<'_>) -> Option<Value> + Send + Sync + 'static) -> Self {
        Self::Dynamic(Arc::new(handler))
    }

    fn resolve(&self, matched: &InputMatch<'_>) -> Option<Value> {
        match self {
            Self::Fixed(attrs) => attrs.clone(),
            Self::Dynamic(compute) => compute(matched),
        }
    }
}

pub type JoinPredicate = Arc<dyn Fn(&InputMatch<'_>, &Node) -> bool + Send + Sync>;

pub fn wrapping_input_rule(
    pattern: &str,
    flags: &str,
    type_name: impl Into<String>,
    attrs: Attrs,
    join: Option<JoinPredicate>,
) -> Result<InputRule, regress::Error> {
    let type_name = type_name.into();
    InputRule::new(pattern, flags, InputRuleOptions::default(), move |state, matched, start, end| {
        let mut tr = state.tr();
        tr.delete(start, end);
        let pos = tr.doc().resolve(start).ok()?;
        let range = pos.block_range(&pos)?;
        let wrapping = find_wrapping(tr.doc(), &range, &type_name, attrs.resolve(matched), None)?;
        tr.apply_transform(|transform| { transform.wrap(&range, &wrapping); });
        if let Some(before_pos) = start.checked_sub(1) {
            if let Some(before) = tr.doc().resolve(before_pos).ok()?.node_before() {
                if before.type_name() == type_name
                    && can_join(tr.doc(), before_pos)
                    && join.as_ref().is_none_or(|predicate| predicate(matched, &before)) {
                    tr.apply_transform(|transform| { transform.join(before_pos, 1); });
                }
            }
        }
        Some(tr)
    })
}

pub fn textblock_type_input_rule(
    pattern: &str,
    flags: &str,
    type_name: impl Into<String>,
    attrs: Attrs,
) -> Result<InputRule, regress::Error> {
    let type_name = type_name.into();
    InputRule::new(pattern, flags, InputRuleOptions::default(), move |state, matched, start, end| {
        let pos = state.doc.resolve(start).ok()?;
        let depth = pos.depth.checked_sub(1)?;
        let parent = pos.node(depth);
        if !parent.can_replace_with(pos.index(depth), pos.index_after(depth), &type_name, &[]) {
            return None;
        }
        let attrs = attrs.resolve(matched);
        let mut tr = state.tr();
        tr.delete(start, end);
        tr.apply_transform(|transform| { transform.set_block_type(start, start, &type_name, attrs); });
        Some(tr)
    })
}
