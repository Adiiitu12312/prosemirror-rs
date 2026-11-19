use std::sync::{Arc, Mutex};

use prosemirror_model::Fragment;
use prosemirror_state::{EditorState, Plugin, PluginKey, StateField, Transaction};
use prosemirror_transform::Step;
use regress::{Flags, Match, Regex};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_MATCH: usize = 500;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InCode {
    #[default]
    Never,
    Always,
    Only,
}

#[derive(Clone, Copy, Debug)]
pub struct InputRuleOptions {
    pub undoable: bool,
    pub in_code: InCode,
    pub in_code_mark: bool,
}

impl Default for InputRuleOptions {
    fn default() -> Self {
        Self { undoable: true, in_code: InCode::Never, in_code_mark: true }
    }
}

pub struct InputMatch<'a> {
    input: &'a [u16],
    matched: Match,
}

impl InputMatch<'_> {
    pub fn index(&self) -> usize {
        self.matched.start()
    }

    pub fn text(&self) -> String {
        String::from_utf16_lossy(&self.input[self.matched.range()])
    }

    pub fn group(&self, index: usize) -> Option<String> {
        self.group_units(index).map(String::from_utf16_lossy)
    }

    pub fn named_group(&self, name: &str) -> Option<String> {
        self.matched.named_group(name).map(|range| String::from_utf16_lossy(&self.input[range]))
    }

    fn group_units(&self, index: usize) -> Option<&[u16]> {
        self.matched.group(index).map(|range| &self.input[range])
    }
}

type Handler = dyn Fn(&EditorState, &InputMatch<'_>, usize, usize, &str) -> Option<Transaction> + Send + Sync;

#[derive(Clone)]
pub struct InputRule {
    matcher: Arc<Matcher>,
    handler: Arc<Handler>,
    pub options: InputRuleOptions,
}

struct Matcher {
    regex: Regex,
    unicode: bool,
    sticky: bool,
    last_index: Option<Mutex<usize>>,
}

impl Matcher {
    fn find<'a>(&self, text: &'a [u16]) -> Option<InputMatch<'a>> {
        let mut last_index = self.last_index.as_ref().map(|index| index.lock().unwrap());
        let start = last_index.as_deref().copied().unwrap_or_default();
        let matched = if start > text.len() {
            None
        } else if self.unicode {
            self.regex.find_from_utf16(text, start).next()
        } else {
            self.regex.find_from_ucs2(text, start).next()
        }.filter(|matched| !self.sticky || matched.start() == start);
        if let Some(index) = last_index.as_deref_mut() {
            *index = matched.as_ref().map_or(0, Match::end);
        }
        matched.map(|matched| InputMatch { input: text, matched })
    }
}

impl InputRule {
    pub fn new(
        pattern: &str,
        flags: &str,
        options: InputRuleOptions,
        handler: impl Fn(&EditorState, &InputMatch<'_>, usize, usize) -> Option<Transaction>
            + Send + Sync + 'static,
    ) -> Result<Self, regress::Error> {
        Self::new_with_text(pattern, flags, options, move |state, matched, start, end, _| {
            handler(state, matched, start, end)
        })
    }

    /// Construct a rule whose handler also receives the text that triggered it.
    pub fn new_with_text(
        pattern: &str,
        flags: &str,
        options: InputRuleOptions,
        handler: impl Fn(&EditorState, &InputMatch<'_>, usize, usize, &str) -> Option<Transaction>
            + Send + Sync + 'static,
    ) -> Result<Self, regress::Error> {
        let parsed = Flags::from(flags);
        let unicode = parsed.unicode || parsed.unicode_sets;
        let sticky = flags.contains('y');
        let regex = if unicode {
            Regex::with_flags(pattern, parsed)?
        } else {
            Regex::from_unicode(pattern.encode_utf16().map(u32::from), parsed)?
        };
        Ok(Self {
            matcher: Arc::new(Matcher {
                regex,
                unicode,
                sticky,
                last_index: (sticky || flags.contains('g')).then(|| Mutex::new(0)),
            }),
            handler: Arc::new(handler),
            options,
        })
    }

    pub fn replace(
        pattern: &str,
        flags: &str,
        replacement: impl Into<String>,
        options: InputRuleOptions,
    ) -> Result<Self, regress::Error> {
        let replacement = replacement.into();
        Self::new(pattern, flags, options, move |state, matched, start, end| {
            Some(string_handler(state, matched, start, end, &replacement))
        })
    }
}

fn string_handler(
    state: &EditorState,
    matched: &InputMatch<'_>,
    mut start: usize,
    end: usize,
    replacement: &str,
) -> Transaction {
    let mut insert = replacement.to_owned();
    if let Some(group) = matched.group_units(1).filter(|group| !group.is_empty()) {
        let whole = &matched.input[matched.matched.range()];
        if let Some(offset) = whole.windows(group.len()).rposition(|part| part == group) {
            insert.push_str(&String::from_utf16_lossy(&whole[offset + group.len()..]));
            start += offset;
            if start > end {
                let cutoff = start - end;
                insert.insert_str(0, &String::from_utf16_lossy(&whole[offset - cutoff..offset]));
                start = end;
            }
        }
    }
    let mut tr = state.tr();
    tr.insert_text(&insert, start, end);
    tr
}

#[derive(Deserialize, Serialize)]
struct UndoRecord {
    inverted: Vec<Value>,
    from: usize,
    to: usize,
    text: String,
}

struct InputRulesField(PluginKey);

impl StateField for InputRulesField {
    fn init(&self, _config: &Value) -> Value {
        Value::Null
    }

    fn apply(&self, tr: &Transaction, prev: &Value) -> Value {
        if let Some(record) = tr.get_meta(&self.0) {
            return record.clone();
        }
        if tr.selection_set() || tr.doc_changed() {
            Value::Null
        } else {
            prev.clone()
        }
    }
}

pub fn input_rules(rules: Vec<InputRule>) -> Plugin {
    let key = PluginKey::new("plugin");
    let rules: Arc<[InputRule]> = rules.into();
    let text_key = key.clone();
    let text_rules = Arc::clone(&rules);
    let composition_key = key.clone();
    let mut plugin = Plugin::new(key.clone(), Some(Arc::new(InputRulesField(key))))
        .with_text_input(move |state, from, to, text, composing| {
            run(state, from, to, text, composing, &text_rules, &text_key)
        })
        .with_composition_end(move |state| {
            let cursor = state.selection.cursor_pos()?;
            run(state, cursor, cursor, "", false, &rules, &composition_key)
        });
    plugin.spec.insert("isInputRules".into(), Value::Bool(true));
    plugin
}

fn run(
    state: &EditorState,
    from: usize,
    to: usize,
    text: &str,
    composing: bool,
    rules: &[InputRule],
    key: &PluginKey,
) -> Option<Transaction> {
    if composing || from > to || to > state.doc.content().size() {
        return None;
    }
    let pos = state.doc.resolve(from).ok()?;
    let in_code = pos.parent().node_type().is_some_and(|kind| kind.spec.code);
    let code_mark = |mark: &prosemirror_model::Mark| {
        state.schema.mark_type(&mark.type_name).is_some_and(|kind| kind.spec.code)
    };
    let stored_marks = if state.selection.from() == from && state.selection.to(&state.doc) == to {
        state.stored_marks.as_deref()
    } else {
        None
    };
    let in_code_mark = if let Some(marks) = stored_marks {
        marks.iter().any(code_mark)
    } else if from == to {
        pos.marks().iter().any(code_mark)
    } else {
        let end = state.doc.resolve(to).ok()?;
        pos.marks_across(&end).is_some_and(|marks| marks.iter().any(code_mark))
    };
    let text_before = pos.parent().text_between_with(
        pos.parent_offset.saturating_sub(MAX_MATCH),
        pos.parent_offset,
        None,
        Some(&|_| "\u{fffc}".to_owned()),
    );
    let input = text_before.encode_utf16().chain(text.encode_utf16()).collect::<Vec<_>>();
    let inserted_len = text.encode_utf16().count();
    for rule in rules {
        if (in_code_mark && !rule.options.in_code_mark)
            || (in_code && rule.options.in_code == InCode::Never)
            || (!in_code && rule.options.in_code == InCode::Only)
        {
            continue;
        }
        let Some(matched) = rule.matcher.find(&input) else {
            continue;
        };
        let Some(start) = matched.matched.range().len()
            .checked_sub(inserted_len).and_then(|length| from.checked_sub(length))
        else {
            continue;
        };
        if !rule.options.in_code_mark {
            let mut covered_code_mark = false;
            state.doc.nodes_between(start, from, |node, _, _, _| {
                covered_code_mark |= node.is_inline() && node.marks().iter().any(code_mark);
                !covered_code_mark
            });
            if covered_code_mark {
                continue;
            }
        }
        let Some(mut tr) = (rule.handler)(state, &matched, start, to, text) else {
            continue;
        };
        if rule.options.undoable {
            let inverted = tr.steps().iter().zip(&tr.transform.docs).rev()
                .map(|(step, doc)| serde_json::to_value(step.invert(doc)).expect("serializable inverse"))
                .collect();
            let record = UndoRecord { inverted, from, to, text: text.to_owned() };
            tr.set_meta(key, serde_json::to_value(record).expect("serializable rule state"));
        }
        return Some(tr);
    }
    None
}

pub fn undo_input_rule(state: &EditorState, mut dispatch: impl FnMut(Transaction)) -> bool {
    for plugin in &state.plugins {
        if plugin.spec.get("isInputRules").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let Some(record) = plugin.get_state(state) else {
            continue;
        };
        let Ok(record) = serde_json::from_value::<UndoRecord>(record.clone()) else {
            continue;
        };
        let mut tr = state.tr();
        for json in record.inverted {
            let Ok(step) = Step::from_json(&state.schema, &json) else {
                return false;
            };
            if tr.step(step).is_err() {
                return false;
            }
        }
        if record.text.is_empty() {
            tr.delete(record.from, record.to);
        } else {
            let Ok(pos) = tr.doc().resolve(record.from) else {
                return false;
            };
            let text = state.schema.text(&record.text, Some(pos.marks()));
            tr.replace_with(record.from, record.to, Fragment::from_nodes(vec![text]));
        }
        dispatch(tr);
        return true;
    }
    false
}
