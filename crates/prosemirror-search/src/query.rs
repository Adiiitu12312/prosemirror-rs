//! Port of prosemirror-search 1.1.0, src/query.ts.

use std::ops::Range;
use std::sync::{Arc, OnceLock};

use prosemirror_model::{Fragment, Node, Slice};
use prosemirror_state::EditorState;
use regress::{Match, Regex};

pub type QueryFilter = Arc<dyn Fn(&EditorState, &SearchResult) -> bool + Send + Sync>;

#[derive(Clone, Default)]
pub struct SearchQueryConfig {
    pub search: String,
    pub case_sensitive: bool,
    pub literal: bool,
    pub regexp: bool,
    pub replace: String,
    pub whole_word: bool,
    pub filter: Option<QueryFilter>,
}

#[derive(Clone)]
pub struct SearchQuery {
    config: SearchQueryConfig,
    matcher: Matcher,
}

#[derive(Clone)]
enum Matcher {
    Invalid,
    String(Vec<u16>),
    Regexp(Regex),
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub from: usize,
    pub to: usize,
    pub matched: Option<RegexMatch>,
    pub match_start: usize,
}

#[derive(Clone, Debug)]
pub struct RegexMatch {
    input: Vec<u16>,
    inner: Match,
}

impl RegexMatch {
    pub fn index(&self) -> usize {
        self.inner.start()
    }

    pub fn input(&self) -> String {
        String::from_utf16_lossy(&self.input)
    }

    pub fn group(&self, index: usize) -> Option<String> {
        self.group_range(index)
            .map(|range| String::from_utf16_lossy(&self.input[range]))
    }

    pub fn group_range(&self, index: usize) -> Option<Range<usize>> {
        self.inner.group(index)
    }

    pub fn named_group(&self, name: &str) -> Option<String> {
        self.inner.named_group(name).map(|range| String::from_utf16_lossy(&self.input[range]))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Replacement {
    pub from: usize,
    pub to: usize,
    pub insert: Slice,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self::new(SearchQueryConfig::default())
    }
}

impl SearchQuery {
    pub fn new(config: SearchQueryConfig) -> Self {
        let matcher = if config.search.is_empty() {
            Matcher::Invalid
        } else if config.regexp {
            let flags = if config.case_sensitive { "u" } else { "iu" };
            Regex::with_flags(&config.search, flags).map_or(Matcher::Invalid, Matcher::Regexp)
        } else {
            let mut string = unquote(&config.search, config.literal);
            if !config.case_sensitive {
                string = string.to_lowercase();
            }
            Matcher::String(string.encode_utf16().collect())
        };
        Self { config, matcher }
    }

    pub fn config(&self) -> &SearchQueryConfig {
        &self.config
    }

    pub fn valid(&self) -> bool {
        !matches!(self.matcher, Matcher::Invalid)
    }

    pub fn find_next(&self, state: &EditorState, mut from: usize, to: usize) -> Option<SearchResult> {
        while from < to {
            let result = self.find(state, from, to)?;
            if self.check_result(state, &result) {
                return Some(result);
            }
            from = (result.from + 1).max(from + 1);
        }
        None
    }

    pub fn find_prev(&self, state: &EditorState, mut from: usize, to: usize) -> Option<SearchResult> {
        while from > to {
            let result = self.find(state, from, to)?;
            if self.check_result(state, &result) {
                return Some(result);
            }
            from = result.to.saturating_sub(1).min(from - 1);
        }
        None
    }

    fn find(&self, state: &EditorState, from: usize, to: usize) -> Option<SearchResult> {
        let forward = from < to;
        match &self.matcher {
            Matcher::Invalid => None,
            Matcher::String(needle) => {
                scan_textblocks(&state.doc, from, to, 0, &mut |node, start| {
                    let offset = from.min(to).max(start);
                    let mut text = text_content(node);
                    text.truncate(from.max(to).saturating_sub(start));
                    let text = text.get(offset - start..)?;
                    let lowered;
                    let text = if self.config.case_sensitive {
                        text
                    } else {
                        lowered = String::from_utf16_lossy(text).to_lowercase().encode_utf16().collect::<Vec<_>>();
                        &lowered
                    };
                    let mut windows = text.windows(needle.len());
                    let index = if forward {
                        windows.position(|part| part == needle)
                    } else {
                        windows.rposition(|part| part == needle)
                    }?;
                    Some(SearchResult {
                        from: offset + index,
                        to: offset + index + needle.len(),
                        matched: None,
                        match_start: start,
                    })
                })
            }
            Matcher::Regexp(regex) => {
                scan_textblocks(&state.doc, from, to, 0, &mut |node, start| {
                    let mut text = text_content(node);
                    text.truncate(from.max(to).saturating_sub(start));
                    let matched = if forward {
                        regex.find_from_utf16(&text, from.saturating_sub(start)).next()
                    } else {
                        let mut last = None;
                        let mut offset = 0;
                        while offset <= text.len() {
                            let Some(next) = regex.find_from_utf16(&text, offset).next() else {
                                break;
                            };
                            offset = (next.start() + 1).max(offset + 1);
                            last = Some(next);
                        }
                        last
                    }?;
                    Some(SearchResult {
                        from: start + matched.start(),
                        to: start + matched.end(),
                        matched: Some(RegexMatch { input: text, inner: matched }),
                        match_start: start,
                    })
                })
            }
        }
    }

    fn check_result(&self, state: &EditorState, result: &SearchResult) -> bool {
        (!self.config.whole_word
            || (check_word_boundary(&state.doc, result.from)
                && check_word_boundary(&state.doc, result.to)))
            && self.config.filter.as_ref().is_none_or(|filter| filter(state, result))
    }

    pub fn get_replacements(&self, state: &EditorState, result: &SearchResult) -> Vec<Replacement> {
        let marks = state.doc.resolve(result.from).ok()
            .zip(state.doc.resolve(result.to).ok())
            .and_then(|(from, to)| from.marks_across(&to));
        let mut ranges = Vec::new();
        let mut fragment = Fragment::empty();
        let mut pos = result.from;
        let replacement = unquote(&self.config.replace, self.config.literal);
        for part in parse_replacement(&replacement) {
            match part {
                ReplacementPart::Text(text) => {
                    fragment = fragment.add_to_end(state.schema.text(&text, marks.clone()));
                }
                ReplacementPart::Group { group, copy } => {
                    let span = match &result.matched {
                        Some(matched) => matched.group_range(group),
                        None => (group == 0).then_some(0..result.to - result.from),
                    };
                    let Some(span) = span else {
                        continue;
                    };
                    let from = result.match_start + span.start;
                    let to = result.match_start + span.end;
                    if copy {
                        if let Ok(slice) = state.doc.slice(from, to, false) {
                            fragment = fragment.append(&slice.content);
                        }
                    } else {
                        if fragment.size() > 0 || from > pos {
                            let insert = Slice::new(std::mem::take(&mut fragment), 0, 0);
                            ranges.push(Replacement { from: pos, to: from, insert });
                        }
                        pos = to;
                    }
                }
            }
        }
        if fragment.size() > 0 || pos < result.to {
            ranges.push(Replacement { from: pos, to: result.to, insert: Slice::new(fragment, 0, 0) });
        }
        ranges
    }
}

impl PartialEq for SearchQuery {
    fn eq(&self, other: &Self) -> bool {
        let a = &self.config;
        let b = &other.config;
        a.search == b.search && a.replace == b.replace && a.case_sensitive == b.case_sensitive
            && a.regexp == b.regexp && a.whole_word == b.whole_word
    }
}

fn unquote(string: &str, literal: bool) -> String {
    if literal {
        return string.to_owned();
    }
    let mut chars = string.chars().peekable();
    let mut result = String::with_capacity(string.len());
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped) = chars.next_if(|next| matches!(next, 'n' | 'r' | 't' | '\\')) {
                result.push(match escaped {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    _ => '\\',
                });
                continue;
            }
        }
        result.push(ch);
    }
    result
}

enum ReplacementPart {
    Text(String),
    Group { group: usize, copy: bool },
}

fn parse_replacement(text: &str) -> Vec<ReplacementPart> {
    let mut parts = Vec::new();
    let mut highest_seen = None;
    let mut offset = 0;
    for (index, matched) in text.as_bytes().windows(2).enumerate() {
        if index < offset || matched[0] != b'$'
            || !matches!(matched[1], b'$' | b'&' | b'0'..=b'9' | b'+')
        {
            continue;
        }
        let placeholder = matched[1];
        if index > offset {
            let end = index + usize::from(placeholder == b'$');
            add_text(&mut parts, &text[offset..end]);
        }
        if placeholder.is_ascii_digit() || placeholder == b'&' {
            let group = if placeholder == b'&' { 0 } else { usize::from(placeholder - b'0') };
            let copy = highest_seen.is_some_and(|highest| highest >= group);
            if !copy {
                highest_seen = Some(if group == 0 { 1000 } else { group });
            }
            parts.push(ReplacementPart::Group { group, copy });
        } else if placeholder == b'+' {
            highest_seen = Some(1000);
        }
        offset = index + 2;
    }
    if offset < text.len() {
        add_text(&mut parts, &text[offset..]);
    }
    parts
}

fn add_text(parts: &mut Vec<ReplacementPart>, text: &str) {
    if let Some(ReplacementPart::Text(last)) = parts.last_mut() {
        last.push_str(text);
    } else {
        parts.push(ReplacementPart::Text(text.to_owned()));
    }
}

pub fn valid_regexp(source: &str) -> bool {
    Regex::with_flags(source, "u").is_ok()
}

fn text_content(node: &Node) -> Vec<u16> {
    fn collect(node: &Node, output: &mut Vec<u16>) {
        for child in &node.content().children {
            if let Some(text) = child.text() {
                output.extend(text.encode_utf16());
            } else if child.is_leaf() {
                output.push(0xfffc);
            } else {
                output.push(u16::from(b' '));
                collect(child, output);
                output.push(u16::from(b' '));
            }
        }
    }
    let mut output = Vec::with_capacity(node.content().size());
    collect(node, &mut output);
    output
}

fn scan_textblocks<T>(
    node: &Node,
    from: usize,
    to: usize,
    start: usize,
    scan: &mut impl FnMut(&Node, usize) -> Option<T>,
) -> Option<T> {
    if node.inline_content() {
        return scan(node, start);
    }
    if from > to {
        let mut pos = start + node.content().size();
        for child in node.content().children.iter().rev() {
            if pos <= to {
                break;
            }
            pos -= child.node_size();
            if pos < from {
                if let Some(result) = scan_textblocks(child, from, to, pos + 1, scan) {
                    return Some(result);
                }
            }
        }
    } else {
        let mut pos = start;
        for child in &node.content().children {
            if pos >= to {
                break;
            }
            let child_start = pos;
            pos += child.node_size();
            if pos > from {
                if let Some(result) = scan_textblocks(child, from, to, child_start + 1, scan) {
                    return Some(result);
                }
            }
        }
    }
    None
}

fn check_word_boundary(doc: &Node, pos: usize) -> bool {
    let Ok(pos) = doc.resolve(pos) else { return true };
    let before = pos.node_before().and_then(|node| node.text()?.chars().next_back());
    let after = pos.node_after().and_then(|node| node.text()?.chars().next());
    !before.is_some_and(is_letter) || !after.is_some_and(is_letter)
}

fn is_letter(ch: char) -> bool {
    static LETTER: OnceLock<Regex> = OnceLock::new();
    let regex = LETTER.get_or_init(|| Regex::with_flags(r"\p{L}", "u").expect("Unicode letter property"));
    regex.find(ch.encode_utf8(&mut [0; 4])).is_some()
}
