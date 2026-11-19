//! Browser event handlers from upstream input.ts and tableEditing.

use std::rc::Rc;
use std::sync::Arc;

use prosemirror_keymap::{keymap, Dispatch, KeyCommand, Platform};
use prosemirror_state::{EditorState, Selection};
use prosemirror_view::browser::{BrowserView, BrowserViewProps, TextblockDirection, ViewEventHandler, ViewportPoint};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Event, EventTarget, KeyboardEvent, MouseEvent};

use super::drag::{dom_cell, mouse_handler, report, DragSessions, Listener};
use crate::{cell_around, delete_cell_selection, draw_cell_selection, handle_paste, handle_triple_click, Axis, TableRect, TABLE_EDITING_KEY};
use crate::input::{arrow, shift_arrow, Direction};

/// Pair this adapter with [`crate::table_editing`] in `BrowserViewProps::state_plugins`.
pub fn table_editing_props() -> Rc<BrowserViewProps> {
    let sessions = Rc::new(DragSessions::default());
    Rc::new(BrowserViewProps {
        decorations: vec![Rc::new(draw_cell_selection)],
        handle_dom_events: [("mousedown".into(), mouse_handler({
            let sessions = sessions.clone();
            move |view, event| mouse_down(view, event, &sessions)
        }))].into(),
        plugin_views: vec![sessions.cleanup()],
        create_selection_between: Some(Rc::new(|view, _, _| {
            let state = view.state();
            active_anchor(&state).map(|_| state.selection)
        })),
        handle_triple_click: Some(Rc::new(|view, pos, _| {
            handle_triple_click(&view.state(), Some(&mut |tr| report(view.dispatch(tr).map(|_| ()))), pos)
        })),
        handle_key_down: Some(key_handler()),
        handle_paste: Some(Rc::new(|view, _, slice| {
            handle_paste(&view.state(), Some(&mut |tr| report(view.dispatch(tr).map(|_| ()))), slice.clone())
        })),
        ..BrowserViewProps::default()
    })
}

fn key_handler() -> ViewEventHandler<KeyboardEvent> {
    use Axis::{Horizontal, Vertical};
    use Direction::{Backward, Forward};

    let motion = |axis, dir, extend| -> KeyCommand<BrowserView> {
        Arc::new(move |state: &EditorState, dispatch: Dispatch<'_>, view: &BrowserView| {
            let at_edge = |axis, dir| {
                let direction = match (axis, dir) {
                    (Horizontal, Backward) => TextblockDirection::Left,
                    (Horizontal, Forward) => TextblockDirection::Right,
                    (Vertical, Backward) => TextblockDirection::Up,
                    (Vertical, Forward) => TextblockDirection::Down,
                };
                view.end_of_textblock(direction, None)
            };
            if extend {
                shift_arrow(state, Some(dispatch), axis, dir, at_edge)
            } else {
                arrow(state, Some(dispatch), axis, dir, at_edge)
            }
        })
    };
    let delete: KeyCommand<BrowserView> = Arc::new(|state: &EditorState, dispatch: Dispatch<'_>, _: &BrowserView| {
        delete_cell_selection(state, Some(dispatch))
    });
    keymap([
        ("ArrowLeft", motion(Horizontal, Backward, false)),
        ("ArrowRight", motion(Horizontal, Forward, false)),
        ("ArrowUp", motion(Vertical, Backward, false)),
        ("ArrowDown", motion(Vertical, Forward, false)),
        ("Shift-ArrowLeft", motion(Horizontal, Backward, true)),
        ("Shift-ArrowRight", motion(Horizontal, Forward, true)),
        ("Shift-ArrowUp", motion(Vertical, Backward, true)),
        ("Shift-ArrowDown", motion(Vertical, Forward, true)),
        ("Backspace", delete.clone()),
        ("Mod-Backspace", delete.clone()),
        ("Delete", delete.clone()),
        ("Mod-Delete", delete),
    ], Platform::browser()).expect("valid table key bindings").into_browser_handler()
}

fn active_anchor(state: &EditorState) -> Option<usize> {
    usize::try_from(state.get_plugin_state(TABLE_EDITING_KEY)?.as_u64()?).ok()
}

fn cell_under_mouse(view: &BrowserView, event: &MouseEvent) -> Option<usize> {
    let hit = view.pos_at_coords(ViewportPoint { left: f64::from(event.client_x()), top: f64::from(event.client_y()) })?;
    let state = view.state();
    hit.inside.into_iter().chain([hit.pos]).find_map(|pos| {
        cell_around(&state.doc.resolve(pos).ok()?).map(|cell| cell.pos)
    })
}

fn set_cell_selection(view: &BrowserView, anchor: usize, event: &MouseEvent) -> Result<(), JsValue> {
    let state = view.state();
    let starting = active_anchor(&state).is_none();
    let head = cell_under_mouse(view, event).filter(|&head| TableRect::from_cells(&state.doc, anchor, head).is_some());
    let Some(head) = head.or(starting.then_some(anchor)) else { return Ok(()) };
    let Ok(selection) = Selection::cell(&state.doc, anchor, head) else { return Ok(()) };
    if starting || state.selection != selection {
        let mut transaction = state.tr();
        transaction.set_selection(selection);
        if starting {
            transaction.set_meta(TABLE_EDITING_KEY, anchor.into());
        }
        view.dispatch(transaction)?;
    }
    Ok(())
}

fn stop(view: &BrowserView, sessions: &DragSessions) -> Result<(), JsValue> {
    sessions.stop(&view.dom());
    let state = view.state();
    if active_anchor(&state).is_some() {
        let mut transaction = state.tr();
        transaction.set_meta(TABLE_EDITING_KEY, (-1).into());
        view.dispatch(transaction)?;
    }
    Ok(())
}

fn mouse_down(view: &BrowserView, start: &MouseEvent, sessions: &Rc<DragSessions>) -> Result<bool, JsValue> {
    if start.button() != 0 || start.ctrl_key() || start.meta_key() {
        return Ok(false);
    }
    let initial_cell = dom_cell(view, start);
    let state = view.state();
    let shift_anchor = if start.shift_key() {
        if let Selection::Cell(selection) = &state.selection {
            Some(selection.anchor)
        } else if initial_cell.is_some() {
            state.doc.resolve(state.selection.anchor()).ok().and_then(|pos| cell_around(&pos))
                .map(|cell| cell.pos).filter(|&anchor| cell_under_mouse(view, start) != Some(anchor))
        } else { None }
    } else { None };
    if let Some(anchor) = shift_anchor {
        set_cell_selection(view, anchor, start)?;
        start.prevent_default();
    } else if initial_cell.is_none() {
        return Ok(false);
    }

    let target: EventTarget = view.root().into();
    let weak = view.downgrade();
    let sessions_weak = Rc::downgrade(sessions);
    let stop_listener = |kind| {
        let weak = weak.clone();
        let sessions = sessions_weak.clone();
        Listener::new(&target, kind, move |_| {
            if let Some((view, sessions)) = weak.upgrade().zip(sessions.upgrade()) {
                report(stop(&view, &sessions));
            }
        })
    };
    let mouseup = stop_listener("mouseup")?;
    let dragstart = stop_listener("dragstart")?;
    let start_event = start.clone();
    let mousemove = Listener::new(&target, "mousemove", move |event: Event| {
        let Some((view, sessions)) = weak.upgrade().zip(sessions_weak.upgrade()) else { return };
        let Some(event) = event.dyn_ref::<MouseEvent>() else { return };
        let state = view.state();
        let anchor = if let Some(anchor) = active_anchor(&state) {
            anchor
        } else {
            let same_cell = match (initial_cell.as_ref(), dom_cell(&view, event).as_ref()) {
                (Some(initial), Some(current)) => initial.is_same_node(Some(current.as_ref())),
                (None, None) => true,
                _ => false,
            };
            if same_cell {
                return;
            }
            let Some(anchor) = cell_under_mouse(&view, &start_event) else {
                report(stop(&view, &sessions));
                return;
            };
            anchor
        };
        report(set_cell_selection(&view, anchor, event));
    })?;
    sessions.start(view.dom(), vec![mouseup, dragstart, mousemove]);
    Ok(start.default_prevented())
}
