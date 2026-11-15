use prosemirror_model::{Fragment, Mark, MarkType, Node, NodeType, Slice};
use prosemirror_state::{EditorState, Selection, Transaction};

pub type DispatchFn<'a> = Option<&'a mut dyn FnMut(Transaction)>;

pub fn delete_selection(state: &EditorState, dispatch: DispatchFn) -> bool {
    if state.selection.empty() {
        return false;
    }
    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        tr.delete(state.selection.from(), state.selection.to(&state.doc));
        dispatch(tr);
    }
    true
}

pub fn join_backward(state: &EditorState, dispatch: DispatchFn) -> bool {
    if !state.selection.empty() {
        return false;
    }
    let pos = state.selection.from();
    if pos == 0 {
        return false;
    }

    let rpos = match state.doc.resolve(pos) {
        Ok(r) => r,
        Err(_) => return false,
    };

    if rpos.parent().is_block() && rpos.start(rpos.depth) == pos {
        // At the start of a block, join with preceding block
        if let Some(dispatch) = dispatch {
            let mut tr = state.tr();
            tr.delete(pos - 1, pos + 1);
            dispatch(tr);
        }
        return true;
    }

    // Otherwise backspace deletes single char/code-unit before cursor
    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        tr.delete(pos - 1, pos);
        dispatch(tr);
    }
    true
}

pub fn join_forward(state: &EditorState, dispatch: DispatchFn) -> bool {
    if !state.selection.empty() {
        return false;
    }
    let pos = state.selection.from();
    let doc_size = state.doc.content().size();
    if pos >= doc_size {
        return false;
    }

    let rpos = match state.doc.resolve(pos) {
        Ok(r) => r,
        Err(_) => return false,
    };

    if rpos.parent().is_block() && rpos.end(rpos.depth) == pos {
        // At the end of a block, join with following block
        if let Some(dispatch) = dispatch {
            let mut tr = state.tr();
            tr.delete(pos, pos + 2);
            dispatch(tr);
        }
        return true;
    }

    // Otherwise delete key deletes single char/code-unit after cursor
    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        tr.delete(pos, pos + 1);
        dispatch(tr);
    }
    true
}

pub fn split_block(state: &EditorState, dispatch: DispatchFn) -> bool {
    let pos = state.selection.from();
    let rpos = match state.doc.resolve(pos) {
        Ok(r) => r,
        Err(_) => return false,
    };

    let parent = rpos.parent();
    if !parent.is_block() {
        return false;
    }

    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        if !state.selection.empty() {
            tr.delete(state.selection.from(), state.selection.to(&state.doc));
        }

        let slice = Slice::new(
            Fragment::from_nodes(vec![
                parent.copy(Some(Fragment::empty())),
                parent.copy(Some(Fragment::empty())),
            ]),
            1,
            1,
        );
        tr.replace(pos, pos, slice);
        dispatch(tr);
    }
    true
}

pub fn select_parent_node(state: &EditorState, dispatch: DispatchFn) -> bool {
    let pos = state.selection.from();
    let rpos = match state.doc.resolve(pos) {
        Ok(r) => r,
        Err(_) => return false,
    };

    if rpos.depth == 0 {
        return false;
    }

    if let Some(dispatch) = dispatch {
        let mut tr = state.tr();
        tr.set_selection(Selection::node(rpos.before(rpos.depth)));
        dispatch(tr);
    }
    true
}

pub fn toggle_mark(mark_type: &str, attrs: Option<serde_json::Value>) -> impl Fn(&EditorState, DispatchFn) -> bool {
    let mark_name = mark_type.to_string();
    move |state: &EditorState, dispatch: DispatchFn| {
        let mark = Mark::new(mark_name.clone(), attrs.clone());
        if state.selection.empty() {
            let mut stored = state.stored_marks.clone().unwrap_or_else(|| {
                if let Ok(rpos) = state.doc.resolve(state.selection.from()) {
                    rpos.marks()
                } else {
                    Vec::new()
                }
            });

            if mark.is_in_set(&stored) {
                stored = Mark::remove_from_set(&stored, &mark.type_name);
            } else {
                stored = Mark::add_to_set(&stored, mark);
            }

            if let Some(dispatch) = dispatch {
                let mut tr = state.tr();
                tr.set_stored_marks(Some(stored));
                dispatch(tr);
            }
            true
        } else {
            let from = state.selection.from();
            let to = state.selection.to(&state.doc);
            let has_mark = doc_has_mark(&state.doc, from, to, &mark_name);

            if let Some(dispatch) = dispatch {
                let mut tr = state.tr();
                if has_mark {
                    tr.remove_mark(from, to, mark);
                } else {
                    tr.add_mark(from, to, mark);
                }
                dispatch(tr);
            }
            true
        }
    }
}

fn doc_has_mark(doc: &Node, from: usize, to: usize, mark_type: &str) -> bool {
    let mut found = false;
    let mut cur = 0;
    for child in &doc.content().children {
        let end = cur + child.node_size();
        if end > from && cur < to {
            if child.is_text() {
                if child.marks().iter().any(|m| m.type_name == mark_type) {
                    found = true;
                    break;
                }
            } else if doc_has_mark(child, if from > cur { from - cur - 1 } else { 0 }, to - cur - 1, mark_type) {
                found = true;
                break;
            }
        }
        cur = end;
    }
    found
}

pub fn chain_commands<'a>(
    commands: Vec<Box<dyn Fn(&EditorState, DispatchFn) -> bool + 'a>>,
) -> impl Fn(&EditorState, DispatchFn) -> bool + 'a {
    move |state: &EditorState, mut dispatch: DispatchFn| {
        for cmd in &commands {
            if let Some(ref mut d) = dispatch {
                if cmd(state, Some(*d)) {
                    return true;
                }
            } else if cmd(state, None) {
                return true;
            }
        }
        false
    }
}
