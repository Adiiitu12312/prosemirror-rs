//! Live column resize handles and drag events from upstream columnresizing.ts.

use std::rc::Rc;

use prosemirror_model::{Node as ModelNode, Schema};
use prosemirror_state::EditorState;
use prosemirror_view::browser::{BrowserView, BrowserViewProps, GetPos, NodeViewFactory, ViewportPoint, WidgetView};
use prosemirror_view::{Decoration, DecorationGroup};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, EventTarget, HtmlElement, MouseEvent};

use super::drag::{dom_cell, mouse_handler, report, DragSessions, Listener};
use super::tableview::{apply_layout, BrowserTableView};
use crate::columnresizing::{column_decorations, Edge};
use crate::{cell_around, table_node_types, ColumnResizingOptions, ResizeState, TableRect, TableRole};

const RESIZE_WIDGET: u64 = 0x7461_626c_6572_6573;

/// Pair this adapter with [`crate::column_resizing`] in `BrowserViewProps::state_plugins`.
pub fn column_resizing_props(schema: &Schema, options: ColumnResizingOptions) -> Rc<BrowserViewProps> {
    let factory: NodeViewFactory = Rc::new(move |document: &Document, node: &ModelNode, _: GetPos, _: &[Decoration], _: &DecorationGroup| {
        Ok(BrowserTableView::new(document, node.clone(), options.default_cell_min_width)?.into_node_view())
    });
    column_resizing_props_with_view(schema, options, Some(factory))
}

/// Override the table view factory, or pass `None` to leave table rendering to the schema.
pub fn column_resizing_props_with_view(
    schema: &Schema,
    options: ColumnResizingOptions,
    factory: Option<NodeViewFactory>,
) -> Rc<BrowserViewProps> {
    let sessions = Rc::new(DragSessions::default());
    let node_views = table_node_types(schema).get(&TableRole::Table).zip(factory).into_iter()
        .map(|(kind, factory)| (kind.name.clone(), factory)).collect();
    Rc::new(BrowserViewProps {
        attributes: vec![Rc::new(|state| {
            ResizeState::get(state).and_then(|resize| resize.active_handle).map_or_else(Default::default,
                |_| [("class".into(), "resize-cursor".into())].into())
        })],
        decorations: vec![Rc::new(|state| {
            let handle = ResizeState::get(state)?.active_handle?;
            Some(column_decorations(state, Some(handle), |_| RESIZE_WIDGET))
        })],
        node_views,
        widgets: [(RESIZE_WIDGET, Rc::new(|document: &Document, _: &EditorState, _: &Decoration, _: GetPos| {
            let handle = document.create_element("div")?;
            handle.set_class_name("column-resize-handle");
            Ok(WidgetView::new(handle))
        }) as _)].into(),
        handle_dom_events: [
            ("mousemove".into(), mouse_handler(move |view, event| mouse_move(view, event, options))),
            ("mouseleave".into(), mouse_handler(move |view, _| mouse_leave(view, options))),
            ("mousedown".into(), mouse_handler({
                let sessions = sessions.clone();
                move |view, event| mouse_down(view, event, options, &sessions)
            })),
        ].into(),
        plugin_views: vec![sessions.cleanup()],
        ..BrowserViewProps::default()
    })
}

fn mouse_move(view: &BrowserView, event: &MouseEvent, options: ColumnResizingOptions) -> Result<bool, JsValue> {
    let state = view.state();
    if !view.editable() || ResizeState::get(&state).is_none_or(|resize| resize.dragging.is_some()) {
        return Ok(false);
    }
    let hit = dom_cell(view, event).and_then(|target| {
        let bounds = target.get_bounding_client_rect();
        let x = f64::from(event.client_x());
        let edge = options.edge_at(x, bounds.left(), bounds.right())?;
        // Hit-test inside the border, where the browser reports stable cell positions.
        let offset = if edge == Edge::Right { -options.handle_width } else { options.handle_width };
        let position = view.pos_at_coords(ViewportPoint { left: x + offset, top: f64::from(event.client_y()) })?;
        let cell = cell_around(&state.doc.resolve(position.pos).ok()?)?;
        Some((cell.pos, edge))
    });
    if let Some(transaction) = options.hover(&state, hit) {
        view.dispatch(transaction)?;
    }
    Ok(false)
}

fn mouse_leave(view: &BrowserView, options: ColumnResizingOptions) -> Result<bool, JsValue> {
    if view.editable() {
        if let Some(transaction) = options.hover(&view.state(), None) {
            view.dispatch(transaction)?;
        }
    }
    Ok(false)
}

fn mouse_down(
    view: &BrowserView,
    event: &MouseEvent,
    options: ColumnResizingOptions,
    sessions: &Rc<DragSessions>,
) -> Result<bool, JsValue> {
    if !view.editable() {
        return Ok(false);
    }
    let state = view.state();
    let Some(handle) = ResizeState::get(&state).filter(|resize| resize.dragging.is_none()).and_then(|resize| resize.active_handle)
    else { return Ok(false) };
    let Some(window) = view.dom().owner_document().and_then(|doc| doc.default_view()).or_else(web_sys::window)
    else { return Ok(false) };
    let x = f64::from(event.client_x());
    let width = view.dom_at_pos(handle, 0).and_then(|(dom, offset)| dom.child_nodes().item(offset))
        .and_then(|cell| cell.dyn_into::<HtmlElement>().ok()).map_or(0.0, |cell| f64::from(cell.offset_width()));
    let Some(transaction) = options.start_drag(&state, x, width) else { return Ok(false) };
    view.dispatch(transaction)?;
    preview(view, options, x)?;

    let target: EventTarget = window.into();
    let weak = view.downgrade();
    let sessions_weak = Rc::downgrade(sessions);
    let mouseup = Listener::new(&target, "mouseup", {
        let weak = weak.clone();
        let sessions = sessions_weak.clone();
        move |event| {
            if let Some(((view, sessions), event)) = weak.upgrade().zip(sessions.upgrade()).zip(event.dyn_ref::<MouseEvent>()) {
                report(finish(&view, &sessions, options, f64::from(event.client_x())));
            }
        }
    })?;
    let mousemove = Listener::new(&target, "mousemove", move |event| {
        let Some(((view, sessions), event)) = weak.upgrade().zip(sessions_weak.upgrade()).zip(event.dyn_ref::<MouseEvent>())
        else { return };
        let x = f64::from(event.client_x());
        if event.buttons() == 0 && event.which() == 0 {
            report(finish(&view, &sessions, options, x));
        } else {
            report(preview(&view, options, x));
        }
    })?;
    sessions.start(view.dom(), vec![mouseup, mousemove]);
    event.prevent_default();
    Ok(true)
}

fn finish(view: &BrowserView, sessions: &DragSessions, options: ColumnResizingOptions, x: f64) -> Result<(), JsValue> {
    sessions.stop(&view.dom());
    let state = view.state();
    if ResizeState::get(&state).is_some_and(|resize| resize.dragging.is_some()) {
        if let Some(transaction) = options.commit_width(&state, x) {
            view.dispatch(transaction)?;
        }
        view.dispatch(options.end_drag(&view.state()))?;
    }
    Ok(())
}

fn preview(view: &BrowserView, options: ColumnResizingOptions, x: f64) -> Result<(), JsValue> {
    let state = view.state();
    let Some(layout) = options.preview(&state, x) else { return Ok(()) };
    let Some(handle) = ResizeState::get(&state).and_then(|resize| resize.active_handle) else { return Ok(()) };
    let Some(rect) = TableRect::from_cells(&state.doc, handle, handle) else { return Ok(()) };
    let mut dom = view.dom_at_pos(rect.table_start, 0).map(|(dom, _)| dom);
    while let Some(current) = dom {
        if current.node_name() == "TABLE" {
            let table: HtmlElement = current.dyn_into()?;
            if let Some(colgroup) = table.first_element_child().filter(|element| element.tag_name() == "COLGROUP") {
                return apply_layout(&colgroup.dyn_into()?, &table, &layout);
            }
            break;
        }
        dom = current.parent_node();
    }
    Ok(())
}
