use crate::{InputRule, InputRuleOptions};

fn punctuation(pattern: &str, replacement: &str) -> InputRule {
    let options = InputRuleOptions { in_code_mark: false, ..InputRuleOptions::default() };
    InputRule::replace(pattern, "", replacement, options).expect("valid punctuation pattern")
}

pub fn em_dash() -> InputRule {
    punctuation(r"--$", "—")
}

pub fn ellipsis() -> InputRule {
    punctuation(r"\.\.\.$", "…")
}

pub fn open_double_quote() -> InputRule {
    punctuation(r#"(?:^|[\s\{\[\(\<'"\u2018\u201C])(")$"#, "“")
}

pub fn close_double_quote() -> InputRule {
    punctuation(r#""$"#, "”")
}

pub fn open_single_quote() -> InputRule {
    punctuation(r#"(?:^|[\s\{\[\(\<'"\u2018\u201C])(')$"#, "‘")
}

pub fn close_single_quote() -> InputRule {
    punctuation(r"'$", "’")
}

pub fn smart_quotes() -> [InputRule; 4] {
    [open_double_quote(), close_double_quote(), open_single_quote(), close_single_quote()]
}
