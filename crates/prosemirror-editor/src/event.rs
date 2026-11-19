//! Input shared by native hosts and scripted replay.

use prosemirror_model::Slice;

use crate::{ClipboardContent, Movement, Point};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub alt: bool,
    pub control: bool,
    pub meta: bool,
    pub shift: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyInput {
    pub key: String,
    pub code: Option<u16>,
    pub modifiers: Modifiers,
    pub repeating: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerButton {
    #[default]
    Primary,
    Secondary,
    Middle,
}

#[derive(Clone, Copy, Debug)]
pub struct PointerInput {
    pub point: Point,
    pub button: PointerButton,
    pub click_count: u8,
    pub modifiers: Modifiers,
}

#[derive(Clone, Copy, Debug)]
pub enum PointerEvent {
    Down(PointerInput),
    Move(Point),
    Up(PointerInput),
    Cancel,
}

#[derive(Clone, Copy, Debug)]
pub enum TouchEvent {
    Tap(Point),
    LongPress(Point),
    DragStart(Point),
    Drag(Point),
    DragEnd(Point),
    Cancel,
}

#[derive(Clone, Debug)]
pub enum CompositionEvent {
    Start,
    Update { text: String, selection: std::ops::Range<usize> },
    Commit(String),
    Cancel,
}

#[derive(Clone, Debug)]
pub enum ClipboardEvent {
    Copy,
    Cut,
    Paste,
    PasteContent { content: ClipboardContent, plain_text: bool },
}

#[derive(Clone, Debug)]
pub enum DragEvent {
    Start,
    StartBlock { pos: usize },
    Over(Point),
    Drop { point: Point, slice: Option<Slice>, copy: bool },
    Cancel,
}

#[derive(Clone, Debug)]
pub enum Event {
    KeyDown(KeyInput),
    KeyUp(KeyInput),
    TextInput(String),
    Move { movement: Movement, extend: bool },
    Composition(CompositionEvent),
    Pointer(PointerEvent),
    Touch(TouchEvent),
    Focus,
    Blur,
    Scroll(Point),
    Clipboard(ClipboardEvent),
    Drag(DragEvent),
}
