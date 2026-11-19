//! Native selection navigation over ProseMirror positions and host line geometry.

use prosemirror_model::{Node, ResolvedPos};
use prosemirror_state::{find_gap_cursor_from, EditorState, Selection};
use unicode_segmentation::UnicodeSegmentation;

use crate::host::{Layout, LineBox, Point, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Backward,
    Forward,
}

impl Direction {
    fn bias(self) -> i32 {
        match self {
            Self::Backward => -1,
            Self::Forward => 1,
        }
    }

    fn edge<T>(self, from: T, to: T) -> T {
        match self {
            Self::Backward => from,
            Self::Forward => to,
        }
    }

    fn reverse(self) -> Self {
        self.edge(Self::Forward, Self::Backward)
    }
}

/// Moves in document order, through a visual line, or between visual lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Movement {
    Character(Direction),
    /// Backward moves left, and forward moves right, in the host's measured layout.
    VisualCharacter(Direction),
    Word(Direction),
    LineBoundary(Direction),
    /// Backward moves up, and forward moves down.
    Vertical(Direction),
    Paragraph(Direction),
    Document(Direction),
}

impl Movement {
    fn direction(self) -> Direction {
        match self {
            Self::Character(dir) | Self::VisualCharacter(dir) | Self::Word(dir) | Self::LineBoundary(dir) | Self::Vertical(dir)
            | Self::Paragraph(dir) | Self::Document(dir) => dir,
        }
    }
}

/// Returns a new selection after applying the movement. Reuse `goal` across
/// consecutive vertical moves and clear it on pointer, input, or focus changes.
pub fn move_selection(
    state: &EditorState,
    layout: &(impl Layout + ?Sized),
    movement: Movement,
    extend: bool,
    goal: &mut Option<f64>,
) -> Option<Selection> {
    if !matches!(movement, Movement::Vertical(_)) {
        *goal = None;
    }
    let doc = &state.doc;
    let selection = &state.selection;
    let direction = movement.direction();
    let visual = match movement {
        Movement::VisualCharacter(physical) => doc.resolve(selection.head(doc)).ok()
            .and_then(|head| VisualLine::measure(layout, &head)).map(|line| (line, physical)),
        _ => None,
    };
    let direction = if visual.is_some_and(|(line, _)| line.right_to_left) { direction.reverse() } else { direction };
    let from_gap = matches!(selection, Selection::GapCursor { .. });
    let from = selection.from();
    let to = selection.to(doc);
    let edge = visual.and_then(|(_, physical)| visual_edge(layout, from, to, physical))
        .unwrap_or_else(|| direction.edge(from, to));
    let start = if extend || matches!(selection, Selection::Text { .. }) {
        selection.head(doc)
    } else {
        edge
    };
    let start = doc.resolve(start).ok()?;
    let target = match movement {
        Movement::Character(_) | Movement::VisualCharacter(_) | Movement::Word(_) if !extend && !selection.empty_in(doc) => {
            let edge = doc.resolve(edge).ok()?;
            match selection {
                Selection::All => document_boundary(doc, direction),
                Selection::Node { .. } if !edge.parent().inline_content() => {
                    block_step(&edge, direction, false)?
                }
                _ => Selection::near(&edge, direction.bias()),
            }
        }
        Movement::Character(_) | Movement::VisualCharacter(_) | Movement::Word(_) => {
            visual.and_then(|(line, physical)| line.step(layout, &start, physical, extend))
                .or_else(|| inline_step(&start, direction, matches!(movement, Movement::Word(_)), extend))
                .or_else(|| block_step(&start, direction, from_gap))?
        }
        Movement::LineBoundary(_) => line_boundary(layout, &start, direction, from_gap)?,
        Movement::Vertical(_) => vertical(layout, &start, direction, goal)
            .or_else(|| block_step(&start, direction, from_gap))?,
        Movement::Paragraph(_) => paragraph_boundary(&start, direction, from_gap)?,
        Movement::Document(_) => document_boundary(doc, direction),
    };
    let next = if extend {
        let end = if target.empty_in(doc) { target.head(doc) } else { direction.edge(target.from(), target.to(doc)) };
        Selection::between(&doc.resolve(selection.anchor()).ok()?, &doc.resolve(end).ok()?, direction.bias())
    } else {
        target
    };
    (&next != selection).then_some(next)
}

fn inline_step(pos: &ResolvedPos, dir: Direction, word: bool, extend: bool) -> Option<Selection> {
    let parent = pos.parent();
    if !parent.inline_content() {
        return None;
    }
    if !word && !extend {
        if let Some(atom) = adjacent_atom(pos, dir) {
            return Some(atom);
        }
    }
    let cursor = pos.parent_offset;
    let mut best = None;
    visit_inline_boundaries(parent, dir, word, |candidate| {
        let distance = match dir {
            Direction::Backward => cursor.checked_sub(candidate),
            Direction::Forward => candidate.checked_sub(cursor),
        };
        if let Some(distance) = distance.filter(|&distance| distance != 0) {
            if best.is_none_or(|(old, _)| distance < old) {
                best = Some((distance, candidate));
            }
        }
    });
    best.map(|(_, target)| Selection::cursor(pos.start(pos.depth) + target))
}

fn adjacent_atom(pos: &ResolvedPos, dir: Direction) -> Option<Selection> {
    if pos.text_offset() != 0 {
        return None;
    }
    let atom = match dir {
        Direction::Backward => pos.node_before(),
        Direction::Forward => pos.node_after(),
    }.filter(|node| !node.is_text() && node.is_atom() && node.is_selectable())?;
    let before = match dir {
        Direction::Backward => pos.pos.checked_sub(atom.node_size())?,
        Direction::Forward => pos.pos,
    };
    Some(Selection::node(before))
}

fn visit_inline_boundaries(parent: &Node, dir: Direction, word: bool, mut visit: impl FnMut(usize)) {
    let mut run = Vec::new();
    let mut offset = 0;
    for child in &parent.content().children {
        if let Some(text) = child.text_utf16() {
            run.extend_from_slice(&text);
        } else {
            visit_text_boundaries(&run, offset - run.len(), dir, word, &mut visit);
            run.clear();
            visit(offset);
            visit(offset + child.node_size());
        }
        offset += child.node_size();
    }
    visit_text_boundaries(&run, offset - run.len(), dir, word, visit);
}

#[derive(Clone, Copy)]
struct VisualLine {
    line: LineBox,
    caret: Rect,
    right_to_left: bool,
}

impl VisualLine {
    fn measure(layout: &(impl Layout + ?Sized), pos: &ResolvedPos) -> Option<Self> {
        if !pos.parent().inline_content() {
            return None;
        }
        let caret = layout.coords_at_pos(pos.pos, 1)?;
        let lines = layout.line_boxes(pos.start(pos.depth), pos.end(pos.depth));
        let line = *current_line(&lines, pos.pos, Some(caret.center().y))?;
        let right_to_left = layout.coords_at_pos(line.from, 1).zip(layout.coords_at_pos(line.to, -1))
            .is_some_and(|(from, to)| from.left > to.left);
        Some(Self { line, caret, right_to_left })
    }

    fn step(self, layout: &(impl Layout + ?Sized), pos: &ResolvedPos, physical: Direction, extend: bool) -> Option<Selection> {
        let start = pos.start(pos.depth);
        let mut closest = None;
        visit_inline_boundaries(pos.parent(), physical, false, |offset| {
            let position = start + offset;
            if position == pos.pos || !(self.line.from..=self.line.to).contains(&position) {
                return;
            }
            if let Some(caret) = layout.coords_at_pos(position, 1).filter(|caret| same_line(*caret, self.caret)) {
                let distance = (caret.left - self.caret.left) * f64::from(physical.bias());
                if distance > 0.0 && closest.is_none_or(|(score, _)| distance < score) {
                    closest = Some((distance, position));
                }
            }
        });
        let (_, target) = closest?;
        if !extend {
            let logical = if target < pos.pos { Direction::Backward } else { Direction::Forward };
            if let Some(atom) = adjacent_atom(pos, logical) {
                if logical.edge(atom.from(), atom.to(pos.doc())) == target {
                    return Some(atom);
                }
            }
        }
        Some(Selection::cursor(target))
    }
}

fn same_line(a: Rect, b: Rect) -> bool {
    let center = a.center().y;
    b.top <= center && center <= b.bottom
}

fn visual_edge(layout: &(impl Layout + ?Sized), from: usize, to: usize, dir: Direction) -> Option<usize> {
    let first = layout.coords_at_pos(from, 1)?;
    let last = layout.coords_at_pos(to, -1)?;
    same_line(first, last).then(|| {
        if first.left <= last.left { dir.edge(from, to) } else { dir.edge(to, from) }
    })
}

fn visit_text_boundaries(
    units: &[u16],
    start: usize,
    dir: Direction,
    word: bool,
    mut visit: impl FnMut(usize),
) {
    if units.is_empty() {
        return;
    }
    let text = String::from_utf16_lossy(units);
    visit(start);
    if word {
        let mut offset = start;
        for piece in text.split_word_bounds() {
            let end = offset + piece.encode_utf16().count();
            if piece.unicode_words().next().is_some() {
                visit(dir.edge(offset, end));
            }
            offset = end;
        }
        visit(offset);
    } else {
        let mut offset = start;
        for grapheme in text.graphemes(true) {
            offset += grapheme.encode_utf16().count();
            visit(offset);
        }
    }
}

fn block_step(pos: &ResolvedPos, dir: Direction, must_move: bool) -> Option<Selection> {
    let boundary = if pos.parent().inline_content() && pos.depth > 0 {
        pos.doc().resolve(dir.edge(pos.before(pos.depth), pos.after(pos.depth))).ok()?
    } else {
        pos.clone()
    };
    find_gap_cursor_from(&boundary, dir.bias(), must_move)
        .map(|gap| Selection::gap_cursor(gap.pos))
        .or_else(|| Selection::find_from(&boundary, dir.bias(), false))
}

fn paragraph_boundary(pos: &ResolvedPos, dir: Direction, from_gap: bool) -> Option<Selection> {
    if pos.parent().inline_content() {
        let end = dir.edge(pos.start(pos.depth), pos.end(pos.depth));
        if pos.pos != end {
            return Some(Selection::cursor(end));
        }
    }
    block_step(pos, dir, from_gap)
}

fn document_boundary(doc: &Node, dir: Direction) -> Selection {
    match dir {
        Direction::Backward => Selection::at_start(doc),
        Direction::Forward => Selection::at_end(doc),
    }
}

fn line_boundary(layout: &(impl Layout + ?Sized), pos: &ResolvedPos, dir: Direction, from_gap: bool) -> Option<Selection> {
    let text_range = pos.start(pos.depth)..pos.end(pos.depth);
    let lines = layout.line_boxes(text_range.start, text_range.end);
    let y = layout.coords_at_pos(pos.pos, 1).map(|caret| caret.center().y);
    if let Some(line) = current_line(&lines, pos.pos, y) {
        let end = dir.edge(line.from, line.to);
        return selection_at(pos.doc(), end, dir);
    }
    if !pos.parent().inline_content() {
        return block_step(pos, dir, from_gap);
    }
    let mut from = text_range.start;
    let mut to = text_range.end;
    let mut offset = from;
    let mut consider_break = |start, end| {
        if end <= pos.pos {
            from = from.max(end);
        } else if start >= pos.pos {
            to = to.min(start);
        }
    };
    for child in &pos.parent().content().children {
        if let Some(units) = child.text_utf16() {
            for (index, &unit) in units.iter().enumerate() {
                if unit == u16::from(b'\n') {
                    let before = index - usize::from(index > 0 && units[index - 1] == u16::from(b'\r'));
                    consider_break(offset + before, offset + index + 1);
                }
            }
        } else if child.type_name() == "hard_break" || child.node_type().is_some_and(|kind| kind.spec.linebreak_replacement) {
            consider_break(offset, offset + child.node_size());
        }
        offset += child.node_size();
    }
    Some(Selection::cursor(dir.edge(from, to)))
}

fn current_line(lines: &[LineBox], pos: usize, y: Option<f64>) -> Option<&LineBox> {
    lines.iter().filter(|line| line.from <= pos && pos <= line.to)
        .min_by(|a, b| {
            let distance = |line: &LineBox| y.map_or(0.0, |y| (line.rect.center().y - y).abs());
            distance(a).total_cmp(&distance(b))
        })
}

fn vertical(
    layout: &(impl Layout + ?Sized), pos: &ResolvedPos, dir: Direction, goal: &mut Option<f64>,
) -> Option<Selection> {
    if !pos.parent().inline_content() {
        return None;
    }
    let caret = layout.coords_at_pos(pos.pos, 1)?;
    if !caret.left.is_finite() || !caret.top.is_finite() || !caret.bottom.is_finite() {
        return None;
    }
    let x = *goal.get_or_insert(caret.left);
    let lines = layout.line_boxes(0, pos.doc().content().size());
    let focus_y = caret.center().y;
    let target = if let Some(current) = current_line(&lines, pos.pos, Some(focus_y)) {
        let current_y = current.rect.center().y;
        let offset = |line: &LineBox| (line.rect.center().y - current_y) * f64::from(dir.bias());
        let horizontal_distance = |line: &LineBox| (line.rect.left - x).max(x - line.rect.right).max(0.0);
        let line = lines.iter().filter(|line| line.rect.left <= line.rect.right && line.from <= line.to && offset(line) > 0.0)
            .min_by(|a, b| offset(a).total_cmp(&offset(b)).then_with(|| horizontal_distance(a).total_cmp(&horizontal_distance(b))))?;
        let target_x = x.clamp(line.rect.left, line.rect.right);
        let hit = layout.pos_at_coords(Point { x: target_x, y: line.rect.center().y });
        hit.map_or_else(|| {
            if x <= line.rect.center().x { line.from } else { line.to }
        }, |hit| hit.pos.clamp(line.from, line.to))
    } else {
        let y = dir.edge(caret.top, caret.bottom) + f64::from(dir.bias());
        layout.pos_at_coords(Point { x, y })?.pos
    };
    (target != pos.pos).then(|| selection_at(pos.doc(), target, dir)).flatten()
}

fn selection_at(doc: &Node, pos: usize, dir: Direction) -> Option<Selection> {
    let pos = doc.resolve(pos).ok()?;
    Some(Selection::near(&pos, dir.bias()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::PositionAtCoords;
    use prosemirror_state::EditorStateConfig;
    use prosemirror_test_builder::{builders, doc, hr, p, test_schema, TaggedDoc};
    use serde_json::json;

    struct NoLayout;

    impl Layout for NoLayout {
        fn coords_at_pos(&self, _: usize, _: i32) -> Option<Rect> { None }
        fn pos_at_coords(&self, _: Point) -> Option<PositionAtCoords> { None }
    }

    fn state(doc: TaggedDoc, selection: Selection) -> EditorState {
        EditorState::create(EditorStateConfig {
            schema: test_schema().clone(), doc: Some(doc.node), selection: Some(selection),
            stored_marks: None, plugins: vec![],
        })
    }

    fn set_selection(state: &mut EditorState, next: Selection) {
        let mut tr = state.tr();
        tr.set_selection(next);
        *state = state.apply(&tr);
    }

    #[test]
    fn character_motion_uses_extended_graphemes_across_marks() {
        let build = builders(test_schema(), Default::default());
        let combining = build.mark("em", None, vec!["\u{301}".into()]).unwrap();
        let family = "👨‍👩‍👧‍👦";
        let line = build.node("paragraph", None, vec!["e".into(), combining, family.into(), "z".into()]).unwrap();
        let mut state = state(doc!(line.node), Selection::cursor(1));
        let mut goal = Some(42.0);
        let forward = Movement::Character(Direction::Forward);
        let first = move_selection(&state, &NoLayout, forward, false, &mut goal).unwrap();
        assert_eq!(first, Selection::cursor(3));
        assert_eq!(goal, None);
        set_selection(&mut state, first);
        let next = move_selection(&state, &NoLayout, forward, false, &mut goal).unwrap();
        assert_eq!(next, Selection::cursor(3 + family.encode_utf16().count()));
        set_selection(&mut state, next);
        assert_eq!(move_selection(&state, &NoLayout, Movement::Character(Direction::Backward), false, &mut goal),
            Some(Selection::cursor(3)));
    }

    #[test]
    fn moves_by_unicode_words_and_extends_from_the_anchor() {
        let mut state = state(doc!(p!("hi  naïve world")), Selection::cursor(1));
        let first = move_selection(&state, &NoLayout, Movement::Word(Direction::Forward), true, &mut None).unwrap();
        assert_eq!(first, Selection::text(1, 3));
        set_selection(&mut state, first);
        let next = move_selection(&state, &NoLayout, Movement::Word(Direction::Forward), true, &mut None).unwrap();
        assert_eq!(next, Selection::text(1, 10));
        set_selection(&mut state, next);
        assert_eq!(move_selection(&state, &NoLayout, Movement::Word(Direction::Backward), true, &mut None),
            Some(Selection::text(1, 5)));
        assert_eq!(move_selection(&state, &NoLayout, Movement::Character(Direction::Forward), false, &mut None),
            Some(Selection::cursor(10)));
    }

    #[test]
    fn moves_to_line_and_paragraph_boundaries_without_measured_lines() {
        let line_break = test_schema().node("hard_break", None, None, None).unwrap();
        let mut state = state(doc!(p!("ab", line_break, "cd")), Selection::cursor(5));
        assert_eq!(move_selection(&state, &NoLayout, Movement::LineBoundary(Direction::Backward), false, &mut None),
            Some(Selection::cursor(4)));
        assert_eq!(move_selection(&state, &NoLayout, Movement::Paragraph(Direction::Backward), false, &mut None),
            Some(Selection::cursor(1)));
        set_selection(&mut state, Selection::cursor(2));
        assert_eq!(move_selection(&state, &NoLayout, Movement::LineBoundary(Direction::Forward), false, &mut None),
            Some(Selection::cursor(3)));
        assert_eq!(move_selection(&state, &NoLayout, Movement::Document(Direction::Forward), true, &mut None),
            Some(Selection::text(2, 6)));
    }

    #[test]
    fn navigates_between_node_gap_and_text_selections() {
        let image = test_schema().node("image", Some(json!({"src": "image.png"})), None, None).unwrap();
        let mut inline = state(doc!(p!("a", image, "b")), Selection::cursor(2));
        let movement = Movement::Character(Direction::Forward);
        let selected = move_selection(&inline, &NoLayout, movement, false, &mut None).unwrap();
        assert_eq!(selected, Selection::node(2));
        set_selection(&mut inline, selected);
        assert_eq!(move_selection(&inline, &NoLayout, movement, false, &mut None), Some(Selection::cursor(3)));

        let mut atoms = state(doc!(hr(), hr()), Selection::node(0));
        let selected = move_selection(&atoms, &NoLayout, movement, false, &mut None).unwrap();
        assert_eq!(selected, Selection::gap_cursor(1));
        set_selection(&mut atoms, selected);
        assert_eq!(move_selection(&atoms, &NoLayout, movement, false, &mut None), Some(Selection::node(1)));

        let all = state(doc!(p!("a"), p!("b")), Selection::all());
        assert_eq!(move_selection(&all, &NoLayout, movement, false, &mut None), Some(Selection::cursor(5)));
    }

    struct Lines(Vec<LineBox>);

    impl Layout for Lines {
        fn coords_at_pos(&self, pos: usize, _side: i32) -> Option<Rect> {
            let line = self.0.iter().find(|line| (line.from..=line.to).contains(&pos))?;
            let x = line.rect.left + (pos - line.from) as f64 * 10.0;
            Some(Rect { left: x, right: x, ..line.rect })
        }

        fn pos_at_coords(&self, point: Point) -> Option<PositionAtCoords> {
            let line = self.0.iter().find(|line| line.rect.contains(point))?;
            let column = ((point.x - line.rect.left) / 10.0).round() as usize;
            Some(PositionAtCoords { pos: (line.from + column).min(line.to), inside: None })
        }

        fn line_boxes(&self, from: usize, to: usize) -> Vec<LineBox> {
            self.0.iter().copied().filter(|line| line.to >= from && line.from <= to).collect()
        }
    }

    #[test]
    fn vertical_motion_preserves_the_goal_column_across_shorter_lines() {
        let layout = Lines(vec![
            LineBox { from: 1, to: 7, rect: Rect { left: 0.0, right: 60.0, top: 0.0, bottom: 10.0 } },
            LineBox { from: 9, to: 11, rect: Rect { left: 0.0, right: 20.0, top: 20.0, bottom: 30.0 } },
            LineBox { from: 13, to: 19, rect: Rect { left: 0.0, right: 60.0, top: 40.0, bottom: 50.0 } },
        ]);
        let mut state = state(doc!(p!("abcdef"), p!("xy"), p!("123456")), Selection::cursor(6));
        let mut goal = None;
        let next = move_selection(&state, &layout, Movement::Vertical(Direction::Forward), false, &mut goal).unwrap();
        assert_eq!((next.clone(), goal), (Selection::cursor(11), Some(50.0)));
        set_selection(&mut state, next);
        let next = move_selection(&state, &layout, Movement::Vertical(Direction::Forward), false, &mut goal).unwrap();
        assert_eq!((next.clone(), goal), (Selection::cursor(18), Some(50.0)));
        set_selection(&mut state, next);
        assert_eq!(move_selection(&state, &layout, Movement::Vertical(Direction::Backward), true, &mut goal),
            Some(Selection::text(18, 11)));
    }

    struct BidiLine(&'static [f64]);

    impl Layout for BidiLine {
        fn coords_at_pos(&self, pos: usize, _side: i32) -> Option<Rect> {
            let x = *self.0.get(pos.checked_sub(1)?)?;
            Some(Rect { left: x, right: x, top: 0.0, bottom: 10.0 })
        }

        fn pos_at_coords(&self, _: Point) -> Option<PositionAtCoords> { None }

        fn line_boxes(&self, _: usize, _: usize) -> Vec<LineBox> {
            vec![LineBox { from: 1, to: self.0.len(), rect: Rect {
                left: 0.0, right: 50.0, top: 0.0, bottom: 10.0,
            } }]
        }
    }

    #[test]
    fn physical_arrows_follow_measured_caret_order_in_bidirectional_text() {
        let left = Movement::VisualCharacter(Direction::Backward);
        let right = Movement::VisualCharacter(Direction::Forward);
        let mixed = state(doc!(p!("aאבגb")), Selection::cursor(2));
        let layout = BidiLine(&[0.0, 10.0, 40.0, 30.0, 20.0, 50.0]);
        assert_eq!(move_selection(&mixed, &layout, right, false, &mut None), Some(Selection::cursor(5)));
        assert_eq!(move_selection(&mixed, &layout, right, true, &mut None), Some(Selection::text(2, 5)));
        assert_eq!(move_selection(&mixed, &layout, Movement::Character(Direction::Forward), false, &mut None),
            Some(Selection::cursor(3)));

        let mut rtl = state(doc!(p!("אבגד")), Selection::text(2, 4));
        let layout = BidiLine(&[40.0, 30.0, 20.0, 10.0, 0.0]);
        assert_eq!(move_selection(&rtl, &layout, left, false, &mut None), Some(Selection::cursor(4)));
        assert_eq!(move_selection(&rtl, &layout, right, false, &mut None), Some(Selection::cursor(2)));
        set_selection(&mut rtl, Selection::cursor(5));
        assert_eq!(move_selection(&rtl, &layout, left, false, &mut None), None);
    }
}
