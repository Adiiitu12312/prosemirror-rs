//! Myers diff and UTF-16 token encoding from prosemirror-changeset 2.4.0 diff.ts.

use prosemirror_model::{Fragment, Mark, Node};

use crate::Change;

pub trait TokenEncoder {
    type Token;

    fn encode_character(&self, unit: u16, marks: &[Mark]) -> Self::Token;
    fn encode_node_start(&self, node: &Node) -> Self::Token;
    fn encode_node_end(&self, node: &Node) -> Self::Token;
    fn compare_tokens(&self, a: &Self::Token, b: &Self::Token) -> bool;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultEncoder;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DefaultToken {
    Character(u16),
    NodeStart(String),
    NodeEnd { name: String, rank: Option<usize> },
}

impl TokenEncoder for DefaultEncoder {
    type Token = DefaultToken;

    fn encode_character(&self, unit: u16, _: &[Mark]) -> Self::Token {
        DefaultToken::Character(unit)
    }

    fn encode_node_start(&self, node: &Node) -> Self::Token {
        DefaultToken::NodeStart(node.type_name().to_owned())
    }

    fn encode_node_end(&self, node: &Node) -> Self::Token {
        let rank = node.schema().and_then(|schema| {
            schema.node_types().position(|ty| ty.name == node.type_name())
        });
        DefaultToken::NodeEnd { name: node.type_name().to_owned(), rank }
    }

    fn compare_tokens(&self, a: &Self::Token, b: &Self::Token) -> bool {
        match (a, b) {
            (
                DefaultToken::NodeEnd { name: a, rank: a_rank },
                DefaultToken::NodeEnd { name: b, rank: b_rank },
            ) => a_rank.zip(*b_rank).map_or(a == b, |(a, b)| a == b),
            _ => a == b,
        }
    }
}

fn tokenize<E: TokenEncoder>(
    frag: &Fragment,
    encoder: &E,
    start: usize,
    end: usize,
    target: &mut Vec<E::Token>,
) {
    let mut offset = 0;
    for child in &frag.children {
        if offset >= end {
            break;
        }
        let child_end = offset + child.node_size();
        let from = offset.max(start);
        let to = child_end.min(end);
        if from < to {
            if let Some(text) = child.text_utf16() {
                target.extend(text[from - offset..to - offset].iter().map(|unit| {
                    encoder.encode_character(*unit, child.marks())
                }));
            } else if child.is_leaf() {
                target.push(encoder.encode_node_start(child));
            } else {
                if from == offset {
                    target.push(encoder.encode_node_start(child));
                }
                tokenize(
                    child.content(),
                    encoder,
                    from.saturating_sub(offset + 1),
                    to.min(child_end - 1) - (offset + 1),
                    target,
                );
                if to == child_end {
                    target.push(encoder.encode_node_end(child));
                }
            }
        }
        offset = child_end;
    }
}

pub fn compute_diff<D: Clone>(
    a: &Fragment,
    b: &Fragment,
    range: &Change<D>,
    encoder: &impl TokenEncoder,
) -> Vec<Change<D>> {
    let (mut a_tokens, mut b_tokens) = (Vec::new(), Vec::new());
    tokenize(a, encoder, range.from_a, range.to_a, &mut a_tokens);
    tokenize(b, encoder, range.from_b, range.to_b, &mut b_tokens);
    let start = a_tokens.iter().zip(&b_tokens).take_while(|(a, b)| encoder.compare_tokens(a, b)).count();
    let (mut end_a, mut end_b) = (a_tokens.len(), b_tokens.len());
    if start == end_a && start == end_b {
        return Vec::new();
    }
    while end_a > start && end_b > start && encoder.compare_tokens(&a_tokens[end_a - 1], &b_tokens[end_b - 1]) {
        end_a -= 1;
        end_b -= 1;
    }
    if end_a == start || end_b == start || (end_a == end_b && end_a == start + 1) {
        return vec![range.slice(start, end_a, start, end_b)];
    }

    match myers(&a_tokens[start..end_a], &b_tokens[start..end_b], encoder) {
        Some(edits) => edits.into_iter().map(|[from_a, to_a, from_b, to_b]| {
            range.slice(start + from_a, start + to_a, start + from_b, start + to_b)
        }).collect(),
        None => vec![range.slice(start, end_a, start, end_b)],
    }
}

// Limit the edit distance to bound the quadratic frontier search.
const MAX_DIFF_SIZE: usize = 5_000;

fn myers<E: TokenEncoder>(a: &[E::Token], b: &[E::Token], encoder: &E) -> Option<Vec<[usize; 4]>> {
    let max = MAX_DIFF_SIZE.min(a.len() + b.len()) as isize;
    let center = max + 1;
    let mut frontier = vec![-1_i32; (center * 2 + 1) as usize];
    let mut history = Vec::new();
    let (len_a, len_b) = (a.len() as isize, b.len() as isize);
    let min_unchanged = (a.len().max(b.len()) / 10).clamp(2, 15);
    for size in 0..=max {
        for diag in (-size..=size).step_by(2) {
            let index = (diag + center) as usize;
            let (next, prev) = (frontier[index + 1], frontier[index - 1]);
            let mut x = (if next < prev { prev } else { next + 1 }) as isize;
            let mut y = x + diag;
            while x < len_a && y < len_b && encoder.compare_tokens(&a[x as usize], &b[y as usize]) {
                x += 1;
                y += 1;
            }
            frontier[index] = x as i32;
            if x >= len_a && y >= len_b {
                return Some(trace_back(size, diag, center, &frontier, &history, min_unchanged));
            }
        }
        if size % 2 == 0 {
            history.push(frontier.clone());
        }
    }
    None
}

fn trace_back<'a>(
    size: isize,
    mut diag: isize,
    center: isize,
    mut frontier: &'a [i32],
    history: &'a [Vec<i32>],
    min_unchanged: usize,
) -> Vec<[usize; 4]> {
    let mut edits: Vec<[usize; 4]> = Vec::new();
    for i in (0..size).rev() {
        let index = (diag + center) as usize;
        let (next, prev) = (frontier[index + 1], frontier[index - 1]);
        let edit = if next < prev {
            diag -= 1;
            let x = prev as usize;
            let y = (prev as isize + diag) as usize;
            [x, x, y, y + 1]
        } else {
            diag += 1;
            let x = next as usize;
            let y = (next as isize + diag) as usize;
            [x, x + 1, y, y]
        };
        if let Some(last) = edits.last_mut().filter(|last| last[0] < edit[1] + min_unchanged) {
            last[0] = edit[0];
            last[2] = edit[2];
        } else {
            edits.push(edit);
        }
        frontier = &history[(i / 2) as usize];
    }
    edits.reverse();
    edits
}
