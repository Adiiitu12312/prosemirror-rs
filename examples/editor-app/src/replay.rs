use std::{error::Error, fs, path::Path};

use prosemirror_editor::{
    ClipboardContent, ClipboardEvent, CompositionEvent, DragEvent, Editor, Event, KeyInput,
    Layout, Modifiers, Point, PointerButton, PointerEvent, PointerInput, TouchEvent,
};
use prosemirror_keymap::Platform;
use prosemirror_model::Slice;
use prosemirror_state::Selection;
use serde::Deserialize;
use serde_json::Value;

use crate::{setup, surface::SurfaceHost};

#[derive(Deserialize)]
struct Replay {
    width: Option<u32>,
    height: Option<u32>,
    doc: Option<Value>,
    selection: Option<Value>,
    events: Vec<Input>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Target {
    Pixel { x: f64, y: f64 },
    Caret { pos: usize },
    Block { block: usize, #[serde(default)] after: bool },
}

impl Target {
    fn point(self, host: &SurfaceHost) -> Result<Point, Box<dyn Error>> {
        match self {
            Self::Pixel { x, y } => Ok(host.document_point(Point { x, y })),
            Self::Caret { pos } => host.coords_at_pos(pos, 1).map(|rect| rect.center())
                .ok_or_else(|| format!("No caret geometry at position {pos}").into()),
            Self::Block { block, after } => host.block_bounds(block).map(|rect| Point {
                x: rect.center().x,
                y: if after { rect.bottom - 1.0 } else { rect.top + 1.0 },
            }).ok_or_else(|| format!("No block geometry at position {block}").into()),
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Input {
    Focus,
    Blur,
    Text { text: String },
    Key {
        key: String,
        #[serde(default)] control: bool,
        #[serde(default)] shift: bool,
        #[serde(default)] alt: bool,
        #[serde(default)] meta: bool,
    },
    CompositionStart,
    CompositionUpdate { text: String, from: usize, to: usize },
    CompositionCommit { text: String },
    CompositionCancel,
    PointerDown { at: Target, #[serde(default = "single_click")] clicks: u8 },
    PointerMove { at: Target },
    PointerUp { at: Target },
    TouchTap { at: Target },
    TouchLongPress { at: Target },
    TouchDragStart { at: Target },
    TouchDrag { at: Target },
    TouchDragEnd { at: Target },
    Copy,
    Cut,
    Paste,
    PasteText { text: String },
    DragBlock { pos: usize },
    DragOver { at: Target },
    Drop { at: Target, slice: Option<Value>, #[serde(default)] copy: bool },
    Scroll { x: f64, y: f64 },
}

fn single_click() -> u8 { 1 }

fn pointer(point: Point, click_count: u8) -> PointerInput {
    PointerInput { point, button: PointerButton::Primary, click_count, modifiers: Modifiers::default() }
}

impl Input {
    fn translate(self, host: &SurfaceHost) -> Result<Event, Box<dyn Error>> {
        let event = match self {
            Self::Focus => Event::Focus,
            Self::Blur => Event::Blur,
            Self::Text { text } => Event::TextInput(text),
            Self::Key { key, control, shift, alt, meta } => Event::KeyDown(KeyInput {
                key, code: None, modifiers: Modifiers { control, shift, alt, meta }, repeating: false,
            }),
            Self::CompositionStart => Event::Composition(CompositionEvent::Start),
            Self::CompositionUpdate { text, from, to } => {
                Event::Composition(CompositionEvent::Update { text, selection: from..to })
            }
            Self::CompositionCommit { text } => Event::Composition(CompositionEvent::Commit(text)),
            Self::CompositionCancel => Event::Composition(CompositionEvent::Cancel),
            Self::PointerDown { at, clicks } => Event::Pointer(PointerEvent::Down(pointer(at.point(host)?, clicks))),
            Self::PointerMove { at } => Event::Pointer(PointerEvent::Move(at.point(host)?)),
            Self::PointerUp { at } => Event::Pointer(PointerEvent::Up(pointer(at.point(host)?, 1))),
            Self::TouchTap { at } => Event::Touch(TouchEvent::Tap(at.point(host)?)),
            Self::TouchLongPress { at } => Event::Touch(TouchEvent::LongPress(at.point(host)?)),
            Self::TouchDragStart { at } => Event::Touch(TouchEvent::DragStart(at.point(host)?)),
            Self::TouchDrag { at } => Event::Touch(TouchEvent::Drag(at.point(host)?)),
            Self::TouchDragEnd { at } => Event::Touch(TouchEvent::DragEnd(at.point(host)?)),
            Self::Copy => Event::Clipboard(ClipboardEvent::Copy),
            Self::Cut => Event::Clipboard(ClipboardEvent::Cut),
            Self::Paste => Event::Clipboard(ClipboardEvent::Paste),
            Self::PasteText { text } => Event::Clipboard(ClipboardEvent::PasteContent {
                content: ClipboardContent { plain_text: text, internal: None }, plain_text: true,
            }),
            Self::DragBlock { pos } => Event::Drag(DragEvent::StartBlock { pos }),
            Self::DragOver { at } => Event::Drag(DragEvent::Over(at.point(host)?)),
            Self::Drop { at, slice, copy } => Event::Drag(DragEvent::Drop {
                point: at.point(host)?, slice: slice.map(|value| parse_slice(&value)).transpose()?, copy,
            }),
            Self::Scroll { x, y } => Event::Scroll(Point { x, y }),
        };
        Ok(event)
    }
}

fn parse_slice(value: &Value) -> Result<Slice, Box<dyn Error>> {
    Ok(setup::schema().slice_from_json(value)?)
}

pub(super) fn run(path: &Path, png: Option<&Path>) -> Result<(), Box<dyn Error>> {
    let script: Replay = serde_json::from_reader(fs::File::open(path)?)?;
    let width = script.width.unwrap_or(720);
    let height = script.height.unwrap_or(560);
    if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
        return Err("Replay dimensions must be in 1..=4096".into());
    }
    let doc = match script.doc {
        Some(data) => setup::schema().node_from_json(&data)?,
        None => setup::sample(),
    };
    let selection = script.selection.map(|value| Selection::from_json(&doc, &value)).transpose()?;
    let platform = Platform::Other;
    let mut editor = Editor::with_platform(
        setup::initialize(doc, selection), SurfaceHost::deterministic(width, height),
        setup::props(platform), platform,
    );
    for (index, input) in script.events.into_iter().enumerate() {
        let event = input.translate(editor.host()).map_err(|error| format!("Event {}: {error}", index + 1))?;
        editor.handle(event).map_err(|error| format!("Event {}: {error}", index + 1))?;
    }
    if let Some(path) = png {
        editor.host().save_png(path)?;
    }
    println!("{}", editor.state().to_json());
    Ok(())
}
