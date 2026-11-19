//! Port of prosemirror-inputrules 1.5.0.

mod inputrules;
mod rulebuilders;
mod rules;

pub use regress::Error as InputRuleError;
pub use inputrules::{input_rules, undo_input_rule, InCode, InputMatch, InputRule, InputRuleOptions};
pub use rulebuilders::{textblock_type_input_rule, wrapping_input_rule, Attrs, JoinPredicate};
pub use rules::{
    close_double_quote, close_single_quote, ellipsis, em_dash, open_double_quote,
    open_single_quote, smart_quotes,
};
