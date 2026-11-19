//! Port of prosemirror-example-setup@1.2.3/src/inputrules.ts.

use std::sync::Arc;

use prosemirror_inputrules::{
    ellipsis, em_dash, input_rules, smart_quotes, textblock_type_input_rule, wrapping_input_rule,
    Attrs, InputRule, InputRuleError,
};
use prosemirror_model::Schema;
use prosemirror_state::Plugin;
use serde_json::json;

pub fn blockquote_rule(node: &str) -> InputRule {
    wrapping_input_rule(r"^\s*>\s$", "", node, Attrs::default(), None).expect("valid blockquote pattern")
}

pub fn ordered_list_rule(node: &str) -> InputRule {
    wrapping_input_rule(
        r"^(\d+)\.\s$", "", node,
        Attrs::dynamic(|matched| {
            let order = matched.group(1)?.parse::<u64>().ok()?;
            Some(json!({"order": order}))
        }),
        Some(Arc::new(|matched, node| {
            let order = node.attrs().and_then(|attrs| attrs["order"].as_u64()).unwrap_or(1);
            let next = order.saturating_add(node.child_count() as u64);
            matched.group(1).and_then(|group| group.parse().ok()) == Some(next)
        })),
    ).expect("valid ordered list pattern")
}

pub fn bullet_list_rule(node: &str) -> InputRule {
    wrapping_input_rule(r"^\s*([-+*])\s$", "", node, Attrs::default(), None).expect("valid bullet list pattern")
}

pub fn code_block_rule(node: &str) -> InputRule {
    textblock_type_input_rule(r"^\x60{3}$", "", node, Attrs::default()).expect("valid code block pattern")
}

pub fn heading_rule(node: &str, max_level: u8) -> Result<InputRule, InputRuleError> {
    textblock_type_input_rule(
        &format!(r"^(#{{1,{max_level}}})\s$"), "", node,
        Attrs::dynamic(|matched| Some(json!({"level": matched.group(1)?.len()}))),
    )
}

pub fn build_input_rules(schema: &Schema) -> Plugin {
    let mut rules = Vec::from(smart_quotes());
    rules.extend([ellipsis(), em_dash()]);
    for (name, create) in [
        ("blockquote", blockquote_rule as fn(&str) -> InputRule),
        ("ordered_list", ordered_list_rule),
        ("bullet_list", bullet_list_rule),
        ("code_block", code_block_rule),
    ] {
        if schema.node_type(name).is_some() {
            rules.push(create(name));
        }
    }
    if schema.node_type("heading").is_some() {
        rules.push(heading_rule("heading", 6).expect("valid heading pattern"));
    }
    input_rules(rules)
}
