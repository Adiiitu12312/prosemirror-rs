//! Winit window, native input, and platform services for the sample editor.

use std::error::Error;
use std::num::NonZeroU32;
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

use prosemirror_editor::{
    BlockUpdate, ClipboardContent, ClipboardEvent, CompositionDisplay, CompositionEvent, CursorIcon,
    DisplayList, DragEvent, Editor, EditorError, Event, Host, KeyInput, Layout, LineBox, Modifiers,
    Point, PointerButton, PointerEvent, PointerInput, PositionAtCoords, Rect, TouchEvent,
};
use prosemirror_keymap::Platform;
use prosemirror_model::Node;
use prosemirror_state::Selection;
use prosemirror_view::AccessibilityTreeUpdate;
use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon as WindowCursor, Window, WindowId};

use crate::{setup, surface::SurfaceHost, system_clipboard::SystemClipboard};

const CLICK_INTERVAL: Duration = Duration::from_millis(450);
const LONG_PRESS: Duration = Duration::from_millis(500);
const TOUCH_MOUSE_DELAY: Duration = Duration::from_millis(300);
const PREEDIT_CANCEL_DELAY: Duration = Duration::from_millis(40);
const GUTTER_WIDTH: f64 = 24.0;

type WindowSurface = Surface<OwnedDisplayHandle, Rc<Window>>;

pub fn run() -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    let context = Context::new(event_loop.owned_display_handle())?;
    let mut app = Application { context, desktop: None, failure: None };
    event_loop.run_app(&mut app)?;
    app.failure.map_or(Ok(()), Err)
}

struct Application {
    context: Context<OwnedDisplayHandle>,
    desktop: Option<Desktop>,
    failure: Option<Box<dyn Error>>,
}

impl ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.desktop.is_none() {
            match Desktop::new(event_loop, &self.context) {
                Ok(desktop) => self.desktop = Some(desktop),
                Err(error) => {
                    self.failure = Some(error);
                    event_loop.exit();
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(desktop) = self.desktop.as_mut().filter(|desktop| desktop.window.id() == id) else { return };
        if matches!(event, WindowEvent::CloseRequested | WindowEvent::Destroyed) {
            event_loop.exit();
        } else if let Err(error) = desktop.window_event(event) {
            self.failure = Some(error);
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(desktop) = &mut self.desktop {
            if let Err(error) = desktop.process_deadlines(Instant::now()) {
                self.failure = Some(error.into());
                event_loop.exit();
            }
            desktop.sync_platform();
            event_loop.set_control_flow(desktop.next_deadline().map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
        }
    }
}

struct Desktop {
    window: Rc<Window>,
    drawable: WindowSurface,
    editor: Editor<DesktopHost>,
    platform: Platform,
    modifiers: Modifiers,
    pointer: Option<Point>,
    mouse: Option<MousePress>,
    last_click: Option<Click>,
    touch: Option<TouchSession>,
    last_touch: Option<Instant>,
    ime_allowed: bool,
    ime_clear_at: Option<Instant>,
    reported_caret: Option<Rect>,
    reported_cursor: Option<CursorIcon>,
}

impl Desktop {
    fn new(event_loop: &ActiveEventLoop, context: &Context<OwnedDisplayHandle>) -> Result<Self, Box<dyn Error>> {
        let window = Rc::new(event_loop.create_window(Window::default_attributes()
            .with_title("ProseMirror Rust Editor")
            .with_inner_size(LogicalSize::new(960.0, 680.0))
            .with_min_inner_size(LogicalSize::new(420.0, 300.0)))?);
        let drawable = Surface::new(context, Rc::clone(&window))?;
        let size = window.inner_size();
        let host = DesktopHost {
            surface: SurfaceHost::new(size.width, size.height),
            clipboard: SystemClipboard::default(),
            ghost_at: None,
            drag_hit: None,
            touch_carets: [None, None],
        };
        let platform = Platform::current();
        let editor = Editor::with_platform(setup::initialize(setup::sample(), None), host, setup::props(platform), platform);
        window.request_redraw();
        Ok(Self {
            window, drawable, editor, platform, modifiers: Modifiers::default(), pointer: None,
            mouse: None, last_click: None, touch: None, last_touch: None, ime_allowed: false,
            ime_clear_at: None, reported_caret: None, reported_cursor: None,
        })
    }

    fn window_event(&mut self, event: WindowEvent) -> Result<(), Box<dyn Error>> {
        match event {
            WindowEvent::RedrawRequested => self.present()?,
            WindowEvent::Resized(size) => self.resize(size),
            WindowEvent::ScaleFactorChanged { .. } => self.resize(self.window.inner_size()),
            WindowEvent::Focused(focused) => self.focus(focused)?,
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = editor_modifiers(modifiers.state()),
            WindowEvent::KeyboardInput { event, is_synthetic, .. } => {
                if !is_synthetic || event.state == ElementState::Released {
                    self.keyboard(event)?;
                }
            }
            WindowEvent::Ime(event) => self.ime(event)?,
            WindowEvent::CursorMoved { position, .. } => self.cursor_moved(point(position))?,
            WindowEvent::CursorLeft { .. } => {
                self.pointer = None;
                self.cancel_mouse()?;
            }
            WindowEvent::MouseInput { state, button, .. } => self.mouse_button(state, button)?,
            WindowEvent::MouseWheel { delta, .. } => self.mouse_wheel(delta)?,
            WindowEvent::Touch(touch) => self.touch(touch)?,
            _ => {}
        }
        self.sync_platform();
        Ok(())
    }

    fn route(&mut self, event: Event) -> Result<bool, EditorError> {
        let handled = self.editor.handle(event)?;
        if handled {
            self.window.request_redraw();
        }
        Ok(handled)
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        let surface = &mut self.editor.host_mut().surface;
        if (size.width, size.height) != (surface.width(), surface.height()) {
            surface.resize(size.width, size.height);
            self.editor.redraw();
        }
        self.window.request_redraw();
    }

    fn present(&mut self) -> Result<(), Box<dyn Error>> {
        let size = self.window.inner_size();
        let (Some(width), Some(height)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return Ok(()) };
        let surface = &mut self.editor.host_mut().surface;
        if (surface.width(), surface.height()) != (size.width, size.height) {
            surface.resize(size.width, size.height);
            self.editor.redraw();
        }
        self.drawable.resize(width, height)?;
        let mut buffer = self.drawable.buffer_mut()?;
        buffer.copy_from_slice(self.editor.host().surface.pixels());
        self.window.pre_present_notify();
        buffer.present()?;
        Ok(())
    }

    fn focus(&mut self, focused: bool) -> Result<(), EditorError> {
        if !focused {
            self.cancel_mouse()?;
            if self.touch.take().is_some() {
                self.route(Event::Touch(TouchEvent::Cancel))?;
            }
            self.cancel_ime()?;
            self.modifiers = Modifiers::default();
            self.last_click = None;
        }
        self.route(if focused { Event::Focus } else { Event::Blur })?;
        Ok(())
    }

    fn sync_platform(&mut self) {
        if self.editor.host_mut().surface.take_focus_request() && !self.window.has_focus() {
            self.window.focus_window();
        }
        let allowed = self.editor.has_focus() && self.editor.editable();
        if self.ime_allowed != allowed {
            self.ime_allowed = allowed;
            self.window.set_ime_allowed(allowed);
            self.reported_caret = None;
        }
        let caret = self.editor.host().surface.ime_candidate_rect();
        if allowed && self.reported_caret != caret {
            if let Some(caret) = caret {
                let position = PhysicalPosition::new(caret.left.round() as i32, caret.top.round() as i32);
                let area = PhysicalSize::new(caret.width().ceil().max(1.0) as u32, caret.height().ceil().max(1.0) as u32);
                self.window.set_ime_cursor_area(position, area);
            }
            self.reported_caret = caret;
        }
        let cursor = if self.mouse.as_ref().is_some_and(|press| press.dragging) {
            CursorIcon::Grabbing
        } else if self.pointer.and_then(|at| self.gutter_block(self.document_point(at))).is_some() && self.editor.editable() {
            CursorIcon::Grab
        } else {
            self.editor.host().surface.cursor_icon()
        };
        if self.reported_cursor != Some(cursor) {
            self.window.set_cursor(window_cursor(cursor));
            self.reported_cursor = Some(cursor);
        }
    }

    fn keyboard(&mut self, event: KeyEvent) -> Result<(), EditorError> {
        let key = match &event.logical_key {
            Key::Character(key) => key.to_string(),
            Key::Named(NamedKey::Space) => " ".into(),
            Key::Named(key) => format!("{key:?}"),
            Key::Dead(_) => "Dead".into(),
            Key::Unidentified(_) => "Unidentified".into(),
        };
        if event.state == ElementState::Pressed && key == "Escape" && self.mouse.as_ref().is_some_and(|press| press.dragging) {
            return self.cancel_mouse();
        }
        let input = KeyInput { key, code: None, modifiers: self.modifiers, repeating: event.repeat };
        if event.state == ElementState::Released {
            self.route(Event::KeyUp(input))?;
            return Ok(());
        }
        if self.primary_modifier() && self.modifiers.shift && input.key.eq_ignore_ascii_case("v") && !self.editor.composing() {
            if let Some(content) = self.editor.host_mut().read_clipboard() {
                self.route(Event::Clipboard(ClipboardEvent::PasteContent { content, plain_text: true }))?;
            }
        } else if !self.route(Event::KeyDown(input))? && !self.editor.composing()
            && !self.modifiers.meta && (!self.modifiers.control || self.modifiers.alt)
        {
            if let Some(text) = event.text.filter(|text| !text.is_empty() && !text.chars().any(char::is_control)) {
                self.route(Event::TextInput(text.to_string()))?;
            }
        }
        Ok(())
    }

    fn ime(&mut self, ime: Ime) -> Result<(), EditorError> {
        match ime {
            Ime::Enabled => {}
            Ime::Disabled => self.cancel_ime()?,
            Ime::Preedit(text, caret) => {
                if !text.is_empty() {
                    self.ime_clear_at = None;
                    if !self.editor.composing() {
                        self.route(Event::Composition(CompositionEvent::Start))?;
                    }
                } else if self.editor.composing() {
                    // Winit clears preedit before a commit. Wait briefly for that commit.
                    self.ime_clear_at = Some(Instant::now() + PREEDIT_CANCEL_DELAY);
                }
                if self.editor.composing() {
                    let selection = preedit_selection(&text, caret);
                    self.route(Event::Composition(CompositionEvent::Update { text, selection }))?;
                }
            }
            Ime::Commit(text) => {
                self.ime_clear_at = None;
                self.route(Event::Composition(CompositionEvent::Commit(text)))?;
            }
        }
        Ok(())
    }

    fn cancel_ime(&mut self) -> Result<(), EditorError> {
        self.ime_clear_at = None;
        if self.editor.composing() {
            self.route(Event::Composition(CompositionEvent::Cancel))?;
        }
        Ok(())
    }

    fn next_deadline(&self) -> Option<Instant> {
        let touch = self.touch.as_ref().and_then(|touch| match touch.mode {
            TouchMode::Pending(at) => Some(at),
            _ => None,
        });
        [touch, self.ime_clear_at].into_iter().flatten().min()
    }

    fn process_deadlines(&mut self, now: Instant) -> Result<(), EditorError> {
        if self.ime_clear_at.is_some_and(|at| now >= at) {
            self.cancel_ime()?;
        }
        if let Some(touch) = self.touch.as_mut().filter(|touch| matches!(touch.mode, TouchMode::Pending(at) if now >= at)) {
            touch.mode = TouchMode::Held;
            let target = touch.last;
            self.route(Event::Touch(TouchEvent::LongPress(self.document_point(target))))?;
        }
        Ok(())
    }

    fn document_point(&self, window: Point) -> Point {
        self.editor.host().surface.document_point(window)
    }

    fn gutter_block(&self, point: Point) -> Option<usize> {
        let viewport = self.editor.host().viewport()?;
        if !(viewport.left..viewport.left + GUTTER_WIDTH).contains(&point.x) {
            return None;
        }
        let mut result = None;
        self.editor.state().doc.for_each(|node, pos, _| {
            if result.is_none() && node.is_selectable()
                && self.editor.host().block_bounds(pos).is_some_and(|block| {
                    block.top <= point.y && point.y <= block.bottom && point.x < block.left
                        && block.left - point.x <= GUTTER_WIDTH
                })
            {
                result = Some(pos);
            }
        });
        result
    }

    fn block_drop_hit(&self, point: Point) -> Option<PositionAtCoords> {
        top_level_drop_hit(&self.editor.state().doc, &self.editor.host().surface, point)
    }

    fn primary_modifier(&self) -> bool {
        if matches!(self.platform, Platform::MacOs | Platform::Ios) { self.modifiers.meta } else { self.modifiers.control }
    }

    fn copy_modifier(&self) -> bool {
        if matches!(self.platform, Platform::MacOs | Platform::Ios) { self.modifiers.alt } else { self.modifiers.control }
    }

    fn cursor_moved(&mut self, window: Point) -> Result<(), EditorError> {
        self.pointer = Some(window);
        let Some(mut press) = self.mouse.take() else { return Ok(()) };
        let at = self.document_point(window);
        press.moved |= distance_squared(press.origin, window) > (5.0 * self.window.scale_factor()).powi(2);
        if let Some(pos) = press.block {
            if press.moved && !press.dragging {
                self.editor.host_mut().ghost_at = Some(at);
                press.dragging = self.route(Event::Drag(DragEvent::StartBlock { pos }))?;
                if !press.dragging {
                    self.editor.host_mut().ghost_at = None;
                }
            }
            if press.dragging {
                let hit = self.block_drop_hit(at);
                let host = self.editor.host_mut();
                host.ghost_at = Some(at);
                host.drag_hit = hit;
                self.route(Event::Drag(DragEvent::Over(at)))?;
            }
        } else {
            self.route(Event::Pointer(PointerEvent::Move(at)))?;
        }
        self.mouse = Some(press);
        Ok(())
    }

    fn mouse_button(&mut self, state: ElementState, button: MouseButton) -> Result<(), EditorError> {
        if self.touch.is_some() || self.last_touch.is_some_and(|at| at.elapsed() < TOUCH_MOUSE_DELAY) {
            return Ok(());
        }
        let (Some(window), Some(button)) = (self.pointer, pointer_button(button)) else { return Ok(()) };
        let at = self.document_point(window);
        if state == ElementState::Pressed {
            if self.mouse.is_some() {
                return Ok(());
            }
            let count = click_count(self.last_click, button, window, Instant::now(), self.window.scale_factor());
            let block = (button == PointerButton::Primary && self.editor.editable())
                .then(|| self.gutter_block(at)).flatten();
            self.mouse = Some(MousePress { origin: window, button, count, block, moved: false, dragging: false });
            if block.is_none() {
                self.route(Event::Pointer(PointerEvent::Down(PointerInput { point: at, button, click_count: count, modifiers: self.modifiers })))?;
            }
        } else if let Some(press) = self.mouse.take_if(|press| press.button == button) {
            if !press.moved {
                self.last_click = Some(Click { button, at: window, time: Instant::now(), count: press.count });
            } else {
                self.last_click = None;
            }
            if press.dragging {
                let hit = self.block_drop_hit(at);
                let host = self.editor.host_mut();
                host.ghost_at = None;
                host.drag_hit = hit;
                let result = self.route(Event::Drag(DragEvent::Drop { point: at, slice: None, copy: self.copy_modifier() }));
                self.editor.host_mut().drag_hit = None;
                result?;
            } else if let Some(pos) = press.block.filter(|_| !press.moved) {
                let mut tr = self.editor.state().tr();
                tr.set_selection(Selection::node(pos)).scroll_into_view();
                if self.editor.dispatch(tr)? {
                    self.window.request_redraw();
                }
            } else if press.block.is_none() {
                self.route(Event::Pointer(PointerEvent::Up(PointerInput {
                    point: at, button, click_count: press.count, modifiers: self.modifiers,
                })))?;
            }
        }
        Ok(())
    }

    fn cancel_mouse(&mut self) -> Result<(), EditorError> {
        if let Some(press) = self.mouse.take() {
            let host = self.editor.host_mut();
            host.ghost_at = None;
            host.drag_hit = None;
            let event = if press.dragging { Event::Drag(DragEvent::Cancel) } else { Event::Pointer(PointerEvent::Cancel) };
            self.route(event)?;
        }
        self.last_click = None;
        Ok(())
    }

    fn mouse_wheel(&mut self, delta: MouseScrollDelta) -> Result<(), EditorError> {
        let mut delta = match delta {
            MouseScrollDelta::LineDelta(x, y) => {
                let line = 36.0 * self.window.scale_factor();
                Point { x: -f64::from(x) * line, y: -f64::from(y) * line }
            }
            MouseScrollDelta::PixelDelta(position) => Point { x: -position.x, y: -position.y },
        };
        if self.modifiers.shift && delta.x == 0.0 {
            std::mem::swap(&mut delta.x, &mut delta.y);
        }
        self.route(Event::Scroll(delta))?;
        Ok(())
    }

    fn touch(&mut self, touch: Touch) -> Result<(), EditorError> {
        let at = point(touch.location);
        let now = Instant::now();
        self.last_touch = Some(now);
        if touch.phase == TouchPhase::Started {
            if self.touch.is_none() {
                self.cancel_mouse()?;
                let hit = self.editor.host().touch_handle(self.document_point(at), 14.0 * self.window.scale_factor());
                let mode = if let Some(caret) = hit {
                    self.route(Event::Touch(TouchEvent::DragStart(caret)))?;
                    TouchMode::Selecting
                } else {
                    TouchMode::Pending(now + LONG_PRESS)
                };
                self.touch = Some(TouchSession { id: touch.id, origin: at, last: at, mode });
            }
            return Ok(());
        }
        let Some(mut current) = self.touch.take_if(|current| current.id == touch.id) else { return Ok(()) };
        let target = self.document_point(at);
        let moved = distance_squared(current.origin, at) > (8.0 * self.window.scale_factor()).powi(2);
        match touch.phase {
            TouchPhase::Moved => {
                if moved {
                    current.mode = match current.mode {
                        TouchMode::Pending(_) => TouchMode::Scrolling,
                        TouchMode::Held => {
                            self.route(Event::Touch(TouchEvent::DragStart(self.document_point(current.last))))?;
                            TouchMode::Selecting
                        }
                        mode => mode,
                    };
                }
                match current.mode {
                    TouchMode::Selecting => { self.route(Event::Touch(TouchEvent::Drag(target)))?; }
                    TouchMode::Scrolling => {
                        self.route(Event::Scroll(Point { x: current.last.x - at.x, y: current.last.y - at.y }))?;
                    }
                    _ => {}
                }
                current.last = at;
                self.touch = Some(current);
            }
            TouchPhase::Ended => match current.mode {
                TouchMode::Pending(at) if now >= at && !moved => { self.route(Event::Touch(TouchEvent::LongPress(target)))?; }
                TouchMode::Pending(_) if !moved => { self.route(Event::Touch(TouchEvent::Tap(target)))?; }
                TouchMode::Selecting => { self.route(Event::Touch(TouchEvent::DragEnd(target)))?; }
                _ => {}
            },
            TouchPhase::Cancelled => { self.route(Event::Touch(TouchEvent::Cancel))?; }
            TouchPhase::Started => unreachable!(),
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Click {
    button: PointerButton,
    at: Point,
    time: Instant,
    count: u8,
}

struct MousePress {
    origin: Point,
    button: PointerButton,
    count: u8,
    block: Option<usize>,
    moved: bool,
    dragging: bool,
}

#[derive(Clone, Copy)]
enum TouchMode {
    Pending(Instant),
    Held,
    Selecting,
    Scrolling,
}

struct TouchSession {
    id: u64,
    origin: Point,
    last: Point,
    mode: TouchMode,
}

fn click_count(last: Option<Click>, button: PointerButton, at: Point, now: Instant, scale: f64) -> u8 {
    last.filter(|last| last.button == button && now.duration_since(last.time) < CLICK_INTERVAL
        && distance_squared(last.at, at) <= (5.0 * scale).powi(2))
        .map_or(1, |last| 1 + last.count % 3)
}

fn point(position: PhysicalPosition<f64>) -> Point {
    Point { x: position.x, y: position.y }
}

fn top_level_drop_hit(doc: &Node, layout: &impl Layout, point: Point) -> Option<PositionAtCoords> {
    let hit = layout.pos_at_coords(point)?;
    let resolved = doc.resolve(hit.pos).ok()?;
    // Gutter drags reorder top-level blocks, including nested lists and tables.
    let pos = if resolved.depth > 0 {
        let start = resolved.before(1);
        match layout.block_bounds(start).zip(doc.node_at(start)) {
            Some((bounds, block)) if point.y >= bounds.center().y => start + block.node_size(),
            _ => start,
        }
    } else {
        hit.pos
    };
    Some(PositionAtCoords { pos, inside: None })
}

fn distance_squared(a: Point, b: Point) -> f64 {
    (a.x - b.x).powi(2) + (a.y - b.y).powi(2)
}

fn editor_modifiers(modifiers: ModifiersState) -> Modifiers {
    Modifiers { alt: modifiers.alt_key(), control: modifiers.control_key(), meta: modifiers.super_key(), shift: modifiers.shift_key() }
}

fn pointer_button(button: MouseButton) -> Option<PointerButton> {
    match button {
        MouseButton::Left => Some(PointerButton::Primary),
        MouseButton::Right => Some(PointerButton::Secondary),
        MouseButton::Middle => Some(PointerButton::Middle),
        _ => None,
    }
}

fn window_cursor(cursor: CursorIcon) -> WindowCursor {
    match cursor {
        CursorIcon::Default => WindowCursor::Default,
        CursorIcon::Text => WindowCursor::Text,
        CursorIcon::Pointer => WindowCursor::Pointer,
        CursorIcon::Grab => WindowCursor::Grab,
        CursorIcon::Grabbing => WindowCursor::Grabbing,
        CursorIcon::Move => WindowCursor::Move,
        CursorIcon::ColumnResize => WindowCursor::ColResize,
        CursorIcon::RowResize => WindowCursor::RowResize,
        CursorIcon::NotAllowed => WindowCursor::NotAllowed,
    }
}

fn preedit_selection(text: &str, caret: Option<(usize, usize)>) -> Range<usize> {
    let byte_offset = |index| {
        let mut index = text.len().min(index);
        while !text.is_char_boundary(index) {
            index -= 1;
        }
        text[..index].encode_utf16().count()
    };
    let (from, to) = caret.unwrap_or((text.len(), text.len()));
    byte_offset(from.min(to))..byte_offset(from.max(to))
}

struct DesktopHost {
    surface: SurfaceHost,
    clipboard: SystemClipboard,
    ghost_at: Option<Point>,
    drag_hit: Option<PositionAtCoords>,
    touch_carets: [Option<Rect>; 2],
}

impl DesktopHost {
    fn touch_handle(&self, at: Point, radius: f64) -> Option<Point> {
        self.touch_carets.iter().flatten().filter_map(|rect| {
            let handle = Point { x: rect.left, y: rect.bottom + 3.0 };
            let distance = distance_squared(at, handle);
            (distance <= radius * radius).then_some((distance, rect.center()))
        }).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, point)| point)
    }
}

impl Layout for DesktopHost {
    fn coords_at_pos(&self, pos: usize, side: i32) -> Option<Rect> { self.surface.coords_at_pos(pos, side) }
    fn pos_at_coords(&self, point: Point) -> Option<PositionAtCoords> { self.drag_hit.or_else(|| self.surface.pos_at_coords(point)) }
    fn line_boxes(&self, from: usize, to: usize) -> Vec<LineBox> { self.surface.line_boxes(from, to) }
    fn block_bounds(&self, pos: usize) -> Option<Rect> { self.surface.block_bounds(pos) }
    fn composition_caret(&self, composition: &CompositionDisplay) -> Option<Rect> { self.surface.composition_caret(composition) }
    fn selection_rects(&self, from: usize, to: usize) -> Vec<Rect> { self.surface.selection_rects(from, to) }
}

impl Host for DesktopHost {
    fn update_layout(&mut self, frame: &DisplayList, changed: &BlockUpdate) {
        self.surface.update_layout(frame, changed);
    }

    fn draw(&mut self, frame: &DisplayList, changed: &BlockUpdate) {
        let ghost = self.ghost_at.zip(frame.drag.as_ref()).and_then(|(at, drag)| {
            let block = frame.node_at(drag.source)?;
            let label = block.node.text_content().chars().take(38).collect::<String>();
            let label = if label.is_empty() { block.node.type_name().into() } else { label };
            Some((Point { x: at.x + 12.0, y: at.y + 14.0 }, label))
        });
        self.surface.set_drag_feedback(None, ghost);
        self.touch_carets = if frame.touch_handles {
            let anchor = self.coords_at_pos(frame.selection.anchor, 1);
            let head = (frame.selection.anchor != frame.selection.head)
                .then(|| self.coords_at_pos(frame.selection.head, -1)).flatten();
            [anchor, head]
        } else {
            [None, None]
        };
        self.surface.draw(frame, changed);
    }

    fn read_clipboard(&mut self) -> Option<ClipboardContent> {
        self.clipboard.read()
    }

    fn write_clipboard(&mut self, content: &ClipboardContent) {
        self.clipboard.write(content);
    }

    fn set_ime_candidate_rect(&mut self, rect: Option<Rect>) { self.surface.set_ime_candidate_rect(rect); }
    fn set_cursor(&mut self, cursor: CursorIcon) { self.surface.set_cursor(cursor); }
    fn request_focus(&mut self) { self.surface.request_focus(); }
    fn viewport(&self) -> Option<Rect> { self.surface.viewport() }
    fn scroll_by(&mut self, delta: Point) { self.surface.scroll_by(delta); }
    fn update_accessibility(&mut self, update: &AccessibilityTreeUpdate) { self.surface.update_accessibility(update); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_preedit_byte_offsets_map_to_editor_utf16_positions() {
        let text = "a😀한";
        assert_eq!(preedit_selection(text, Some((1, 5))), 1..3);
        assert_eq!(preedit_selection(text, Some((9, 2))), 1..4);
        assert_eq!(preedit_selection(text, None), 4..4);
    }

    #[test]
    fn click_count_resets_after_time_button_or_location_changes() {
        let now = Instant::now();
        let primary = PointerButton::Primary;
        let last = Some(Click { button: primary, at: Point::default(), time: now, count: 2 });
        assert_eq!(click_count(last, primary, Point { x: 3.0, y: 2.0 }, now + Duration::from_millis(30), 1.0), 3);
        assert_eq!(click_count(last, primary, Point::default(), now + CLICK_INTERVAL, 1.0), 1);
        assert_eq!(click_count(last, PointerButton::Middle, Point::default(), now, 1.0), 1);
        assert_eq!(click_count(last, primary, Point { x: 40.0, y: 0.0 }, now, 1.0), 1);
    }

    #[test]
    fn gutter_drops_target_the_top_level_table_boundary() {
        struct TableLayout(usize);

        impl Layout for TableLayout {
            fn coords_at_pos(&self, _: usize, _: i32) -> Option<Rect> { None }
            fn pos_at_coords(&self, _: Point) -> Option<PositionAtCoords> {
                Some(PositionAtCoords { pos: self.0 + 4, inside: Some(self.0 + 3) })
            }
            fn block_bounds(&self, pos: usize) -> Option<Rect> {
                (pos == self.0).then_some(Rect { left: 24.0, right: 500.0, top: 100.0, bottom: 160.0 })
            }
        }

        let doc = setup::sample();
        let mut table = None;
        doc.for_each(|node, pos, _| {
            if node.type_name() == "table" { table = Some((pos, node.node_size())); }
        });
        let (start, size) = table.unwrap();
        let layout = TableLayout(start);
        let before = top_level_drop_hit(&doc, &layout, Point { x: 80.0, y: 110.0 }).unwrap();
        let after = top_level_drop_hit(&doc, &layout, Point { x: 80.0, y: 150.0 }).unwrap();
        assert_eq!(before, PositionAtCoords { pos: start, inside: None });
        assert_eq!(after, PositionAtCoords { pos: start + size, inside: None });
    }
}
