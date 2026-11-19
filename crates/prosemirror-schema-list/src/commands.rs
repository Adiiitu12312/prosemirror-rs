use prosemirror_model::{Fragment, Node, NodeRange, ResolvedPos, Slice};
use prosemirror_state::{EditorState, Selection, Transaction};
use prosemirror_transform::{can_join, can_split, find_wrapping, lift_target, Step};
use serde_json::Value;

pub type DispatchFn<'a> = Option<&'a mut dyn FnMut(Transaction)>;

pub fn wrap_in_list(name: &str, attrs: Option<Value>) -> impl for<'a> Fn(&EditorState, DispatchFn<'a>) -> bool + '_ {
    move |state, dispatch| {
        let Some(tr) = wrap_list_transaction(state, name, attrs.clone()) else { return false };
        finish(tr, dispatch)
    }
}

fn wrap_list_transaction(state: &EditorState, name: &str, attrs: Option<Value>) -> Option<Transaction> {
    let (from, to) = selection_positions(state)?;
    let mut range = from.block_range(&to)?;
    let mut outer = range.clone();
    let mut join = false;
    let kind = state.schema.node_type(name)?;
    if range.depth >= 2 && range.start_index == 0
        && from.node(range.depth - 1).node_type().is_some_and(|parent| parent.compatible_content(kind))
    {
        if from.index(range.depth - 1) == 0 {
            return None;
        }
        let insert = state.doc.resolve(range.start.checked_sub(2)?).ok()?;
        outer = range_at(&insert, &insert, range.depth);
        if range.end_index < from.node(range.depth).child_count() {
            let end = state.doc.resolve(to.end(range.depth)).ok()?;
            range = range_at(&from, &end, range.depth);
        }
        join = true;
    }
    let wrappers = find_wrapping(&state.doc, &outer, name, attrs, Some(&range))?;
    let offset = if join { 2 } else { 0 };
    let mut tr = state.tr();
    let mut content = Fragment::empty();
    for node in wrappers.iter().rev() {
        content = Fragment::from_nodes(vec![node.copy(Some(content))]);
    }
    tr.step(Step::replace_around(
        range.start.checked_sub(offset)?, range.end, range.start, range.end,
        Slice::new(content, 0, 0), wrappers.len(), true,
    )).ok()?;
    let list = wrappers.iter().rposition(|wrapper| wrapper.type_name() == name)?;
    let split_depth = wrappers.len() - list - 1;
    let mut pos = range.start + wrappers.len() - offset;
    let parent = from.node(range.depth);
    for index in range.start_index..range.end_index {
        if index > range.start_index && can_split(tr.doc(), pos, split_depth, None) {
            tr.apply_transform(|transform| { transform.split(pos, split_depth); });
            pos += 2 * split_depth;
        }
        pos += parent.child(index).node_size();
    }
    Some(tr)
}

pub fn split_list_item(name: &str, attrs: Option<Value>) -> impl for<'a> Fn(&EditorState, DispatchFn<'a>) -> bool + '_ {
    move |state, dispatch| {
        let Some(tr) = split_list_transaction(state, name, attrs.clone()) else { return false };
        finish(tr, dispatch)
    }
}

fn split_list_transaction(state: &EditorState, name: &str, attrs: Option<Value>) -> Option<Transaction> {
    let (from, to) = selection_positions(state)?;
    if from.depth < 2 || !from.same_parent(&to)
        || (matches!(state.selection, Selection::Node { .. }) && from.node_after().is_some_and(|node| node.is_block()))
    {
        return None;
    }
    let item_depth = from.depth - 1;
    let item = from.node(item_depth);
    if item.type_name() != name {
        return None;
    }
    if from.parent().content().size() == 0 && item.child_count() == from.index_after(item_depth) {
        return split_nested_empty_item(state, &from, name);
    }
    let mut tr = state.tr();
    tr.delete(from.pos, to.pos);
    let types = if to.pos == from.end(from.depth) {
        item.content_match_at(0).and_then(|matched| {
            let next = state.schema.node_type(matched.default_type()?)?;
            let item_type = state.schema.node_type(name)?;
            let item = attrs.map(|attrs| item_type.create(Some(attrs), None, None));
            Some([item, Some(next.create(None, None, None))])
        })
    } else {
        None
    };
    let types = types.as_ref().map(<[_; 2]>::as_slice);
    if !can_split(tr.doc(), from.pos, 2, types) {
        return None;
    }
    tr.apply_transform(|transform| { transform.split_with_types(from.pos, 2, types); });
    Some(tr)
}

fn split_nested_empty_item(state: &EditorState, from: &ResolvedPos, name: &str) -> Option<Transaction> {
    let depth = from.depth;
    if depth <= 3 || from.node(depth - 3).type_name() != name
        || from.index(depth - 2) + 1 != from.node(depth - 2).child_count()
    {
        return None;
    }
    let depth_before = if from.index(depth - 1) != 0 { 1 } else if from.index(depth - 2) != 0 { 2 } else { 3 };
    let mut wrap = Fragment::empty();
    for level in (depth - 3..=depth - depth_before).rev() {
        wrap = Fragment::from_nodes(vec![from.node(level).copy(Some(wrap))]);
    }
    let depth_after = if from.index_after(depth - 1) < from.node(depth - 2).child_count() {
        1
    } else if from.index_after(depth - 2) < from.node(depth - 3).child_count() {
        2
    } else {
        3
    };
    let item = state.schema.node_type(name)?.create_and_fill(None, None, None).ok()??;
    wrap = wrap.append(&Fragment::from_nodes(vec![item]));
    let start = from.before(depth - (depth_before - 1));
    let mut tr = state.tr();
    tr.replace(start, from.after(depth - depth_after), Slice::new(wrap, 4 - depth_before, 0));
    let mut selection = None;
    tr.doc().nodes_between(start, tr.doc().content().size(), |node, pos, _, _| {
        if selection.is_none() && node.is_textblock() && node.content().size() == 0 {
            selection = Some(pos + 1);
        }
        selection.is_none()
    });
    if let Some(pos) = selection.and_then(|pos| tr.doc().resolve(pos).ok()) {
        tr.set_selection(Selection::near(&pos, 1));
    }
    Some(tr)
}

pub fn split_list_item_keep_marks(
    name: &str,
    attrs: Option<Value>,
) -> impl for<'a> Fn(&EditorState, DispatchFn<'a>) -> bool + '_ {
    let split = split_list_item(name, attrs);
    move |state, dispatch| {
        let Some(dispatch) = dispatch else { return split(state, None) };
        split(state, Some(&mut |mut tr| {
            let marks = state.stored_marks.clone().or_else(|| {
                let (from, to) = selection_positions(state)?;
                (to.parent_offset != 0).then(|| from.marks())
            });
            if let Some(marks) = marks {
                tr.ensure_marks(marks);
            }
            dispatch(tr);
        }))
    }
}

pub fn lift_list_item(name: &str) -> impl for<'a> Fn(&EditorState, DispatchFn<'a>) -> bool + '_ {
    move |state, dispatch| {
        let Some((from, to)) = selection_positions(state) else { return false };
        let Some(range) = item_range(&from, &to, name) else { return false };
        if dispatch.is_none() {
            return true;
        }
        let tr = if range.depth > 0 && from.node(range.depth - 1).type_name() == name {
            lift_to_outer_list(state, &from, &to, name, range)
        } else {
            lift_out_of_list(state, from.node(range.depth), &range)
        };
        tr.is_some_and(|tr| finish(tr, dispatch))
    }
}

fn lift_to_outer_list(
    state: &EditorState, from: &ResolvedPos, to: &ResolvedPos, name: &str, mut range: NodeRange,
) -> Option<Transaction> {
    let mut tr = state.tr();
    let end = range.end;
    let end_of_list = to.end(range.depth);
    if end < end_of_list {
        let parent = from.node(range.depth).copy(None);
        let item = state.schema.node_type(name)?.create(None, Some(Fragment::from_nodes(vec![parent])), None);
        tr.step(Step::replace_around(
            end - 1, end_of_list, end, end_of_list, Slice::new(Fragment::from_nodes(vec![item]), 1, 0), 1, true,
        )).ok()?;
        range = range_at(&tr.doc().resolve(from.pos).ok()?, &tr.doc().resolve(end_of_list).ok()?, range.depth);
    }
    let target = lift_target(tr.doc(), &range)?;
    tr.apply_transform(|transform| { transform.lift(&range, target); });
    if let Some(after) = tr.mapping().map(end, -1).checked_sub(1) {
        if can_join(tr.doc(), after) {
            tr.apply_transform(|transform| { transform.join(after, 1); });
        }
    }
    Some(tr)
}

fn lift_out_of_list(state: &EditorState, list: &Node, range: &NodeRange) -> Option<Transaction> {
    let mut tr = state.tr();
    let mut pos = range.end;
    for index in (range.start_index + 1..range.end_index).rev() {
        pos -= list.child(index).node_size();
        tr.delete(pos - 1, pos + 1);
    }
    let start = tr.doc().resolve(range.start).ok()?;
    let item = start.node_after()?;
    let end = start.pos + item.node_size();
    if tr.mapping().map(range.end, 1) != end {
        return None;
    }
    let at_start = range.start_index == 0;
    let at_end = range.end_index == list.child_count();
    let outer_depth = start.depth.checked_sub(1)?;
    let parent = start.node(outer_depth);
    let index_before = start.index(outer_depth);
    let suffix = if at_end { Fragment::empty() } else { Fragment::from_nodes(vec![list.clone()]) };
    if !parent.can_replace(index_before + usize::from(!at_start), index_before + 1, &item.content().append(&suffix)) {
        return None;
    }
    let open_start = usize::from(!at_start);
    let open_end = usize::from(!at_end);
    let wraps = if open_start + open_end == 0 {
        Fragment::empty()
    } else {
        Fragment::from_nodes(vec![list.copy(None); open_start + open_end])
    };
    tr.step(Step::replace_around(
        start.pos - usize::from(at_start), end + usize::from(at_end), start.pos + 1, end - 1,
        Slice::new(wraps, open_start, open_end), open_start, false,
    )).ok()?;
    Some(tr)
}

pub fn sink_list_item(name: &str) -> impl for<'a> Fn(&EditorState, DispatchFn<'a>) -> bool + '_ {
    move |state, dispatch| {
        let Some(tr) = sink_list_transaction(state, name) else { return false };
        finish(tr, dispatch)
    }
}

fn sink_list_transaction(state: &EditorState, name: &str) -> Option<Transaction> {
    let (from, to) = selection_positions(state)?;
    let range = item_range(&from, &to, name)?;
    let parent = from.node(range.depth);
    let previous = parent.child(range.start_index.checked_sub(1)?);
    if previous.type_name() != name {
        return None;
    }
    let nested = previous.content().children.last().is_some_and(|node| node.type_name() == parent.type_name());
    let item = state.schema.node_type(name)?;
    let list = parent.node_type()?;
    let inner = if nested { Fragment::from_nodes(vec![item.create(None, None, None)]) } else { Fragment::empty() };
    let list = list.create(None, Some(inner), None);
    let slice = Fragment::from_nodes(vec![item.create(None, Some(Fragment::from_nodes(vec![list])), None)]);
    let open = if nested { 3 } else { 1 };
    let mut tr = state.tr();
    tr.step(Step::replace_around(
        range.start.checked_sub(open)?, range.end, range.start, range.end, Slice::new(slice, open, 0), 1, true,
    )).ok()?;
    Some(tr)
}

fn selection_positions(state: &EditorState) -> Option<(ResolvedPos, ResolvedPos)> {
    Some((state.doc.resolve(state.selection.from()).ok()?, state.doc.resolve(state.selection.to(&state.doc)).ok()?))
}

fn item_range(from: &ResolvedPos, to: &ResolvedPos, name: &str) -> Option<NodeRange> {
    from.block_range_with(to, |node| node.content().children.first().is_some_and(|child| child.type_name() == name))
}

fn range_at(from: &ResolvedPos, to: &ResolvedPos, depth: usize) -> NodeRange {
    NodeRange {
        start: from.before(depth + 1),
        end: to.after(depth + 1),
        depth,
        start_index: from.index(depth),
        end_index: to.index_after(depth),
    }
}

fn finish(mut tr: Transaction, dispatch: DispatchFn<'_>) -> bool {
    if let Some(dispatch) = dispatch {
        tr.scroll_into_view();
        dispatch(tr);
    }
    true
}
