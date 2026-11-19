//! Native drop cursor behavior from prosemirror-dropcursor@1.8.4.

#[cfg(all(target_arch = "wasm32", feature = "browser"))]
mod browser;

#[cfg(all(target_arch = "wasm32", feature = "browser"))]
pub use browser::{browser_plugin, browser_plugin_with_disabled, DisableDropCursor};

use prosemirror_model::{Node, Slice};
use prosemirror_transform::drop_point;
use prosemirror_view::LayoutRect;

#[derive(Clone, Debug, PartialEq)]
pub struct DropCursorOptions {
    pub width: f32,
    pub color: Option<String>,
    pub class: Option<String>,
}

impl Default for DropCursorOptions {
    fn default() -> Self {
        Self { width: 1.0, color: Some("black".into()), class: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DropPosition {
    pub pos: usize,
    pub inside: Option<usize>,
}

pub struct DragOver<'a> {
    pub editable: bool,
    pub target: Option<DropPosition>,
    pub dragging: Option<&'a Slice>,
    pub time_ms: u64,
}

/// All positions are measured in viewport coordinates. A host that renders in
/// local coordinates can convert the resulting overlay from the viewport.
pub trait DropCursorGeometry {
    fn position_rect(&self, pos: usize) -> Option<LayoutRect>;
    fn node_rect(&self, pos: usize) -> Option<LayoutRect>;

    fn scale(&self) -> (f32, f32) {
        (1.0, 1.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DropCursorOverlay<'a> {
    pub rect: LayoutRect,
    pub block: bool,
    pub color: Option<&'a str>,
    pub class: Option<&'a str>,
}

#[derive(Default)]
pub struct DropCursor {
    options: DropCursorOptions,
    cursor: Option<usize>,
    remove_at: Option<u64>,
    drag_slice: Option<Slice>,
}

impl DropCursor {
    pub fn new(options: DropCursorOptions) -> Self {
        Self { options, ..Self::default() }
    }

    pub fn position(&self) -> Option<usize> {
        self.cursor
    }

    pub fn drag_over(&mut self, doc: &Node, event: DragOver<'_>, disabled: impl FnOnce(&Node, DropPosition) -> bool) {
        if !event.editable {
            return;
        }
        self.drag_slice = event.dragging.cloned();
        if let Some(target) = drop_target(doc, event.target, self.drag_slice.as_ref(), disabled) {
            self.cursor = Some(target);
            self.schedule_removal(event.time_ms, 5_000);
        }
    }

    /// Recompute the overlay after a document edit during an active drag.
    /// Supply a fresh hit test of the last pointer coordinates.
    pub fn update_drag(
        &mut self, doc: &Node, target: Option<DropPosition>,
        disabled: impl FnOnce(&Node, DropPosition) -> bool,
    ) {
        if self.cursor.is_some() {
            self.cursor = drop_target(doc, target, self.drag_slice.as_ref(), disabled);
        }
    }

    /// Remove the active overlay when the native drop cursor is detached.
    pub fn destroy(&mut self) {
        self.clear();
    }

    pub fn drag_end(&mut self, time_ms: u64) {
        self.schedule_removal(time_ms, 20);
    }

    pub fn drop(&mut self, time_ms: u64) {
        self.schedule_removal(time_ms, 20);
    }

    pub fn drag_leave(&mut self, still_inside: bool) {
        if !still_inside {
            self.clear();
        }
    }

    pub fn update(&mut self, doc: &Node) {
        if self.cursor.is_some_and(|pos| pos > doc.content().size()) {
            self.clear();
        }
    }

    pub fn tick(&mut self, time_ms: u64) {
        if self.remove_at.is_some_and(|deadline| time_ms >= deadline) {
            self.clear();
        }
    }

    pub fn clear(&mut self) {
        self.cursor = None;
        self.remove_at = None;
        self.drag_slice = None;
    }

    fn schedule_removal(&mut self, time_ms: u64, delay: u64) {
        self.remove_at = Some(time_ms.saturating_add(delay));
    }

    pub fn overlay(&self, doc: &Node, geometry: &impl DropCursorGeometry) -> Option<DropCursorOverlay<'_>> {
        let pos = doc.resolve(self.cursor?).ok()?;
        let block = !pos.parent().inline_content();
        let (scale_x, scale_y) = geometry.scale();
        let mut rect = None;
        if block {
            let before = pos.node_before();
            let after = pos.node_after();
            if before.is_some() || after.is_some() {
                let start = pos.pos - before.as_ref().map_or(0, Node::node_size);
                if let Some(node) = geometry.node_rect(start) {
                    let mut y = if before.is_some() { node.y + node.height } else { node.y };
                    if before.is_some() && after.is_some() {
                        if let Some(next) = geometry.node_rect(pos.pos) {
                            y = (y + next.y) / 2.0;
                        }
                    }
                    let width = self.options.width * scale_y;
                    rect = Some(LayoutRect { x: node.x, y: y - width / 2.0, width: node.width, height: width });
                }
            }
        }
        let rect = rect.or_else(|| {
            let coords = geometry.position_rect(pos.pos)?;
            let width = self.options.width * scale_x;
            Some(LayoutRect { x: coords.x - width / 2.0, y: coords.y, width, height: coords.height })
        })?;
        Some(DropCursorOverlay {
            rect,
            block,
            color: self.options.color.as_deref(),
            class: self.options.class.as_deref(),
        })
    }
}

fn drop_target(
    doc: &Node, target: Option<DropPosition>, slice: Option<&Slice>,
    disabled: impl FnOnce(&Node, DropPosition) -> bool,
) -> Option<usize> {
    let target = target.filter(|target| target.pos <= doc.content().size())?;
    if target.inside.and_then(|inside| doc.node_at(inside)).is_some_and(|node| {
        node.node_type().is_some_and(|kind| kind.spec.disable_drop_cursor) || disabled(node, target)
    }) {
        return None;
    }
    Some(slice.and_then(|slice| drop_point(doc, target.pos, slice)).unwrap_or(target.pos))
}
