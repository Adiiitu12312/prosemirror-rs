use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::{Fragment, ModelError, NodeSpec, Schema};

#[derive(Clone, Debug)]
pub struct ContentMatch {
    states: Arc<Vec<MatchState>>,
    index: usize,
}

#[derive(Debug)]
struct MatchState {
    valid_end: bool,
    edges: Vec<(Label, usize)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Label {
    name: String,
    inline: bool,
    generatable: bool,
}

impl ContentMatch {
    pub(crate) fn empty() -> Self {
        Self {
            states: Arc::new(vec![MatchState {
                valid_end: true,
                edges: Vec::new(),
            }]),
            index: 0,
        }
    }

    pub(crate) fn parse(
        source: &str,
        types: &[(String, NodeSpec)],
    ) -> Result<Self, ModelError> {
        let mut parser = Parser::new(source, types);
        if parser.next().is_none() {
            return Ok(Self::empty());
        }
        let expression = parser.choice()?;
        if parser.next().is_some() {
            return Err(parser.error("Unexpected trailing text"));
        }
        let mut nfa = Nfa(vec![Vec::new()]);
        let exits = nfa.compile(&expression, 0);
        let end = nfa.new_state();
        nfa.connect(exits, end);
        let states = nfa.determinize(end);
        if let Some(dead) = states
            .iter()
            .find(|state| !state.valid_end && state.edges.iter().all(|(label, _)| !label.generatable))
        {
            let names = dead
                .edges
                .iter()
                .map(|(label, _)| label.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(parser.error(&format!(
                "Only non-generatable nodes ({names}) in a required position"
            )));
        }
        Ok(Self {
            states: Arc::new(states),
            index: 0,
        })
    }

    pub fn valid_end(&self) -> bool {
        self.states[self.index].valid_end
    }

    pub fn inline_content(&self) -> bool {
        self.edges().first().is_some_and(|(label, _)| label.inline)
    }

    pub fn default_type(&self) -> Option<&str> {
        self.edges()
            .iter()
            .find(|(label, _)| label.generatable)
            .map(|(label, _)| label.name.as_str())
    }

    pub fn edge_count(&self) -> usize {
        self.edges().len()
    }

    pub fn edge(&self, index: usize) -> Option<(&str, Self)> {
        self.edges()
            .get(index)
            .map(|(label, state)| (label.name.as_str(), self.at(*state)))
    }

    pub fn match_type(&self, name: &str) -> Option<Self> {
        self.edges()
            .iter()
            .find(|(label, _)| label.name == name)
            .map(|(_, state)| self.at(*state))
    }

    pub fn match_fragment(&self, fragment: &Fragment, start: usize, end: usize) -> Option<Self> {
        let mut current = self.clone();
        for child in fragment.children.get(start..end)? {
            current = current.match_type(child.type_name())?;
        }
        Some(current)
    }

    pub fn compatible(&self, other: &Self) -> bool {
        self.edges()
            .iter()
            .any(|(label, _)| other.match_type(&label.name).is_some())
    }

    pub fn fill_before(
        &self,
        schema: &Schema,
        after: &Fragment,
        to_end: bool,
        start_index: usize,
    ) -> Option<Fragment> {
        self.fill_before_with_stack(schema, after, to_end, start_index, &mut Vec::new())
    }

    pub(crate) fn fill_before_with_stack(
        &self,
        schema: &Schema,
        after: &Fragment,
        to_end: bool,
        start_index: usize,
        generating: &mut Vec<String>,
    ) -> Option<Fragment> {
        struct Search<'a> {
            schema: &'a Schema,
            after: &'a Fragment,
            to_end: bool,
            start_index: usize,
            seen: HashSet<usize>,
        }

        impl Search<'_> {
            fn visit(
                &mut self,
                matched: &ContentMatch,
                generating: &mut Vec<String>,
            ) -> Option<Vec<crate::Node>> {
                if matched
                    .match_fragment(self.after, self.start_index, self.after.child_count())
                    .is_some_and(|end| !self.to_end || end.valid_end())
                {
                    return Some(Vec::new());
                }

                for (label, next) in matched.edges() {
                    if !label.generatable || self.seen.contains(next) {
                        continue;
                    }
                    let child = self
                        .schema
                        .node_type(&label.name)?
                        .create_and_fill_with_stack(None, None, None, generating)
                        .ok()
                        .flatten();
                    if let Some(child) = child {
                        self.seen.insert(*next);
                        if let Some(mut rest) = self.visit(&matched.at(*next), generating) {
                            rest.insert(0, child);
                            return Some(rest);
                        }
                    }
                }
                None
            }
        }

        Search {
            schema,
            after,
            to_end,
            start_index,
            seen: HashSet::from([self.index]),
        }
        .visit(self, generating)
        .map(Fragment::from_nodes)
    }

    pub fn find_wrapping(&self, schema: &Schema, target: &str) -> Option<Vec<String>> {
        let mut queue = VecDeque::from([(self.clone(), Vec::<String>::new())]);
        let mut seen = HashSet::new();
        while let Some((matched, wrappers)) = queue.pop_front() {
            if matched.match_type(target).is_some() {
                return Some(wrappers);
            }
            for (label, next) in matched.edges() {
                let node_type = schema.node_type(&label.name)?;
                if !node_type.is_leaf()
                    && !node_type.has_required_attrs()
                    && (wrappers.is_empty() || matched.at(*next).valid_end())
                    && seen.insert(label.name.clone())
                {
                    let mut path = wrappers.clone();
                    path.push(label.name.clone());
                    queue.push_back((node_type.content_match().clone(), path));
                }
            }
        }
        None
    }

    fn edges(&self) -> &[(Label, usize)] {
        &self.states[self.index].edges
    }

    fn at(&self, index: usize) -> Self {
        Self {
            states: Arc::clone(&self.states),
            index,
        }
    }
}

enum Expr {
    Choice(Vec<Expr>),
    Sequence(Vec<Expr>),
    Star(Box<Expr>),
    Plus(Box<Expr>),
    Optional(Box<Expr>),
    Range(Box<Expr>, usize, Option<usize>),
    Name(Label),
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<&'a str>,
    index: usize,
    inline: Option<bool>,
    types: &'a [(String, NodeSpec)],
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, types: &'a [(String, NodeSpec)]) -> Self {
        let mut tokens = Vec::new();
        let mut start = None;
        for (index, ch) in source.char_indices() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                start.get_or_insert(index);
            } else {
                if let Some(from) = start.take() {
                    tokens.push(&source[from..index]);
                }
                if !ch.is_whitespace() {
                    tokens.push(&source[index..index + ch.len_utf8()]);
                }
            }
        }
        if let Some(from) = start {
            tokens.push(&source[from..]);
        }
        Self {
            source,
            tokens,
            index: 0,
            inline: None,
            types,
        }
    }

    fn next(&self) -> Option<&'a str> {
        self.tokens.get(self.index).copied()
    }

    fn eat(&mut self, token: &str) -> bool {
        if self.next() == Some(token) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn error(&self, message: &str) -> ModelError {
        ModelError::InvalidSchema(format!("{message} (in content expression '{}')", self.source))
    }

    fn choice(&mut self) -> Result<Expr, ModelError> {
        let mut expressions = vec![self.sequence()?];
        while self.eat("|") {
            expressions.push(self.sequence()?);
        }
        Ok(Expr::Choice(expressions))
    }

    fn sequence(&mut self) -> Result<Expr, ModelError> {
        let mut expressions = vec![self.subscript()?];
        while !matches!(self.next(), None | Some(")" | "|")) {
            expressions.push(self.subscript()?);
        }
        Ok(Expr::Sequence(expressions))
    }

    fn subscript(&mut self) -> Result<Expr, ModelError> {
        let mut body = self.atom()?;
        loop {
            body = match self.next() {
                Some("*") => Expr::Star(Box::new(body)),
                Some("+") => Expr::Plus(Box::new(body)),
                Some("?") => Expr::Optional(Box::new(body)),
                Some("{") => {
                    self.index += 1;
                    body = self.range(body)?;
                    continue;
                }
                _ => return Ok(body),
            };
            self.index += 1;
        }
    }

    fn number(&mut self) -> Result<usize, ModelError> {
        let number = self
            .next()
            .and_then(|token| token.parse().ok())
            .ok_or_else(|| self.error("Expected a repetition count"))?;
        self.index += 1;
        Ok(number)
    }

    fn range(&mut self, body: Expr) -> Result<Expr, ModelError> {
        let min = self.number()?;
        let max = if self.eat(",") {
            (self.next() != Some("}")).then(|| self.number()).transpose()?
        } else {
            Some(min)
        };
        if !self.eat("}") {
            return Err(self.error("Unclosed braced range"));
        }
        Ok(Expr::Range(Box::new(body), min, max))
    }

    fn atom(&mut self) -> Result<Expr, ModelError> {
        if self.eat("(") {
            let result = self.choice()?;
            if !self.eat(")") {
                return Err(self.error("Missing closing paren"));
            }
            return Ok(result);
        }
        let name = self.next().ok_or_else(|| self.error("Unexpected end"))?;
        if !name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            return Err(self.error(&format!("Unexpected token '{name}'")));
        }
        self.index += 1;
        let named = self.types.iter().find(|(kind, _)| kind == name);
        let types = match named {
            Some(item) => vec![item],
            None => self
                .types
                .iter()
                .filter(|(_, spec)| {
                    spec.group
                        .as_deref()
                        .is_some_and(|group| group.split_whitespace().any(|part| part == name))
                })
                .collect(),
        };
        if types.is_empty() {
            return Err(self.error(&format!("No node type or group '{name}' found")));
        }
        let mut expressions = Vec::with_capacity(types.len());
        for (name, spec) in types {
            let inline = spec.inline || name == "text";
            if self.inline.is_some_and(|previous| previous != inline) {
                return Err(self.error("Mixing inline and block content"));
            }
            self.inline = Some(inline);
            expressions.push(Expr::Name(Label {
                name: name.clone(),
                inline,
                generatable: name != "text" && !spec.has_required_attrs(),
            }));
        }
        Ok(Expr::Choice(expressions))
    }
}

struct Nfa(Vec<Vec<NfaEdge>>);

struct NfaEdge {
    label: Option<Label>,
    to: usize,
}

impl Nfa {
    fn new_state(&mut self) -> usize {
        self.0.push(Vec::new());
        self.0.len() - 1
    }

    fn edge(&mut self, from: usize, to: usize, label: Option<Label>) -> (usize, usize) {
        let edges = &mut self.0[from];
        edges.push(NfaEdge { label, to });
        (from, edges.len() - 1)
    }

    fn connect(&mut self, exits: Vec<(usize, usize)>, to: usize) {
        for (state, edge) in exits {
            self.0[state][edge].to = to;
        }
    }

    // The order of edges controls which node the matcher uses as filler.
    fn compile(&mut self, expr: &Expr, mut from: usize) -> Vec<(usize, usize)> {
        match expr {
            Expr::Choice(expressions) => expressions
                .iter()
                .flat_map(|expression| self.compile(expression, from))
                .collect(),
            Expr::Sequence(expressions) => {
                let mut exits = self.compile(&expressions[0], from);
                for expression in &expressions[1..] {
                    from = self.new_state();
                    self.connect(exits, from);
                    exits = self.compile(expression, from);
                }
                exits
            }
            Expr::Star(expression) => {
                let repeat = self.new_state();
                self.edge(from, repeat, None);
                let exits = self.compile(expression, repeat);
                self.connect(exits, repeat);
                vec![self.edge(repeat, 0, None)]
            }
            Expr::Plus(expression) => {
                let repeat = self.new_state();
                let exits = self.compile(expression, from);
                self.connect(exits, repeat);
                let exits = self.compile(expression, repeat);
                self.connect(exits, repeat);
                vec![self.edge(repeat, 0, None)]
            }
            Expr::Optional(expression) => {
                let mut exits = vec![self.edge(from, 0, None)];
                exits.extend(self.compile(expression, from));
                exits
            }
            Expr::Range(expression, min, max) => {
                for _ in 0..*min {
                    let next = self.new_state();
                    let exits = self.compile(expression, from);
                    self.connect(exits, next);
                    from = next;
                }
                if let Some(max) = max {
                    for _ in *min..*max {
                        let next = self.new_state();
                        self.edge(from, next, None);
                        let exits = self.compile(expression, from);
                        self.connect(exits, next);
                        from = next;
                    }
                } else {
                    let exits = self.compile(expression, from);
                    self.connect(exits, from);
                }
                vec![self.edge(from, 0, None)]
            }
            Expr::Name(label) => vec![self.edge(from, 0, Some(label.clone()))],
        }
    }

    fn closure(&self, start: usize) -> Vec<usize> {
        let mut seen = HashSet::new();
        let mut pending = vec![start];
        let mut result = Vec::new();
        while let Some(state) = pending.pop() {
            if !seen.insert(state) {
                continue;
            }
            let edges = &self.0[state];
            if edges.len() != 1 || edges[0].label.is_some() {
                result.push(state);
            }
            pending.extend(edges.iter().filter(|edge| edge.label.is_none()).map(|edge| edge.to));
        }
        result.sort_unstable_by(|a, b| b.cmp(a));
        result
    }

    fn determinize(&self, end: usize) -> Vec<MatchState> {
        let first = self.closure(0);
        let mut indices = HashMap::from([(first.clone(), 0)]);
        let mut pending = VecDeque::from([first]);
        let mut states = Vec::new();
        while let Some(subset) = pending.pop_front() {
            let valid_end = subset.contains(&end);
            let mut transitions: Vec<(Label, HashSet<usize>)> = Vec::new();
            for state in subset {
                for edge in &self.0[state] {
                    let Some(label) = &edge.label else { continue };
                    let targets = match transitions.iter_mut().find(|(known, _)| known == label) {
                        Some((_, targets)) => targets,
                        None => {
                            transitions.push((label.clone(), HashSet::new()));
                            &mut transitions.last_mut().expect("transition was appended").1
                        }
                    };
                    targets.extend(self.closure(edge.to));
                }
            }
            let mut edges = Vec::with_capacity(transitions.len());
            for (label, targets) in transitions {
                let mut targets: Vec<_> = targets.into_iter().collect();
                targets.sort_unstable_by(|a, b| b.cmp(a));
                let next_index = indices.len();
                let target_index = *indices.entry(targets.clone()).or_insert_with(|| {
                    pending.push_back(targets);
                    next_index
                });
                edges.push((label, target_index));
            }
            states.push(MatchState { valid_end, edges });
        }
        states
    }
}
