//! Port of prosemirror-commands@1.6.2/src/commands.ts.

use prosemirror_model::{Fragment, Node, NodeType, ResolvedPos, Slice};
use prosemirror_state::{EditorState, Selection, Transaction};
use prosemirror_transform::{can_join, can_split, find_wrapping, join_point, lift_target, replace_step, MarkFilter, Step};
use serde_json::Value;

pub type DispatchFn<'a> = Option<&'a mut dyn FnMut(Transaction)>;
pub type Command = for<'a> fn(&EditorState, DispatchFn<'a>) -> bool;
pub type BoxedCommand<'a> = Box<dyn for<'b> Fn(&EditorState, DispatchFn<'b>) -> bool + 'a>;

fn dispatch_scrolled(mut tr: Transaction, dispatch: DispatchFn<'_>) -> bool {
    if let Some(dispatch) = dispatch {
        tr.scroll_into_view();
        dispatch(tr);
    }
    true
}

pub fn delete_selection(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    if state.selection.empty_in(&state.doc) {
        return false;
    }
    let mut tr = state.tr();
    tr.delete_selection();
    dispatch_scrolled(tr, dispatch)
}

fn cursor(state: &EditorState) -> Option<ResolvedPos> {
    match state.selection {
        Selection::Text { anchor, head } if anchor == head => state.doc.resolve(head).ok(),
        _ => None,
    }
}

fn at_edge(pos: &ResolvedPos, dir: i32) -> bool {
    if dir < 0 { pos.parent_offset == 0 } else { pos.parent_offset == pos.parent().content().size() }
}

fn isolating(state: &EditorState, node: &Node) -> bool {
    state.schema.node_type(node.type_name()).is_some_and(|kind| kind.spec.isolating)
}

fn find_cut(state: &EditorState, pos: &ResolvedPos, dir: i32) -> Option<ResolvedPos> {
    if isolating(state, pos.parent()) {
        return None;
    }
    for depth in (0..pos.depth).rev() {
        let parent = pos.node(depth);
        if dir < 0 && pos.index(depth) > 0 {
            return state.doc.resolve(pos.before(depth + 1)).ok();
        }
        if dir > 0 && pos.index(depth) + 1 < parent.child_count() {
            return state.doc.resolve(pos.after(depth + 1)).ok();
        }
        if isolating(state, parent) {
            break;
        }
    }
    None
}

pub fn join_backward(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    join_at_cursor(state, dispatch, -1)
}

pub fn join_forward(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    join_at_cursor(state, dispatch, 1)
}

fn join_at_cursor(state: &EditorState, dispatch: DispatchFn<'_>, dir: i32) -> bool {
    let Some(cursor) = cursor(state).filter(|pos| at_edge(pos, dir)) else { return false };
    let Some(cut) = find_cut(state, &cursor, dir) else {
        return dir < 0 && lift(state, dispatch);
    };
    let neighbor = if dir < 0 { cut.node_before() } else { cut.node_after() };
    let Some(neighbor) = neighbor else { return false };
    if let Some(tr) = delete_barrier(state, &cut, dir) {
        return dispatch_scrolled(tr, dispatch);
    }

    let textblock = textblock_at(&neighbor, dir, false);
    if cursor.parent().content().size() == 0 && (textblock || selectable(state, &neighbor)) {
        for depth in (1..=cursor.depth).rev() {
            let from = cursor.before(depth);
            let to = cursor.after(depth);
            if let Some(step @ Step::Replace { .. }) = replace_step(&state.doc, from, to, Slice::empty()) {
                if let Step::Replace { from, to, slice, .. } = &step {
                    if slice.size() < to - from {
                        let mut tr = state.tr();
                        if tr.step(step).is_ok() {
                            let mapped_cut = tr.mapping().map(cut.pos, dir);
                            let selection = if textblock {
                                tr.doc().resolve(mapped_cut).ok()
                                    .and_then(|pos| Selection::find_from(&pos, dir, false))
                            } else {
                                let node_pos = if dir < 0 { cut.pos - neighbor.node_size() } else { mapped_cut };
                                Some(Selection::node(node_pos))
                            };
                            if let Some(selection) = selection {
                                tr.set_selection(selection);
                            }
                            return dispatch_scrolled(tr, dispatch);
                        }
                    }
                }
            }
            if dir > 0 || cursor.node(depth - 1).child_count() > 1 {
                break;
            }
        }
    }

    if neighbor.is_atom() && cut.depth + 1 == cursor.depth {
        let (from, to) = if dir < 0 {
            (cut.pos - neighbor.node_size(), cut.pos)
        } else {
            (cut.pos, cut.pos + neighbor.node_size())
        };
        let mut tr = state.tr();
        tr.delete(from, to);
        return dispatch_scrolled(tr, dispatch);
    }
    false
}

fn textblock_at(mut node: &Node, dir: i32, only: bool) -> bool {
    loop {
        if node.is_textblock() {
            return true;
        }
        if only && node.child_count() != 1 {
            return false;
        }
        let next = if dir < 0 { node.content().children.last() } else { node.content().children.first() };
        let Some(next) = next else { return false };
        node = next;
    }
}

pub fn join_textblock_backward(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    join_textblock_at_cursor(state, dispatch, -1)
}

pub fn join_textblock_forward(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    join_textblock_at_cursor(state, dispatch, 1)
}

fn join_textblock_at_cursor(state: &EditorState, dispatch: DispatchFn<'_>, dir: i32) -> bool {
    let Some(cut) = cursor(state).filter(|pos| at_edge(pos, dir)).and_then(|pos| find_cut(state, &pos, dir))
        else { return false };
    let (Some(before), Some(after)) = (cut.node_before(), cut.node_after()) else { return false };
    let Some(from) = textblock_boundary(state, &before, cut.pos, -1) else { return false };
    let Some(to) = textblock_boundary(state, &after, cut.pos, 1) else { return false };
    let Some(step) = replace_step(&state.doc, from, to, Slice::empty()) else { return false };
    match &step {
        Step::Replace { from: step_from, slice, .. } if *step_from != from || slice.size() >= to - from => return false,
        Step::ReplaceAround { from: step_from, .. } if *step_from != from => return false,
        _ => {}
    }
    let mut tr = state.tr();
    if tr.step(step).is_err() {
        return false;
    }
    tr.set_selection(Selection::cursor(from));
    dispatch_scrolled(tr, dispatch)
}

fn textblock_boundary(state: &EditorState, mut node: &Node, cut: usize, dir: i32) -> Option<usize> {
    let mut depth = 1;
    while !node.is_textblock() {
        if isolating(state, node) {
            return None;
        }
        node = if dir < 0 { node.content().children.last()? } else { node.content().children.first()? };
        depth += 1;
    }
    if dir < 0 { cut.checked_sub(depth) } else { cut.checked_add(depth) }
}

fn selectable(state: &EditorState, node: &Node) -> bool {
    !node.is_text() && state.schema.node_type(node.type_name()).is_some_and(|kind| kind.spec.selectable)
}

pub fn select_node_backward(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    select_adjacent_node(state, dispatch, -1)
}

pub fn select_node_forward(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    select_adjacent_node(state, dispatch, 1)
}

fn select_adjacent_node(state: &EditorState, dispatch: DispatchFn<'_>, dir: i32) -> bool {
    if !state.selection.empty_in(&state.doc) {
        return false;
    }
    let Ok(mut cut) = state.doc.resolve(state.selection.head(&state.doc)) else { return false };
    if cut.parent().is_textblock() {
        if !at_edge(&cut, dir) {
            return false;
        }
        let Some(next) = find_cut(state, &cut, dir) else { return false };
        cut = next;
    }
    let adjacent = if dir < 0 { cut.node_before() } else { cut.node_after() };
    let Some(node) = adjacent.filter(|node| selectable(state, node)) else { return false };
    let mut tr = state.tr();
    tr.set_selection(Selection::node(if dir < 0 { cut.pos - node.node_size() } else { cut.pos }));
    dispatch_scrolled(tr, dispatch)
}

pub fn join_up(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    join_selected(state, dispatch, -1)
}

pub fn join_down(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    join_selected(state, dispatch, 1)
}

fn join_selected(state: &EditorState, dispatch: DispatchFn<'_>, dir: i32) -> bool {
    let is_node = matches!(state.selection, Selection::Node { .. });
    let pos = if dir < 0 { state.selection.from() } else { state.selection.to(&state.doc) };
    let point = if is_node {
        let selected = state.doc.resolve(state.selection.from()).ok().and_then(|pos| pos.node_after());
        if !selected.is_some_and(|node| !node.is_textblock()) || !can_join(&state.doc, pos) {
            return false;
        }
        pos
    } else {
        let Some(point) = join_point(&state.doc, pos, dir) else { return false };
        point
    };
    let mut tr = state.tr();
    tr.apply_transform(|transform| { transform.join(point, 1); });
    if is_node && dir < 0 {
        if let Some(before) = state.doc.resolve(point).ok().and_then(|pos| pos.node_before()) {
            tr.set_selection(Selection::node(point - before.node_size()));
        }
    }
    dispatch_scrolled(tr, dispatch)
}

pub fn select_parent_node(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    let Ok(from) = state.doc.resolve(state.selection.from()) else { return false };
    let depth = from.shared_depth(state.selection.to(&state.doc));
    if depth == 0 {
        return false;
    }
    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        tr.set_selection(Selection::node(from.before(depth)));
        dispatch(tr);
    }
    true
}

pub fn select_all(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        tr.set_selection(Selection::all());
        dispatch(tr);
    }
    true
}

pub fn select_textblock_start(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    select_textblock_side(state, dispatch, -1)
}

pub fn select_textblock_end(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    select_textblock_side(state, dispatch, 1)
}

fn select_textblock_side(state: &EditorState, dispatch: DispatchFn<'_>, dir: i32) -> bool {
    let pos = if dir < 0 { state.selection.from() } else { state.selection.to(&state.doc) };
    let Ok(pos) = state.doc.resolve(pos) else { return false };
    let Some(depth) = (0..=pos.depth).rev().find(|&depth| !pos.node(depth).is_inline()) else { return false };
    if !pos.node(depth).is_textblock() {
        return false;
    }
    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        tr.set_selection(Selection::cursor(if dir < 0 { pos.start(depth) } else { pos.end(depth) }));
        dispatch(tr);
    }
    true
}

fn default_block_at<'a>(state: &'a EditorState, parent: &Node, index: usize) -> Option<&'a NodeType> {
    let content = parent.content_match_at(index)?;
    (0..content.edge_count())
        .filter_map(|index| content.edge(index))
        .filter_map(|(name, _)| state.schema.node_type(name))
        .find(|kind| kind.is_textblock() && !kind.has_required_attrs())
}

fn code_selection(state: &EditorState) -> Option<ResolvedPos> {
    let head = state.doc.resolve(state.selection.head(&state.doc)).ok()?;
    let anchor = state.doc.resolve(state.selection.anchor()).ok()?;
    let kind = state.schema.node_type(head.parent().type_name())?;
    (kind.spec.code && head.same_parent(&anchor)).then_some(head)
}

pub fn newline_in_code(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    if code_selection(state).is_none() {
        return false;
    }
    let mut tr = state.tr();
    tr.insert_text_at_selection("\n");
    dispatch_scrolled(tr, dispatch)
}

pub fn exit_code(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    let Some(head) = code_selection(state).filter(|head| head.depth > 0) else { return false };
    let above = head.node(head.depth - 1);
    let after = head.index_after(head.depth - 1);
    let Some(kind) = default_block_at(state, above, after) else { return false };
    if !above.can_replace_with(after, after, &kind.name, &[]) {
        return false;
    }
    let Ok(Some(block)) = kind.create_and_fill(None, None, None) else { return false };
    let pos = head.after(head.depth);
    let mut tr = state.tr();
    tr.insert(pos, Fragment::from_nodes(vec![block]));
    if let Ok(pos) = tr.doc().resolve(pos) {
        tr.set_selection(Selection::near(&pos, 1));
    }
    dispatch_scrolled(tr, dispatch)
}

pub fn create_paragraph_near(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    let (Ok(from), Ok(to)) = (state.doc.resolve(state.selection.from()), state.doc.resolve(state.selection.to(&state.doc)))
        else { return false };
    if state.selection == Selection::All || from.parent().inline_content() || to.parent().inline_content() {
        return false;
    }
    let Some(kind) = default_block_at(state, to.parent(), to.index_after(to.depth)) else { return false };
    let Ok(Some(block)) = kind.create_and_fill(None, None, None) else { return false };
    let side = if from.parent_offset == 0 && to.index(to.depth) < to.parent().child_count() { from.pos } else { to.pos };
    let mut tr = state.tr();
    tr.insert(side, Fragment::from_nodes(vec![block]));
    if tr.doc_changed() {
        tr.set_selection(Selection::cursor(side + 1));
    }
    dispatch_scrolled(tr, dispatch)
}

pub fn lift(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    let Some(tr) = lift_selection(state, &state.selection, 0) else { return false };
    dispatch_scrolled(tr, dispatch)
}

fn lift_selection(state: &EditorState, selection: &Selection, min_depth: usize) -> Option<Transaction> {
    let from = state.doc.resolve(selection.from()).ok()?;
    let to = state.doc.resolve(selection.to(&state.doc)).ok()?;
    let range = from.block_range(&to)?;
    let target = lift_target(&state.doc, &range)?;
    if target < min_depth {
        return None;
    }
    let mut tr = state.tr();
    tr.apply_transform(|transform| { transform.lift(&range, target); });
    tr.doc_changed().then_some(tr)
}

pub fn lift_empty_block(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    let Some(pos) = cursor(state).filter(|pos| pos.parent().content().size() == 0) else { return false };
    if pos.depth > 1 && pos.after(pos.depth) != pos.end(pos.depth - 1) {
        let before = pos.before(pos.depth);
        if can_split(&state.doc, before, 1, None) {
            let mut tr = state.tr();
            tr.apply_transform(|transform| { transform.split(before, 1); });
            return dispatch_scrolled(tr, dispatch);
        }
    }
    lift(state, dispatch)
}

pub fn split_block(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    split_block_as(|_, _, _| None)(state, dispatch)
}

/// The callback returns an empty node carrying the new block's type and attributes.
pub fn split_block_as(
    split_node: impl Fn(&Node, bool, &ResolvedPos) -> Option<Node>,
) -> impl for<'a> Fn(&EditorState, DispatchFn<'a>) -> bool {
    move |state, dispatch| {
        let (Ok(from), Ok(to)) = (state.doc.resolve(state.selection.from()), state.doc.resolve(state.selection.to(&state.doc)))
            else { return false };
        if matches!(state.selection, Selection::Node { .. }) && from.node_after().is_some_and(|node| node.is_block()) {
            if from.parent_offset == 0 || !can_split(&state.doc, from.pos, 1, None) {
                return false;
            }
            let mut tr = state.tr();
            tr.apply_transform(|transform| { transform.split(from.pos, 1); });
            return dispatch_scrolled(tr, dispatch);
        }
        let Some(block_depth) = (1..=from.depth).rev().find(|&depth| from.node(depth).is_block()) else { return false };
        let inline_depth = from.depth - block_depth;
        let at_end = from.end(block_depth) == from.pos + inline_depth;
        let at_start = from.start(block_depth) + inline_depth == from.pos;
        let parent_depth = block_depth - 1;
        let parent = from.node(parent_depth);
        let default = default_block_at(state, parent, from.index_after(parent_depth));
        let default_node = default.map(|kind| kind.create(None, None, None));
        let mut types = vec![None; inline_depth + 1];
        types[0] = split_node(to.parent(), at_end, &from).or_else(|| if at_end { default_node.clone() } else { None });

        let mut tr = state.tr();
        if !matches!(state.selection, Selection::Node { .. }) {
            tr.delete_selection();
        }
        let pos = tr.mapping().map(from.pos, 1);
        if !can_split(tr.doc(), pos, types.len(), Some(&types)) {
            types[0] = default_node;
            if !can_split(tr.doc(), pos, types.len(), Some(&types)) {
                return false;
            }
        }
        tr.apply_transform(|transform| { transform.split_with_types(pos, types.len(), Some(&types)); });
        if !at_end && at_start {
            if let Some(kind) = default.filter(|kind| kind.name != from.node(block_depth).type_name()) {
                let pos = tr.mapping().map(from.before(block_depth), 1);
                if let Ok(first) = tr.doc().resolve(pos) {
                    let index = first.index(first.depth);
                    if parent.can_replace_with(index, index + 1, &kind.name, &[]) {
                        tr.apply_transform(|transform| {
                            transform.set_node_markup(pos, Some(&kind.name), None, None);
                        });
                    }
                }
            }
        }
        dispatch_scrolled(tr, dispatch)
    }
}

pub fn split_block_keep_marks(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    let Some(dispatch) = dispatch else { return split_block(state, None) };
    split_block(state, Some(&mut |mut tr| {
        let marks = state.stored_marks.clone().or_else(|| {
            let to = state.doc.resolve(state.selection.to(&state.doc)).ok()?;
            (to.parent_offset != 0).then(|| state.doc.resolve(state.selection.from()).ok().map(|from| from.marks())).flatten()
        });
        if let Some(marks) = marks {
            tr.ensure_marks(marks);
        }
        dispatch(tr);
    }))
}

fn delete_barrier(state: &EditorState, cut: &ResolvedPos, dir: i32) -> Option<Transaction> {
    let before = cut.node_before()?;
    let after = cut.node_after()?;
    let cut_index = cut.index(cut.depth);
    let isolated_after = isolating(state, &after);
    let isolated = isolated_after || isolating(state, &before);
    let parent = cut.parent();
    let before_type = state.schema.node_type(before.type_name())?;
    let after_type = state.schema.node_type(after.type_name())?;
    let can_delete_after = !isolated && parent.can_replace(cut_index, cut_index + 1, &Fragment::empty());
    if !isolated && before_type.compatible_content(after_type) {
        if before.content().size() == 0 && parent.can_replace(cut_index - 1, cut_index, &Fragment::empty()) {
            let mut tr = state.tr();
            tr.delete(cut.pos - before.node_size(), cut.pos);
            if tr.doc_changed() {
                return Some(tr);
            }
        }
        if can_delete_after && (after.is_textblock() || can_join(&state.doc, cut.pos)) {
            let mut tr = state.tr();
            tr.apply_transform(|transform| { transform.join(cut.pos, 1); });
            if tr.doc_changed() {
                return Some(tr);
            }
        }
    }

    if can_delete_after {
        if let Some(tr) = wrap_at_barrier(state, cut, &before, &after) {
            return Some(tr);
        }
    }
    if !isolated_after && !(dir > 0 && isolated) {
        if let Some(tr) = Selection::find_from(cut, 1, false).and_then(|selection| lift_selection(state, &selection, cut.depth)) {
            return Some(tr);
        }
    }
    if can_delete_after && textblock_at(&after, 1, true) && textblock_at(&before, -1, false) {
        return join_wrapped_textblocks(state, cut, &before, &after);
    }
    None
}

fn wrap_at_barrier(state: &EditorState, cut: &ResolvedPos, before: &Node, after: &Node) -> Option<Transaction> {
    let matched = before.content_match_at(before.child_count())?;
    let wrappers = matched.find_wrapping(&state.schema, after.type_name())?;
    let outer = wrappers.first().map_or_else(|| after.type_name(), String::as_str);
    if !matched.match_type(outer)?.valid_end() {
        return None;
    }
    let mut wrap = Fragment::empty();
    for name in wrappers.iter().rev() {
        let kind = state.schema.node_type(name)?;
        wrap = Fragment::from_nodes(vec![kind.create(None, Some(wrap), None)]);
    }
    wrap = Fragment::from_nodes(vec![before.copy(Some(wrap))]);
    let end = cut.pos + after.node_size();
    let step = Step::replace_around(cut.pos.checked_sub(1)?, end, cut.pos, end, Slice::new(wrap, 1, 0), wrappers.len(), true);
    let mut tr = state.tr();
    tr.step(step).ok()?;
    let join_at = end + 2 * wrappers.len();
    if tr.doc().resolve(join_at).ok().and_then(|pos| pos.node_after()).is_some_and(|node| node.type_name() == before.type_name())
        && can_join(tr.doc(), join_at)
    {
        tr.apply_transform(|transform| { transform.join(join_at, 1); });
    }
    Some(tr)
}

fn join_wrapped_textblocks(state: &EditorState, cut: &ResolvedPos, before: &Node, after: &Node) -> Option<Transaction> {
    let mut at = before;
    let mut wraps = vec![before];
    while !at.is_textblock() {
        at = at.content().children.last()?;
        wraps.push(at);
    }
    let mut after_text = after;
    let mut after_depth = 1;
    while !after_text.is_textblock() {
        after_text = after_text.content().children.first()?;
        after_depth += 1;
    }
    if !at.can_replace(at.child_count(), at.child_count(), after_text.content()) {
        return None;
    }
    let mut end = Fragment::empty();
    for wrap in wraps.iter().rev() {
        end = Fragment::from_nodes(vec![wrap.copy(Some(end))]);
    }
    let after_end = cut.pos + after.node_size();
    let step = Step::replace_around(
        cut.pos.checked_sub(wraps.len())?, after_end, cut.pos + after_depth, after_end - after_depth,
        Slice::new(end, wraps.len(), 0), 0, true,
    );
    let mut tr = state.tr();
    tr.step(step).ok()?;
    Some(tr)
}

pub fn wrap_in(type_name: &str, attrs: Option<Value>) -> impl for<'a> Fn(&EditorState, DispatchFn<'a>) -> bool + '_ {
    move |state, dispatch| {
        let (Ok(from), Ok(to)) = (state.doc.resolve(state.selection.from()), state.doc.resolve(state.selection.to(&state.doc)))
            else { return false };
        let Some(range) = from.block_range(&to) else { return false };
        let Some(wrapping) = find_wrapping(&state.doc, &range, type_name, attrs.clone(), None) else { return false };
        let mut tr = state.tr();
        tr.apply_transform(|transform| { transform.wrap(&range, &wrapping); });
        dispatch_scrolled(tr, dispatch)
    }
}

pub fn set_block_type(type_name: &str, attrs: Option<Value>) -> impl for<'a> Fn(&EditorState, DispatchFn<'a>) -> bool + '_ {
    move |state, dispatch| {
        if !state.schema.node_type(type_name).is_some_and(NodeType::is_textblock) {
            return false;
        }
        let from = state.selection.from();
        let to = state.selection.to(&state.doc);
        let mut tr = state.tr();
        tr.apply_transform(|transform| { transform.set_block_type(from, to, type_name, attrs.clone()); });
        tr.doc_changed() && dispatch_scrolled(tr, dispatch)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ToggleMarkOptions {
    pub remove_when_present: bool,
    pub enter_inline_atoms: bool,
}

impl Default for ToggleMarkOptions {
    fn default() -> Self {
        Self { remove_when_present: true, enter_inline_atoms: true }
    }
}

pub fn toggle_mark(mark_type: &str, attrs: Option<Value>) -> impl for<'a> Fn(&EditorState, DispatchFn<'a>) -> bool {
    toggle_mark_with_options(mark_type, attrs, ToggleMarkOptions::default())
}

pub fn toggle_mark_with_options(
    mark_type: &str,
    attrs: Option<Value>,
    options: ToggleMarkOptions,
) -> impl for<'a> Fn(&EditorState, DispatchFn<'a>) -> bool {
    let type_name = mark_type.to_owned();
    move |state, dispatch| {
        let Some(kind) = state.schema.mark_type(&type_name) else { return false };
        let doc = &state.doc;
        let from = state.selection.from();
        let to = state.selection.to(doc);
        if !mark_applies(state, from, to, &type_name, options.enter_inline_atoms) {
            return false;
        }
        let mut tr = state.tr();
        if let Some(cursor) = cursor(state) {
            let active = state.stored_marks.clone().unwrap_or_else(|| cursor.marks());
            if kind.is_in_set(&active).is_some() {
                tr.remove_stored_mark_type(&type_name);
            } else if let Ok(mark) = kind.try_create(attrs.clone()) {
                tr.add_stored_mark(mark);
            } else {
                return false;
            }
            if let Some(dispatch) = dispatch {
                dispatch(tr);
            }
            return true;
        }
        if from == to {
            return false;
        }

        let ranges = mark_ranges(doc, from, to, options.enter_inline_atoms);
        let add = if options.remove_when_present {
            !ranges.iter().any(|&(from, to)| range_has_mark(doc, from, to, &type_name))
        } else {
            ranges.iter().any(|&(from, to)| range_missing_mark(state, from, to, &type_name))
        };
        if add {
            let Ok(mark) = kind.try_create(attrs.clone()) else { return false };
            for (from, to) in ranges {
                let (from, to) = whitespace_trimmed_range(doc, from, to);
                tr.add_mark(from, to, mark.clone());
            }
        } else {
            for (from, to) in ranges {
                tr.apply_transform(|transform| { transform.remove_marks(from, to, MarkFilter::Type(&type_name)); });
            }
        }
        dispatch_scrolled(tr, dispatch)
    }
}

fn mark_allowed(state: &EditorState, node: &Node, mark_type: &str) -> bool {
    state.schema.node_type(node.type_name()).is_some_and(|kind| kind.allows_mark_type(mark_type))
}

fn mark_applies(state: &EditorState, from: usize, to: usize, mark_type: &str, enter_atoms: bool) -> bool {
    let mut applies = state.doc.resolve(from).is_ok_and(|pos| pos.depth == 0)
        && state.doc.inline_content()
        && mark_allowed(state, &state.doc, mark_type);
    state.doc.nodes_between(from, to, |node, pos, _, _| {
        if applies || (!enter_atoms && node.is_atom() && node.is_inline() && pos >= from && pos + node.node_size() <= to) {
            return false;
        }
        applies = node.inline_content() && mark_allowed(state, node, mark_type);
        !applies
    });
    applies
}

fn mark_ranges(doc: &Node, from: usize, to: usize, enter_atoms: bool) -> Vec<(usize, usize)> {
    if enter_atoms {
        return vec![(from, to)];
    }
    let mut ranges = Vec::new();
    let mut start = from;
    doc.nodes_between(from, to, |node, pos, _, _| {
        if node.is_atom() && node.is_inline() && node.content().size() > 0 && pos >= start && pos + node.node_size() <= to {
            ranges.push((start, pos + 1));
            start = pos + 1 + node.content().size();
            false
        } else {
            true
        }
    });
    if start < to {
        ranges.push((start, to));
    }
    ranges
}

fn range_has_mark(doc: &Node, from: usize, to: usize, mark_type: &str) -> bool {
    let mut present = false;
    doc.nodes_between(from, to, |node, _, _, _| {
        present |= node.marks().iter().any(|mark| mark.type_name == mark_type);
        !present
    });
    present
}

fn range_missing_mark(state: &EditorState, from: usize, to: usize, mark_type: &str) -> bool {
    let mut missing = false;
    state.doc.nodes_between(from, to, |node, pos, parent, _| {
        if missing {
            return false;
        }
        let whitespace = node.is_text() && node.text_between(from.saturating_sub(pos), node.node_size().min(to - pos))
            .chars().all(is_js_whitespace);
        missing = !whitespace && !node.marks().iter().any(|mark| mark.type_name == mark_type)
            && parent.is_some_and(|parent| mark_allowed(state, parent, mark_type));
        !missing
    });
    missing
}

fn whitespace_trimmed_range(doc: &Node, from: usize, to: usize) -> (usize, usize) {
    let leading = doc.resolve(from).ok().and_then(|pos| pos.node_after())
        .filter(Node::is_text).map_or(0, |node| {
            node.text().unwrap_or_default().chars().take_while(|&ch| is_js_whitespace(ch)).map(char::len_utf16).sum()
        });
    let trailing = doc.resolve(to).ok().and_then(|pos| pos.node_before())
        .filter(Node::is_text).map_or(0, |node| {
            node.text().unwrap_or_default().chars().rev().take_while(|&ch| is_js_whitespace(ch)).map(char::len_utf16).sum()
        });
    if from + leading < to {
        (from + leading, to.saturating_sub(trailing))
    } else {
        (from, to)
    }
}

fn is_js_whitespace(ch: char) -> bool {
    ch == '\u{feff}' || (ch.is_whitespace() && ch != '\u{85}')
}

pub fn auto_join(
    command: impl for<'a> Fn(&EditorState, DispatchFn<'a>) -> bool,
    is_joinable: impl Fn(&Node, &Node) -> bool,
) -> impl for<'a> Fn(&EditorState, DispatchFn<'a>) -> bool {
    move |state, dispatch| {
        let Some(dispatch) = dispatch else { return command(state, None) };
        command(state, Some(&mut |mut tr| {
            if tr.is_generic() {
                let mut ranges = Vec::new();
                for map in &tr.mapping().maps {
                    for (from, to) in &mut ranges {
                        *from = map.map(*from, 1);
                        *to = map.map(*to, 1);
                    }
                    map.for_each(|_, _, from, to| ranges.push((from, to)));
                }
                let mut joins = Vec::new();
                for (from, to) in ranges {
                    let Ok(from) = tr.doc().resolve(from) else { continue };
                    let depth = from.shared_depth(to);
                    let parent = from.node(depth);
                    let index = from.index_after(depth);
                    let mut pos = from.after(depth + 1);
                    for child_index in index..parent.child_count() {
                        if pos > to {
                            break;
                        }
                        let after = parent.child(child_index);
                        if child_index > 0 {
                            let before = parent.child(child_index - 1);
                            if before.type_name() == after.type_name() && is_joinable(before, after) {
                                joins.push(pos);
                            }
                        }
                        pos += after.node_size();
                    }
                }
                joins.sort_unstable();
                joins.dedup();
                for pos in joins.into_iter().rev() {
                    if can_join(tr.doc(), pos) {
                        tr.apply_transform(|transform| { transform.join(pos, 1); });
                    }
                }
            }
            dispatch(tr);
        }))
    }
}

pub fn chain_commands<'a>(commands: Vec<BoxedCommand<'a>>) -> impl for<'b> Fn(&EditorState, DispatchFn<'b>) -> bool + 'a {
    move |state, mut dispatch| {
        for command in &commands {
            if command(state, dispatch.as_mut().map(|dispatch| &mut **dispatch as &mut dyn FnMut(Transaction))) {
                return true;
            }
        }
        false
    }
}

fn run_bindings(commands: &[Command], state: &EditorState, mut dispatch: DispatchFn<'_>) -> bool {
    for command in commands {
        if command(state, dispatch.as_mut().map(|dispatch| &mut **dispatch as &mut dyn FnMut(Transaction))) {
            return true;
        }
    }
    false
}

fn enter(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    run_bindings(&[newline_in_code, create_paragraph_near, lift_empty_block, split_block], state, dispatch)
}

fn backspace(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    run_bindings(&[delete_selection, join_backward, select_node_backward], state, dispatch)
}

fn delete(state: &EditorState, dispatch: DispatchFn<'_>) -> bool {
    run_bindings(&[delete_selection, join_forward, select_node_forward], state, dispatch)
}

pub fn pc_base_keymap(key: &str) -> Option<Command> {
    match key {
        "Enter" => Some(enter),
        "Mod-Enter" => Some(exit_code),
        "Backspace" | "Mod-Backspace" | "Shift-Backspace" => Some(backspace),
        "Delete" | "Mod-Delete" => Some(delete),
        "Mod-a" => Some(select_all),
        _ => None,
    }
}

pub fn mac_base_keymap(key: &str) -> Option<Command> {
    match key {
        "Ctrl-h" | "Alt-Backspace" => Some(backspace),
        "Ctrl-d" | "Ctrl-Alt-Backspace" | "Alt-Delete" | "Alt-d" => Some(delete),
        "Ctrl-a" => Some(select_textblock_start),
        "Ctrl-e" => Some(select_textblock_end),
        _ => pc_base_keymap(key),
    }
}

pub fn base_keymap(key: &str) -> Option<Command> {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        mac_base_keymap(key)
    } else {
        pc_base_keymap(key)
    }
}
