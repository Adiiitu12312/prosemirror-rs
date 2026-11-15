use prosemirror_state::EditorState;
use crate::layout::{LayoutRect, ViewLayout};

#[derive(Clone, Debug, PartialEq)]
pub struct TouchHandle {
    pub pos: usize,
    pub caret_rect: LayoutRect,
    pub grabber_rect: LayoutRect,
    pub hit_target: LayoutRect,
}

#[derive(Clone, Debug)]
pub struct SelectionHandles {
    pub anchor_handle: Option<TouchHandle>,
    pub head_handle: Option<TouchHandle>,
    pub loupe_rect: Option<LayoutRect>,
}

pub struct HandlePhysics {
    pub touch_target_size: f32, // 44.0 pt Apple HIG standard
    pub grabber_radius: f32,
    pub loupe_offset_y: f32,
}

impl Default for HandlePhysics {
    fn default() -> Self {
        Self {
            touch_target_size: 44.0,
            grabber_radius: 8.0,
            loupe_offset_y: -60.0,
        }
    }
}

impl SelectionHandles {
    pub fn compute(
        state: &EditorState,
        layout: &ViewLayout,
        physics: &HandlePhysics,
        is_dragging: bool,
        drag_pos: Option<(f32, f32)>,
    ) -> Self {
        if state.selection.empty() {
            let pos = state.selection.from();
            let caret = layout.pos_to_coords(pos);
            let handle = TouchHandle {
                pos,
                caret_rect: caret.clone(),
                grabber_rect: LayoutRect {
                    x: caret.x - physics.grabber_radius,
                    y: caret.y + caret.height,
                    width: physics.grabber_radius * 2.0,
                    height: physics.grabber_radius * 2.0,
                },
                hit_target: LayoutRect {
                    x: caret.x - physics.touch_target_size / 2.0,
                    y: caret.y + caret.height - 10.0,
                    width: physics.touch_target_size,
                    height: physics.touch_target_size,
                },
            };

            let loupe = if is_dragging {
                if let Some((dx, dy)) = drag_pos {
                    Some(LayoutRect {
                        x: dx - 40.0,
                        y: dy + physics.loupe_offset_y,
                        width: 80.0,
                        height: 50.0,
                    })
                } else {
                    None
                }
            } else {
                None
            };

            Self {
                anchor_handle: Some(handle),
                head_handle: None,
                loupe_rect: loupe,
            }
        } else {
            let from = state.selection.from();
            let to = state.selection.to(&state.doc);

            let from_caret = layout.pos_to_coords(from);
            let to_caret = layout.pos_to_coords(to);

            let start_handle = TouchHandle {
                pos: from,
                caret_rect: from_caret.clone(),
                grabber_rect: LayoutRect {
                    x: from_caret.x - physics.grabber_radius * 2.0,
                    y: from_caret.y - physics.grabber_radius * 2.0,
                    width: physics.grabber_radius * 2.0,
                    height: physics.grabber_radius * 2.0,
                },
                hit_target: LayoutRect {
                    x: from_caret.x - physics.touch_target_size,
                    y: from_caret.y - physics.touch_target_size / 2.0,
                    width: physics.touch_target_size,
                    height: physics.touch_target_size,
                },
            };

            let end_handle = TouchHandle {
                pos: to,
                caret_rect: to_caret.clone(),
                grabber_rect: LayoutRect {
                    x: to_caret.x,
                    y: to_caret.y + to_caret.height,
                    width: physics.grabber_radius * 2.0,
                    height: physics.grabber_radius * 2.0,
                },
                hit_target: LayoutRect {
                    x: to_caret.x - 10.0,
                    y: to_caret.y + to_caret.height - 10.0,
                    width: physics.touch_target_size,
                    height: physics.touch_target_size,
                },
            };

            let loupe = if is_dragging {
                if let Some((dx, dy)) = drag_pos {
                    Some(LayoutRect {
                        x: dx - 40.0,
                        y: dy + physics.loupe_offset_y,
                        width: 80.0,
                        height: 50.0,
                    })
                } else {
                    None
                }
            } else {
                None
            };

            Self {
                anchor_handle: Some(start_handle),
                head_handle: Some(end_handle),
                loupe_rect: loupe,
            }
        }
    }
}
