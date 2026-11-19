use std::fmt;
use std::rc::Rc;

use prosemirror_commands::{mac_base_keymap, pc_base_keymap};
use prosemirror_keymap::{KeyEvent, Keymap, Platform};
use prosemirror_model::{ModelError, Node, Slice};
use prosemirror_state::{EditorState, Selection, Transaction};
use prosemirror_transform::drop_point;
use serde_json::json;

use crate::{
    accessibility_tree, move_selection, paste_slice, serialize_clipboard, Attributes, BlockUpdate, ClipboardEvent,
    CompositionDisplay, CompositionEvent, Decoration, DecorationGroup, DecorationType, Direction, DisplayList, DragDisplay, DragEvent,
    EditorProps, Event, EventAction, Host, KeyInput, MakeNodeView, MakePluginView, Movement,
    NodeView, PluginView, Point, PointerButton, PointerEvent, PointerInput, TouchEvent, ViewMutation,
};

#[derive(Debug)]
pub enum EditorError {
    Model(ModelError),
    MismatchedTransaction,
}

impl fmt::Display for EditorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(error) => write!(f, "{error}"),
            Self::MismatchedTransaction => f.write_str("transaction belongs to a different document"),
        }
    }
}

impl std::error::Error for EditorError {}

impl From<ModelError> for EditorError {
    fn from(value: ModelError) -> Self { Self::Model(value) }
}

struct NodeViewEntry {
    node: Node,
    decorations: Vec<Decoration>,
    position: usize,
    factory: MakeNodeView,
    view: Box<dyn NodeView>,
    selected: bool,
}

struct PointerState {
    anchor: usize,
    origin: Point,
    moved: bool,
}

struct Dragging {
    source: Selection,
    slice: Slice,
    over: Option<usize>,
}

enum DispatchOutcome {
    Applied,
    Intercepted,
    Filtered,
}

impl DispatchOutcome {
    fn handled(self) -> bool {
        !matches!(self, Self::Filtered)
    }
}

pub struct Editor<H: Host> {
    state: EditorState,
    host: H,
    props: EditorProps,
    platform: Platform,
    keys: Keymap,
    decorations: DecorationGroup,
    attributes: Attributes,
    node_views: Vec<NodeViewEntry>,
    plugin_views: Vec<(MakePluginView, Box<dyn PluginView>)>,
    pointer: Option<PointerState>,
    drag: Option<Dragging>,
    composition: Option<CompositionDisplay>,
    composition_id: u64,
    goal_x: Option<f64>,
    focused: bool,
    touch_handles: bool,
}

impl<H: Host> Editor<H> {
    pub fn new(state: EditorState, host: H, props: EditorProps) -> Self {
        Self::with_platform(state, host, props, Platform::current())
    }

    pub fn with_platform(state: EditorState, host: H, props: EditorProps, platform: Platform) -> Self {
        let mut editor = Self {
            state, host, props, platform, keys: default_keys(platform),
            decorations: DecorationGroup::default(), attributes: Attributes::new(),
            node_views: Vec::new(), plugin_views: Vec::new(), pointer: None, drag: None,
            composition: None, composition_id: 0, goal_x: None, focused: false, touch_handles: false,
        };
        editor.refresh_props(None);
        editor.render(None);
        editor
    }

    pub fn state(&self) -> &EditorState { &self.state }
    pub fn host(&self) -> &H { &self.host }
    pub fn host_mut(&mut self) -> &mut H { &mut self.host }
    pub fn props(&self) -> &EditorProps { &self.props }
    pub fn decorations(&self) -> &DecorationGroup { &self.decorations }
    pub fn attributes(&self) -> &Attributes { &self.attributes }
    pub fn has_focus(&self) -> bool { self.focused }
    pub fn composing(&self) -> bool { self.composition.is_some() }
    pub fn drag_target(&self) -> Option<usize> { self.drag.as_ref()?.over }
    pub fn dragged_slice(&self) -> Option<&Slice> { self.drag.as_ref().map(|drag| &drag.slice) }

    pub fn editable(&self) -> bool {
        self.props.providers(&self.state).all(|props| props.editable.as_ref().is_none_or(|editable| editable(&self.state)))
    }

    pub fn set_props(&mut self, props: EditorProps) {
        self.props = props;
        let previous = self.state.clone();
        self.refresh_props(Some(&previous));
        self.render(None);
    }

    pub fn update_state(&mut self, state: EditorState) {
        let same_document = self.state.doc.ptr_eq(&state.doc);
        if !same_document {
            self.composition = None;
            self.drag = None;
        }
        self.replace_state(state);
    }

    /// Refresh geometry after a host resize without changing editor state.
    pub fn redraw(&mut self) { self.render(None); }

    pub fn dispatch(&mut self, transaction: Transaction) -> Result<bool, EditorError> {
        self.dispatch_transaction(transaction).map(DispatchOutcome::handled)
    }

    fn dispatch_transaction(&mut self, transaction: Transaction) -> Result<DispatchOutcome, EditorError> {
        if transaction.before() != &self.state.doc {
            return Err(EditorError::MismatchedTransaction);
        }
        if let Some(dispatch) = &self.props.dispatch_transaction {
            let Some(state) = dispatch(&self.state, transaction) else { return Ok(DispatchOutcome::Intercepted) };
            self.update_state(state);
            return Ok(DispatchOutcome::Applied);
        }
        let applied = self.state.apply_transaction(&transaction);
        if applied.transactions.is_empty() {
            return Ok(DispatchOutcome::Filtered);
        }
        for tr in &applied.transactions {
            self.map_interactions(tr);
        }
        self.replace_state(applied.state);
        Ok(DispatchOutcome::Applied)
    }

    pub fn handle(&mut self, event: Event) -> Result<bool, EditorError> {
        if self.event_stopped(&event) {
            return Ok(false);
        }
        match event {
            Event::KeyDown(event) => {
                self.touch_handles = false;
                self.key_down(&event)
            }
            Event::KeyUp(event) => {
                let action = self.props.providers(&self.state).filter_map(|props| props.handle_key_up.as_ref())
                    .map(|handle| handle(&self.state, &event)).find(handled);
                self.run_action(action)
            }
            Event::TextInput(text) => self.text_input(&text),
            Event::Move { movement, extend } => self.move_to(movement, extend),
            Event::Composition(event) => self.composition_event(event),
            Event::Pointer(event) => {
                self.touch_handles = false;
                self.pointer_event(event)
            }
            Event::Touch(event) => self.touch_event(event),
            Event::Focus => { self.focused = true; self.paint(); Ok(true) }
            Event::Blur => { self.focused = false; self.pointer = None; self.paint(); Ok(true) }
            Event::Scroll(delta) => { self.host.scroll_by(delta); self.paint(); Ok(true) }
            Event::Clipboard(event) => self.clipboard_event(event),
            Event::Drag(event) => self.drag_event(event),
        }
    }

    pub fn ignores_mutation(&self, pos: usize, mutation: ViewMutation) -> bool {
        self.node_views.iter().any(|entry| {
            entry.position <= pos && pos < entry.position + entry.node.node_size()
                && entry.view.ignore_mutation(mutation)
        })
    }

    fn replace_state(&mut self, state: EditorState) {
        let previous = std::mem::replace(&mut self.state, state);
        if self.state.doc != previous.doc {
            self.goal_x = None;
        }
        self.refresh_props(Some(&previous));
        self.render(Some(&previous.doc));
        if self.state.scroll_to_selection > previous.scroll_to_selection && self.scroll_to_selection() {
            self.paint();
        }
    }

    fn refresh_props(&mut self, previous: Option<&EditorState>) {
        self.attributes.clear();
        self.decorations.members.clear();
        for props in self.props.providers(&self.state) {
            if let Some(attributes) = &props.attributes {
                for (name, value) in attributes(&self.state) {
                    if matches!(name.as_str(), "contenteditable" | "nodeName") {
                        continue;
                    }
                    let separator = match name.as_str() { "class" => Some(' '), "style" => Some(';'), _ => None };
                    self.attributes.entry(name).and_modify(|current| {
                        if let Some(separator) = separator {
                            if !current.is_empty() { current.push(separator); }
                            current.push_str(&value);
                        } else if current.is_empty() {
                            current.clone_from(&value);
                        }
                    }).or_insert(value);
                }
            }
            if let Some(decorations) = &props.decorations {
                self.decorations.members.extend(decorations(&self.state).members);
            }
        }
        let factories = self.props.providers(&self.state).filter_map(|props| props.plugin_view.clone())
            .collect::<Vec<_>>();
        let changed = factories.len() != self.plugin_views.len()
            || factories.iter().zip(&self.plugin_views).any(|(a, (b, _))| !Rc::ptr_eq(a, b))
            || previous.is_some_and(|previous| previous.plugins.iter().map(|p| &p.key)
                .ne(self.state.plugins.iter().map(|p| &p.key)));
        if changed {
            for (_, view) in self.plugin_views.iter_mut().rev() { view.destroy(); }
            self.plugin_views = factories.into_iter().map(|make| {
                let view = make(&self.state);
                (make, view)
            }).collect();
        } else if let Some(previous) = previous {
            for (_, view) in &mut self.plugin_views { view.update(&self.state, previous); }
        }
        self.reconcile_node_views();
    }

    fn reconcile_node_views(&mut self) {
        let mut available = std::mem::take(&mut self.node_views);
        let selected = match self.state.selection { Selection::Node { anchor } => Some(anchor), _ => None };
        let doc = &self.state.doc;
        doc.nodes_between(0, doc.content().size(), |node, pos, _, _| {
            let factory = self.props.providers(&self.state)
                .find_map(|props| props.node_views.get(node.type_name())).cloned();
            let Some(factory) = factory else { return true };
            let end = pos + node.node_size();
            let decorations: Vec<_> = self.decorations.members.iter().flat_map(|set| set.find(Some(pos), Some(end)))
                .filter(|decoration| match decoration.kind {
                    DecorationType::Widget { .. } => pos < decoration.from && decoration.from < end,
                    DecorationType::Inline { .. } => decoration.from < end && pos < decoration.to,
                    DecorationType::Node { .. } => pos <= decoration.from && decoration.to <= end,
                }).collect();
            let lookup = |entry: &NodeViewEntry| Rc::ptr_eq(&entry.factory, &factory);
            let reused = available.iter().position(|entry| lookup(entry) && entry.node.ptr_eq(node))
                .or_else(|| available.iter().position(|entry| lookup(entry) && entry.position == pos))
                .map(|index| available.swap_remove(index));
            let mut entry = if let Some(mut entry) = reused {
                if entry.position != pos || !entry.node.ptr_eq(node) || entry.decorations != decorations {
                    if !entry.view.update(node, pos, &self.decorations) {
                        if entry.selected { entry.view.deselect(); }
                        entry.view.destroy();
                        entry.view = factory(node, pos, &self.decorations);
                        entry.selected = false;
                    }
                    entry.position = pos;
                    entry.node = node.clone();
                    entry.decorations = decorations;
                }
                entry
            } else {
                let view = factory(node, pos, &self.decorations);
                NodeViewEntry { node: node.clone(), decorations, position: pos, factory, view, selected: false }
            };
            if selected == Some(pos) && !entry.selected {
                entry.view.select();
                entry.selected = true;
            } else if selected != Some(pos) && entry.selected {
                entry.view.deselect();
                entry.selected = false;
            }
            self.node_views.push(entry);
            true
        });
        for mut stale in available {
            if stale.selected { stale.view.deselect(); }
            stale.view.destroy();
        }
    }

    fn render(&mut self, previous: Option<&Node>) {
        let changed = BlockUpdate::between(previous, &self.state.doc);
        let frame = self.frame();
        self.host.update_layout(&frame, &changed);
        self.draw_frame(frame, &changed);
        self.host.update_accessibility(&accessibility_tree(&self.state));
    }

    fn paint(&mut self) {
        let frame = self.frame();
        let changed = BlockUpdate::between(Some(&self.state.doc), &self.state.doc);
        self.draw_frame(frame, &changed);
    }

    fn frame(&self) -> DisplayList {
        let mut frame = DisplayList::unmeasured(&self.state.doc, &self.state.selection, &self.decorations);
        frame.attributes = self.attributes.clone();
        frame.editable = self.editable();
        frame.composition = self.composition.clone();
        frame.focused = self.focused;
        frame.touch_handles = self.touch_handles && self.focused;
        frame.drag = self.drag.as_ref().map(|drag| DragDisplay { source: drag.source.from(), over: drag.over });
        frame
    }

    fn draw_frame(&mut self, mut frame: DisplayList, changed: &BlockUpdate) {
        frame.measure(&self.host);
        let caret = self.focused.then(|| {
            let pos = self.state.selection.head(&self.state.doc);
            frame.composition_caret.or_else(|| self.host.coords_at_pos(pos, 1))
        }).flatten();
        self.host.set_ime_candidate_rect(caret);
        self.host.draw(&frame, changed);
    }

    fn scroll_to_selection(&mut self) -> bool {
        let head = self.state.selection.head(&self.state.doc);
        let Some(rect) = self.host.coords_at_pos(head, 1) else { return false };
        let before = self.host.viewport();
        self.host.scroll_into_view(rect);
        before != self.host.viewport()
    }

    fn run_action(&mut self, action: Option<EventAction>) -> Result<bool, EditorError> {
        match action {
            Some(EventAction::Dispatch(tr)) => self.dispatch(*tr),
            Some(EventAction::Prevent) => Ok(true),
            _ => Ok(false),
        }
    }

    fn key_down(&mut self, event: &KeyInput) -> Result<bool, EditorError> {
        let editable = self.editable();
        if editable {
            let action = self.props.providers(&self.state).filter_map(|props| props.handle_key_down.as_ref())
                .map(|handle| handle(&self.state, event)).find(handled);
            if action.is_some() { return self.run_action(action); }
        }
        if self.composing() {
            return if event.key == "Escape" { self.composition_event(CompositionEvent::Cancel) } else { Ok(false) };
        }
        let flags = event.modifiers;
        let mac = matches!(self.platform, Platform::MacOs | Platform::Ios);
        let modifier = if mac { flags.meta } else { flags.control };
        let by_word = if mac { flags.alt } else { flags.control };
        let direction = if matches!(event.key.as_str(), "ArrowLeft" | "ArrowUp" | "Home" | "PageUp") {
            Direction::Backward
        } else {
            Direction::Forward
        };
        use Movement::*;
        let movement = match event.key.as_str() {
            "ArrowLeft" | "ArrowRight" if mac && modifier => Some(LineBoundary(direction)),
            "ArrowLeft" | "ArrowRight" if by_word => Some(Word(direction)),
            "ArrowLeft" | "ArrowRight" => Some(VisualCharacter(direction)),
            "ArrowUp" | "ArrowDown" if mac && modifier => Some(Document(direction)),
            "ArrowUp" | "ArrowDown" if by_word => Some(Paragraph(direction)),
            "ArrowUp" | "ArrowDown" => Some(Vertical(direction)),
            "Home" | "End" if modifier => Some(Document(direction)),
            "Home" | "End" => Some(LineBoundary(direction)),
            "PageUp" | "PageDown" => Some(Paragraph(direction)),
            _ => None,
        };
        if let Some(movement) = movement { return self.move_to(movement, flags.shift); }
        if modifier && !flags.alt {
            match event.key.to_lowercase().as_str() {
                "c" => return self.clipboard_event(ClipboardEvent::Copy),
                "x" => return self.clipboard_event(ClipboardEvent::Cut),
                "v" => return self.clipboard_event(ClipboardEvent::Paste),
                "a" => return self.set_selection(Selection::All, false, false),
                _ => {}
            }
        }
        if !editable { return Ok(false); }
        let mut pending = None;
        let binding_handled = self.keys.handle_keydown(&self.state, &mut |tr| pending = Some(tr), &(), KeyEvent {
            key: &event.key, key_code: event.code, legacy_ie: false,
            modifiers: prosemirror_keymap::Modifiers {
                alt: flags.alt, ctrl: flags.control, meta: flags.meta, shift: flags.shift,
            },
        });
        if let Some(tr) = pending { self.dispatch(tr)?; }
        if binding_handled { return Ok(true); }
        if matches!(event.key.as_str(), "Backspace" | "Delete") {
            let direction = if event.key == "Backspace" { Direction::Backward } else { Direction::Forward };
            return self.delete_unit(if by_word { Word(direction) } else { Character(direction) });
        }
        Ok(false)
    }

    fn move_to(&mut self, movement: Movement, extend: bool) -> Result<bool, EditorError> {
        let Some(selection) = move_selection(&self.state, &self.host, movement, extend, &mut self.goal_x) else {
            return Ok(false);
        };
        self.set_selection(selection, true, false)
    }

    fn delete_unit(&mut self, movement: Movement) -> Result<bool, EditorError> {
        let Some(selection) = move_selection(&self.state, &self.host, movement, true, &mut None) else {
            return Ok(false);
        };
        if selection.empty_in(&self.state.doc) { return Ok(false); }
        let mut tr = self.state.tr();
        tr.set_selection(selection).delete_selection().scroll_into_view();
        self.dispatch(tr)
    }

    fn set_selection(&mut self, selection: Selection, scroll: bool, pointer: bool) -> Result<bool, EditorError> {
        if selection == self.state.selection { return Ok(false); }
        let mut tr = self.state.tr();
        tr.set_selection(selection);
        if scroll { tr.scroll_into_view(); }
        if pointer { tr.set_meta("pointer", json!(true)); }
        self.dispatch(tr)
    }

    fn text_input(&mut self, text: &str) -> Result<bool, EditorError> {
        if !self.editable() || self.composing() || text.is_empty() { return Ok(false); }
        let from = self.state.selection.from();
        let to = self.state.selection.to(&self.state.doc);
        let action = self.props.providers(&self.state).filter_map(|props| props.handle_text_input.as_ref())
            .map(|handle| handle(&self.state, from, to, text)).find(handled);
        if action.is_some() { return self.run_action(action); }
        let tr = self.state.handle_text_input(from, to, text, false).unwrap_or_else(|| {
            let mut tr = self.state.tr();
            tr.insert_text_at_selection(text).scroll_into_view();
            tr
        });
        self.dispatch(tr)
    }

    fn composition_event(&mut self, event: CompositionEvent) -> Result<bool, EditorError> {
        if !self.editable() { return Ok(false); }
        match event {
            CompositionEvent::Start => {
                self.composition_id = self.composition_id.wrapping_add(1);
                self.composition = Some(CompositionDisplay {
                    from: self.state.selection.from(), to: self.state.selection.to(&self.state.doc),
                    text: String::new(), selection: 0..0,
                });
            }
            CompositionEvent::Update { text, selection } => {
                let Some(composition) = &mut self.composition else { return Ok(false) };
                let length = text.encode_utf16().count();
                composition.selection = selection.start.min(length)..selection.end.max(selection.start).min(length);
                composition.text = text;
            }
            CompositionEvent::Commit(text) => {
                let Some(composition) = self.composition.take() else { return self.text_input(&text) };
                if text.is_empty() { self.render(Some(&self.state.doc.clone())); return Ok(true); }
                let mut tr = self.state.tr();
                tr.insert_text(&text, composition.from, composition.to).scroll_into_view()
                    .set_meta("composition", json!(self.composition_id));
                self.dispatch(tr)?;
                if let Some(mut tr) = self.state.handle_composition_end() {
                    tr.set_meta("composition", json!(self.composition_id));
                    self.dispatch(tr)?;
                }
                return Ok(true);
            }
            CompositionEvent::Cancel => { self.composition = None; }
        }
        self.render(Some(&self.state.doc.clone()));
        Ok(true)
    }

    fn map_interactions(&mut self, tr: &Transaction) {
        if let Some(drag) = &mut self.drag { drag.source = drag.source.map(tr.doc(), tr.mapping()); }
        let Some(composition) = &mut self.composition else { return };
        let mut changed_inside = false;
        let mut from = composition.from;
        let mut to = composition.to;
        for map in &tr.mapping().maps {
            map.for_each(|start, end, _, _| {
                changed_inside |= start < to && end > from;
            });
            from = map.map(from, -1);
            to = map.map(to, 1);
        }
        if changed_inside {
            self.composition = None;
        } else {
            composition.from = from;
            composition.to = to;
        }
    }

    fn pointer_event(&mut self, event: PointerEvent) -> Result<bool, EditorError> {
        match event {
            PointerEvent::Down(input) if input.button == PointerButton::Primary => {
                let Some(hit) = self.host.pos_at_coords(input.point) else { return Ok(false) };
                self.host.request_focus();
                self.focused = true;
                self.goal_x = None;
                let anchor = if input.modifiers.shift { self.state.selection.anchor() } else { hit.pos };
                self.pointer = Some(PointerState { anchor, origin: input.point, moved: false });
                if input.click_count < 2 {
                    self.select_between_positions(anchor, hit.pos)
                } else {
                    Ok(true)
                }
            }
            PointerEvent::Down(_) => Ok(false),
            PointerEvent::Move(point) => {
                let Some(pointer) = &mut self.pointer else { return Ok(false) };
                let Some(hit) = self.host.pos_at_coords(point) else { return Ok(false) };
                pointer.moved |= (pointer.origin.x - point.x).abs() + (pointer.origin.y - point.y).abs() > 1.0;
                self.select_between_positions(self.pointer.as_ref().unwrap().anchor, hit.pos)
            }
            PointerEvent::Up(input) => {
                if self.pointer.take().is_some_and(|pointer| pointer.moved) { return Ok(true); }
                self.click(input)
            }
            PointerEvent::Cancel => { self.pointer = None; Ok(true) }
        }
    }

    fn click(&mut self, input: PointerInput) -> Result<bool, EditorError> {
        let Some(hit) = self.host.pos_at_coords(input.point) else { return Ok(false) };
        let action = self.props.providers(&self.state).filter_map(|props| match input.click_count {
            0 | 1 => props.handle_click.as_ref(),
            2 => props.handle_double_click.as_ref(),
            _ => props.handle_triple_click.as_ref(),
        }).map(|handle| handle(&self.state, hit.pos, &input)).find(handled);
        if action.is_some() { return self.run_action(action); }
        let doc = &self.state.doc;
        let pos = doc.resolve(hit.pos)?;
        if input.click_count >= 3 && pos.parent().inline_content() {
            return self.set_selection(Selection::text(pos.start(pos.depth), pos.end(pos.depth)), false, true);
        }
        if input.click_count == 2 {
            return self.select_word(hit.pos);
        }
        if let Some(inside) = hit.inside {
            if doc.node_at(inside).is_some_and(|node| node.is_selectable() && (node.is_atom() || input.modifiers.control)) {
                return self.set_selection(Selection::node(inside), false, true);
            }
        }
        Ok(false)
    }

    fn select_between_positions(&mut self, anchor: usize, head: usize) -> Result<bool, EditorError> {
        let anchor = self.state.doc.resolve(anchor)?;
        let head = self.state.doc.resolve(head)?;
        let selection = Selection::between(&anchor, &head, 1);
        self.set_selection(selection, false, true)
    }

    fn select_word(&mut self, pos: usize) -> Result<bool, EditorError> {
        let mut word = self.state.clone();
        word.selection = Selection::cursor(pos);
        let left = move_selection(&word, &self.host, Movement::Word(Direction::Backward), false, &mut None)
            .map_or(pos, |selection| selection.from());
        let right = move_selection(&word, &self.host, Movement::Word(Direction::Forward), false, &mut None)
            .map_or(pos, |selection| selection.head(&word.doc));
        self.select_between_positions(left, right)
    }

    fn touch_event(&mut self, event: TouchEvent) -> Result<bool, EditorError> {
        let input = |point| PointerInput {
            point, button: PointerButton::Primary, click_count: 1, modifiers: Default::default(),
        };
        match event {
            TouchEvent::Tap(point) => {
                let input = input(point);
                let moved = self.pointer_event(PointerEvent::Down(input))?;
                Ok(self.pointer_event(PointerEvent::Up(input))? || moved)
            }
            TouchEvent::LongPress(point) => {
                let Some(hit) = self.host.pos_at_coords(point) else { return Ok(false) };
                self.host.request_focus();
                self.focused = true;
                self.touch_handles = true;
                self.select_word(hit.pos)
            }
            TouchEvent::DragStart(point) => {
                let Some(hit) = self.host.pos_at_coords(point) else { return Ok(false) };
                let selection = &self.state.selection;
                let a = selection.anchor();
                let b = selection.head(&self.state.doc);
                let anchor = if hit.pos.abs_diff(a) < hit.pos.abs_diff(b) { b } else { a };
                self.touch_handles = true;
                self.pointer = Some(PointerState { anchor, origin: point, moved: true });
                self.select_between_positions(anchor, hit.pos)
            }
            TouchEvent::Drag(point) => self.pointer_event(PointerEvent::Move(point)),
            TouchEvent::DragEnd(point) => {
                self.pointer_event(PointerEvent::Move(point))?;
                self.pointer_event(PointerEvent::Up(input(point)))
            }
            TouchEvent::Cancel => self.pointer_event(PointerEvent::Cancel),
        }
    }

    fn clipboard_event(&mut self, event: ClipboardEvent) -> Result<bool, EditorError> {
        match event {
            ClipboardEvent::Copy | ClipboardEvent::Cut => {
                let cut = matches!(event, ClipboardEvent::Cut);
                if self.state.selection.empty_in(&self.state.doc) || (cut && !self.editable()) {
                    return Ok(false);
                }
                let slice = self.state.selection.content(&self.state.doc)?;
                self.host.write_clipboard(&serialize_clipboard(&slice)?);
                if cut {
                    let mut tr = self.state.tr();
                    tr.delete_selection().scroll_into_view().set_meta("uiEvent", json!("cut"));
                    self.dispatch(tr)?;
                }
                Ok(true)
            }
            ClipboardEvent::Paste => {
                let Some(content) = self.host.read_clipboard() else { return Ok(false) };
                self.clipboard_event(ClipboardEvent::PasteContent { content, plain_text: false })
            }
            ClipboardEvent::PasteContent { content, plain_text } => {
                if !self.editable() { return Ok(false); }
                let context = self.state.doc.resolve(self.state.selection.from())?;
                let parsed = paste_slice(&self.state.schema, &content, plain_text, &context)?
                    .map(|slice| self.transform_paste(slice, plain_text));
                let empty = Slice::empty();
                let action = self.props.providers(&self.state).filter_map(|props| props.handle_paste.as_ref())
                    .map(|handle| handle(&self.state, parsed.as_ref().unwrap_or(&empty), plain_text)).find(handled);
                if action.is_some() { return self.run_action(action); }
                let Some(slice) = parsed else { return Ok(false) };
                let mut tr = self.state.tr();
                if let Some(node) = single_node(&slice) {
                    tr.replace_selection_with(node.clone(), plain_text);
                } else {
                    tr.replace_selection(slice);
                }
                tr.scroll_into_view().set_meta("paste", json!(true)).set_meta("uiEvent", json!("paste"));
                self.dispatch(tr)
            }
        }
    }

    fn transform_paste(&self, slice: Slice, plain: bool) -> Slice {
        self.props.providers(&self.state).filter_map(|props| props.transform_pasted.as_ref())
            .fold(slice, |slice, transform| transform(&self.state, slice, plain))
    }

    fn drag_event(&mut self, event: DragEvent) -> Result<bool, EditorError> {
        if !self.editable() { return Ok(false); }
        match event {
            DragEvent::Start => self.start_drag(self.state.selection.clone()),
            DragEvent::StartBlock { pos } => {
                if self.state.doc.node_at(pos).is_some_and(|node| node.is_block() && node.is_selectable()) {
                    self.start_drag(Selection::node(pos))
                } else {
                    Ok(false)
                }
            }
            DragEvent::Over(point) => {
                let Some(drag) = &mut self.drag else { return Ok(false) };
                drag.over = self.host.pos_at_coords(point).map(|hit| {
                    drop_point(&self.state.doc, hit.pos, &drag.slice).unwrap_or(hit.pos)
                });
                self.paint();
                Ok(true)
            }
            DragEvent::Drop { point, slice, copy } => self.drop_at(point, slice, copy),
            DragEvent::Cancel => { self.drag = None; self.paint(); Ok(true) }
        }
    }

    fn start_drag(&mut self, source: Selection) -> Result<bool, EditorError> {
        if source.empty_in(&self.state.doc) { return Ok(false); }
        let slice = source.content(&self.state.doc)?;
        self.drag = Some(Dragging { source, slice, over: None });
        self.paint();
        Ok(true)
    }

    fn drop_at(&mut self, point: Point, slice: Option<Slice>, copy: bool) -> Result<bool, EditorError> {
        let drag = self.drag.take();
        let was_dragging = drag.is_some();
        let Some(hit) = self.host.pos_at_coords(point) else { self.paint(); return Ok(false) };
        let moved = was_dragging && !copy && slice.is_none();
        let slice = slice.or_else(|| drag.as_ref().map(|drag| drag.slice.clone()))
            .map(|slice| self.transform_paste(slice, false));
        let empty = Slice::empty();
        let action = self.props.providers(&self.state).filter_map(|props| props.handle_drop.as_ref())
            .map(|handle| handle(&self.state, slice.as_ref().unwrap_or(&empty), hit.pos, moved)).find(handled);
        match action {
            Some(EventAction::Dispatch(tr)) => return self.dispatch_drop(*tr, was_dragging),
            Some(EventAction::Prevent) => {
                if was_dragging { self.paint(); }
                return Ok(true);
            }
            _ => {}
        }
        let Some(slice) = slice else { self.paint(); return Ok(false) };
        let insert = drop_point(&self.state.doc, hit.pos, &slice).unwrap_or(hit.pos);
        let mut tr = self.state.tr();
        if let Some(drag) = drag.filter(|_| moved) {
            if (drag.source.from()..=drag.source.to(&self.state.doc)).contains(&insert) {
                self.paint();
                return Ok(true);
            }
            tr.set_selection(drag.source).delete_selection();
        }
        let start = tr.mapping().map(insert, 1);
        let before = tr.doc().clone();
        let node = single_node(&slice).cloned();
        if let Some(node) = &node {
            tr.replace_range_with(start, start, node.clone());
        } else {
            tr.replace_range(start, start, slice);
        }
        if tr.doc() == &before { self.paint(); return Ok(false); }
        let selection = if node.as_ref().is_some_and(|node| {
            node.is_selectable() && tr.doc().node_at(start).is_some_and(|at| at.same_markup(node))
        }) {
            Selection::node(start)
        } else {
            let mut end = tr.mapping().map(insert, 1);
            if let Some(map) = tr.mapping().maps.last() { map.for_each(|_, _, _, to| end = to); }
            Selection::between(&tr.doc().resolve(start)?, &tr.doc().resolve(end)?, 1)
        };
        tr.set_selection(selection).set_meta("uiEvent", json!("drop"));
        self.dispatch_drop(tr, was_dragging)
    }

    fn dispatch_drop(&mut self, transaction: Transaction, was_dragging: bool) -> Result<bool, EditorError> {
        let outcome = self.dispatch_transaction(transaction);
        if was_dragging && !matches!(&outcome, Ok(DispatchOutcome::Applied)) {
            self.paint();
        }
        outcome.map(DispatchOutcome::handled)
    }

    fn event_stopped(&self, event: &Event) -> bool {
        let point = match event {
            Event::Pointer(PointerEvent::Down(input) | PointerEvent::Up(input)) => Some(input.point),
            Event::Pointer(PointerEvent::Move(point)) => Some(*point),
            _ => None,
        };
        let pos = point.and_then(|point| self.host.pos_at_coords(point)).map_or(self.state.selection.from(), |hit| hit.pos);
        self.node_views.iter().rev().any(|entry| {
            (entry.position..entry.position + entry.node.node_size()).contains(&pos)
                && entry.view.stop_event(event)
        })
    }
}

impl<H: Host> Drop for Editor<H> {
    fn drop(&mut self) {
        for (_, view) in self.plugin_views.iter_mut().rev() { view.destroy(); }
        for entry in self.node_views.iter_mut().rev() {
            if entry.selected { entry.view.deselect(); }
            entry.view.destroy();
        }
    }
}

fn handled(action: &EventAction) -> bool { !matches!(action, EventAction::Continue) }

fn single_node(slice: &Slice) -> Option<&Node> {
    (slice.open_start == 0 && slice.open_end == 0 && slice.content.child_count() == 1)
        .then(|| slice.content.child(0))
}

fn default_keys(platform: Platform) -> Keymap {
    use std::sync::Arc;
    use prosemirror_keymap::KeyCommand;
    let mac = matches!(platform, Platform::MacOs | Platform::Ios);
    let mut names = vec!["Enter", "Mod-Enter", "Backspace", "Mod-Backspace", "Shift-Backspace", "Delete", "Mod-Delete", "Mod-a"];
    if mac {
        names.extend(["Ctrl-h", "Alt-Backspace", "Ctrl-d", "Ctrl-Alt-Backspace", "Alt-Delete", "Alt-d", "Ctrl-a", "Ctrl-e"]);
    }
    prosemirror_keymap::keymap(names.into_iter().map(|name| {
        let run = if mac { mac_base_keymap(name) } else { pc_base_keymap(name) }.expect("registered base key");
        let run: KeyCommand = Arc::new(move |state, dispatch, _| run(state, Some(dispatch)));
        (name, run)
    }), platform).expect("valid default keymap")
}
