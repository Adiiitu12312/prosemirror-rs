//! The browser implementation of prosemirror-view's EditorView.

mod capturekeys;
mod clipboard;
mod domchange;
mod domcoords;
mod domobserver;
mod input;
mod selection;
mod viewdesc;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use js_sys::Array;
use prosemirror_model::{DomParser, DomSerializer, ResolvedPos, Slice};
use prosemirror_state::{EditorState, PluginKey, Selection, Transaction};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{
    ClipboardEvent, Document, DragEvent, Element, Event, EventTarget, HtmlElement, KeyboardEvent,
    MouseEvent, MutationObserver, MutationObserverInit, MutationRecord, Node,
};

use crate::{DecorationGroup, DecorationSet};

pub use clipboard::BrowserClipboardData;
pub use domcoords::{PositionAtCoords, ScrollSides, TextblockDirection, ViewportPoint, ViewportRect};
use domcoords::ScrollOptions;
pub use selection::SelectionOrigin;
use selection::WriteSelectionOptions;
pub use viewdesc::{
    DocView, GetPos, MarkView, MarkViewFactory, NodeView, NodeViewFactory, ViewMutation, WidgetView,
    WidgetViewFactory,
};

/// Standard ProseMirror browser styles.
pub const CSS: &str = include_str!("../style/prosemirror.css");

/// Where to place the editable element.
pub enum Mount {
    Append(Element),
    Existing(HtmlElement),
    Detached,
    Place(Rc<dyn Fn(&HtmlElement)>),
}

pub type ViewEventHandler<E> = Rc<dyn Fn(&BrowserView, &E) -> bool>;
pub type DispatchTransaction = Rc<dyn Fn(&BrowserView, Transaction)>;
pub type DecorationProvider = Rc<dyn Fn(&EditorState) -> Option<DecorationSet>>;
pub type DecorationGroupProvider = Rc<dyn Fn(&EditorState) -> DecorationGroup>;
pub type Editable = Rc<dyn Fn(&EditorState) -> bool>;
pub type Attributes = HashMap<String, String>;
pub type AttributeProvider = Rc<dyn Fn(&EditorState) -> Attributes>;
pub type PluginViewFactory = Rc<dyn Fn(&BrowserView) -> Box<dyn PluginView>>;
pub type TextInputHandler = Rc<dyn Fn(&BrowserView, usize, usize, &str, &dyn Fn() -> Transaction) -> bool>;
pub type ClickHandler = Rc<dyn Fn(&BrowserView, usize, &MouseEvent) -> bool>;
pub type ClickOnHandler =
    Rc<dyn Fn(&BrowserView, usize, &prosemirror_model::Node, usize, &MouseEvent, bool) -> bool>;
pub type PasteHandler = Rc<dyn Fn(&BrowserView, Option<&ClipboardEvent>, &Slice) -> bool>;
pub type DropHandler = Rc<dyn Fn(&BrowserView, &DragEvent, &Slice, bool) -> bool>;
pub type ScrollHandler = Rc<dyn Fn(&BrowserView) -> bool>;
pub type DragCopies = Rc<dyn Fn(&DragEvent) -> bool>;
pub type CreateSelection = Rc<dyn Fn(&BrowserView, &ResolvedPos, &ResolvedPos) -> Option<Selection>>;
pub type TransformHtml = Rc<dyn Fn(&str, &BrowserView) -> String>;
pub type TransformText = Rc<dyn Fn(&str, bool, &BrowserView) -> String>;
pub type TextParser = Rc<dyn Fn(&str, &ResolvedPos, bool, &BrowserView) -> Option<Slice>>;
pub type TextSerializer = Rc<dyn Fn(&Slice, &BrowserView) -> String>;
pub type TransformPasted = Rc<dyn Fn(Slice, &BrowserView, bool) -> Slice>;
pub type TransformCopied = Rc<dyn Fn(Slice, &BrowserView) -> Slice>;

pub trait PluginView {
    fn update(&mut self, _view: &BrowserView, _previous_state: &EditorState) {}
    fn destroy(&mut self) {}
}

#[derive(Clone, Default)]
pub struct BrowserViewProps {
    pub dispatch_transaction: Option<DispatchTransaction>,
    pub editable: Option<Editable>,
    pub attributes: Vec<AttributeProvider>,
    pub decorations: Vec<DecorationProvider>,
    pub decoration_groups: Vec<DecorationGroupProvider>,
    pub plugin_views: Vec<PluginViewFactory>,
    pub plugins: Vec<Rc<BrowserViewProps>>,
    pub state_plugins: HashMap<PluginKey, Rc<BrowserViewProps>>,
    pub node_views: HashMap<String, NodeViewFactory>,
    pub mark_views: HashMap<String, MarkViewFactory>,
    pub widgets: HashMap<u64, WidgetViewFactory>,
    pub handle_dom_events: HashMap<String, ViewEventHandler<Event>>,
    pub handle_key_down: Option<ViewEventHandler<KeyboardEvent>>,
    pub handle_key_press: Option<ViewEventHandler<KeyboardEvent>>,
    pub handle_text_input: Option<TextInputHandler>,
    pub handle_click_on: Option<ClickOnHandler>,
    pub handle_click: Option<ClickHandler>,
    pub handle_double_click_on: Option<ClickOnHandler>,
    pub handle_double_click: Option<ClickHandler>,
    pub handle_triple_click_on: Option<ClickOnHandler>,
    pub handle_triple_click: Option<ClickHandler>,
    pub handle_paste: Option<PasteHandler>,
    pub handle_drop: Option<DropHandler>,
    pub handle_scroll_to_selection: Option<ScrollHandler>,
    pub drag_copies: Option<DragCopies>,
    pub create_selection_between: Option<CreateSelection>,
    pub dom_parser: Option<Rc<DomParser>>,
    pub clipboard_parser: Option<Rc<DomParser>>,
    pub clipboard_serializer: Option<Rc<DomSerializer>>,
    pub clipboard_text_parser: Option<TextParser>,
    pub clipboard_text_serializer: Option<TextSerializer>,
    pub transform_pasted_html: Option<TransformHtml>,
    pub transform_pasted_text: Option<TransformText>,
    pub transform_pasted: Option<TransformPasted>,
    pub transform_copied: Option<TransformCopied>,
    pub scroll_threshold: Option<f64>,
    pub scroll_margin: Option<f64>,
    pub scroll_threshold_sides: Option<ScrollSides>,
    pub scroll_margin_sides: Option<ScrollSides>,
}

/// A snapshot of the current editor state and view props.
pub struct BrowserViewConfig {
    pub state: EditorState,
    pub props: BrowserViewProps,
}

impl Deref for BrowserViewConfig {
    type Target = BrowserViewProps;
    fn deref(&self) -> &Self::Target {
        &self.props
    }
}

impl DerefMut for BrowserViewConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.props
    }
}

impl BrowserViewConfig {
    /// Walk direct props, view plugins, and state plugins in upstream precedence.
    pub fn providers(&self) -> impl Iterator<Item = &BrowserViewProps> {
        self.props.providers(&self.state)
    }

    /// Return the first value produced by a property lookup.
    pub fn some_prop<'a, T>(&'a self, find: impl FnMut(&'a BrowserViewProps) -> Option<T>) -> Option<T> {
        self.providers().find_map(find)
    }

    fn editable(&self) -> bool {
        self.providers().all(|props| props.editable.as_ref().is_none_or(|edit| edit(&self.state)))
    }
}

impl BrowserViewProps {
    fn providers<'a>(&'a self, state: &'a EditorState) -> impl Iterator<Item = &'a Self> {
        std::iter::once(self).chain(self.plugins.iter().map(Rc::as_ref))
            .chain(state.plugins.iter().filter_map(|plugin| self.state_plugins.get(&plugin.key).map(Rc::as_ref)))
    }
}

#[derive(Clone)]
pub struct BrowserView(Rc<RefCell<BrowserViewInner>>);

struct BrowserViewInner {
    root: HtmlElement,
    state: EditorState,
    props: BrowserViewProps,
    doc_view: DocView,
    selected_node: Option<HtmlElement>,
    mounted: bool,
    destroyed: bool,
    applied_attributes: HashMap<String, Option<String>>,
    plugin_views: Vec<Box<dyn PluginView>>,
    listeners: Vec<Listener>,
    observer: Option<Observer>,
    input: input::InputState,
    dom_input: domchange::InputState,
}

impl BrowserView {
    pub fn new(place: Mount, state: EditorState, props: BrowserViewProps) -> Result<Self, JsValue> {
        let create_root = |document: Document| {
            document.create_element("div")?.dyn_into::<HtmlElement>().map_err(JsValue::from)
        };
        let mounted = matches!(place, Mount::Existing(_));
        let root = match place {
            Mount::Existing(root) => root,
            Mount::Append(parent) => {
                let root = create_root(parent.owner_document().ok_or_else(|| error("No owner document"))?)?;
                parent.append_child(&root)?;
                root
            }
            Mount::Detached => create_root(window_document()?)?,
            Mount::Place(place) => {
                let root = create_root(window_document()?)?;
                place(&root);
                root
            }
        };
        let view = Self(Rc::new(RefCell::new(BrowserViewInner {
            doc_view: DocView::new(root.clone()),
            root,
            state,
            props,
            selected_node: None,
            mounted,
            destroyed: false,
            applied_attributes: HashMap::new(),
            plugin_views: Vec::new(),
            listeners: Vec::new(),
            observer: None,
            input: input::InputState::default(),
            dom_input: domchange::InputState::default(),
        })));
        view.render(false)?;
        view.ensure_listeners()?;
        view.start_observer()?;
        view.create_plugin_views();
        Ok(view)
    }

    pub fn dom(&self) -> HtmlElement {
        self.0.borrow().root.clone()
    }

    pub fn state(&self) -> EditorState {
        self.0.borrow().state.clone()
    }

    pub fn props(&self) -> BrowserViewConfig {
        let inner = self.0.borrow();
        BrowserViewConfig { state: inner.state.clone(), props: inner.props.clone() }
    }

    pub fn same_view(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub fn editable(&self) -> bool {
        !self.is_destroyed() && self.props().editable()
    }

    pub fn composing(&self) -> bool {
        self.0.borrow().dom_input.composing()
    }

    pub fn update(&self, state: EditorState, props: BrowserViewProps) -> Result<(), JsValue> {
        if self.is_destroyed() {
            return Ok(());
        }
        let (reset_views, reset_scroll, scroll_to_selection) = {
            let inner = self.0.borrow();
            let plugins_changed = inner.state.plugins.iter().map(|plugin| &plugin.key)
                .ne(state.plugins.iter().map(|plugin| &plugin.key));
            let old_views = inner.props.providers(&inner.state).flat_map(|props| props.plugin_views.iter())
                .map(|factory| Rc::as_ptr(factory) as *const ());
            let new_views = props.providers(&state).flat_map(|props| props.plugin_views.iter())
                .map(|factory| Rc::as_ptr(factory) as *const ());
            let reset_views = plugins_changed || old_views.ne(new_views);
            (reset_views, plugins_changed, state.scroll_to_selection > inner.state.scroll_to_selection)
        };
        let root = self.dom();
        let scroll_position = (!reset_scroll && !scroll_to_selection)
            .then(|| domcoords::store_scroll_position(&root));
        if reset_views {
            self.destroy_plugin_views();
        }
        let previous = {
            let mut inner = self.0.borrow_mut();
            inner.props = props;
            std::mem::replace(&mut inner.state, state)
        };
        self.render(true)?;
        self.ensure_listeners()?;
        if reset_views {
            self.create_plugin_views();
        } else {
            let mut views = std::mem::take(&mut self.0.borrow_mut().plugin_views);
            for plugin in &mut views {
                plugin.update(self, &previous);
            }
            if self.is_destroyed() {
                for plugin in views.iter_mut().rev() {
                    plugin.destroy();
                }
            } else {
                self.0.borrow_mut().plugin_views = views;
            }
        }
        if reset_scroll {
            root.set_scroll_top(0);
        } else if scroll_to_selection {
            self.scroll_to_selection()?;
        } else if let Some(scroll_position) = scroll_position {
            domcoords::reset_scroll_position(scroll_position);
        }
        Ok(())
    }

    pub fn set_props(&self, update: impl FnOnce(&mut BrowserViewConfig)) -> Result<(), JsValue> {
        let mut config = self.props();
        update(&mut config);
        self.update(config.state, config.props)
    }

    pub fn update_state(&self, state: EditorState) -> Result<(), JsValue> {
        self.update(state, self.props().props)
    }

    pub fn dispatch(&self, transaction: Transaction) -> Result<bool, JsValue> {
        if self.is_destroyed() {
            return Ok(false);
        }
        let config = self.props();
        self.with_input(|input| input.cancel_composition_for(&transaction));
        if let Some(dispatch) = &config.props.dispatch_transaction {
            dispatch(self, transaction);
            return Ok(true);
        }
        if transaction.before() != &config.state.doc {
            return Err(error("Applying a mismatched transaction"));
        }
        let applied = config.state.apply_transaction(&transaction);
        if applied.transactions.is_empty() {
            return Ok(false);
        }
        self.update_state(applied.state)?;
        Ok(true)
    }

    pub fn root(&self) -> Node {
        let root = self.dom();
        let tree = root.get_root_node();
        if tree.node_type() == Node::DOCUMENT_NODE || tree.is_instance_of::<web_sys::ShadowRoot>() {
            tree
        } else {
            root.owner_document().map_or(tree, Into::into)
        }
    }

    /// Rebind document listeners when the mount moves to another document.
    pub fn update_root(&self) -> Result<(), JsValue> {
        if self.is_destroyed() {
            return Ok(());
        }
        let root = self.dom();
        let document = root.owner_document().ok_or_else(|| error("No owner document"))?;
        let listener = self.selection_listener(document)?;
        let root_target: EventTarget = root.into();
        self.0.borrow_mut().listeners.retain(|listener| {
            listener.name != "selectionchange" || listener.target == root_target
        });
        self.0.borrow_mut().listeners.push(listener);
        Ok(())
    }

    pub fn has_focus(&self) -> bool {
        !self.is_destroyed() && selection::has_focus(&self.dom())
    }

    pub fn focus(&self) -> Result<(), JsValue> {
        if self.is_destroyed() {
            return Ok(());
        }
        if self.editable() {
            domcoords::focus_prevent_scroll(&self.dom())?;
        }
        self.write_selection(true)
    }

    pub fn dom_at_pos(&self, pos: usize, side: i32) -> Option<(Node, u32)> {
        self.0.borrow().doc_view.dom_at_pos(pos, side)
    }

    pub fn pos_at_dom(&self, node: &Node, offset: u32, bias: i32) -> Option<usize> {
        self.0.borrow().doc_view.pos_at_dom(node, offset, bias)
    }

    pub fn node_dom(&self, pos: usize) -> Option<Node> {
        self.0.borrow().doc_view.node_dom(pos)
    }

    pub fn coords_at_pos(&self, pos: usize, side: i32) -> Option<ViewportRect> {
        let inner = self.0.borrow();
        domcoords::coords_at_pos(&inner.root, &inner.state.doc, &inner.doc_view, pos, side)
    }

    pub fn pos_at_coords(&self, point: ViewportPoint) -> Option<PositionAtCoords> {
        let inner = self.0.borrow();
        domcoords::pos_at_coords(&inner.root, &inner.state.doc, &inner.doc_view, point)
    }

    pub fn end_of_textblock(&self, direction: TextblockDirection, state: Option<&EditorState>) -> bool {
        let previous = self.state();
        let temporary = state.filter(|state| !state.doc.ptr_eq(&previous.doc) || state.selection != previous.selection);
        if let Some(state) = temporary {
            if self.update_state(state.clone()).is_err() {
                return false;
            }
        }
        let result = {
            let inner = self.0.borrow();
            domcoords::end_of_textblock(&inner.root, &inner.state, &inner.doc_view, direction)
        };
        if temporary.is_some() {
            report_error(self.update_state(previous));
        }
        result
    }

    pub fn scroll_to_selection(&self) -> Result<(), JsValue> {
        let config = self.props();
        if config.providers().any(|props| props.handle_scroll_to_selection.as_ref().is_some_and(|handler| handler(self))) {
            return Ok(());
        }
        let root = self.dom();
        let Some(selection) = selection::native_selection(&root) else { return Ok(()) };
        let Some(focus) = selection.focus_node().filter(|node| root.contains(Some(node))) else { return Ok(()) };
        let head = config.state.selection.head(&config.state.doc);
        let rect = if let Selection::Node { anchor } = config.state.selection {
            self.node_dom(anchor).and_then(|node| node.dyn_into::<Element>().ok())
                .map(|element| ViewportRect::from(element.get_bounding_client_rect()))
        } else {
            self.coords_at_pos(head, 1)
        };
        if let Some(rect) = rect {
            domcoords::scroll_rect_into_view(&root, rect, &focus, ScrollOptions {
                threshold: config.some_prop(|props| props.scroll_threshold_sides.or(props.scroll_threshold.map(Into::into)))
                    .unwrap_or_else(|| 0.0.into()),
                margin: config.some_prop(|props| props.scroll_margin_sides.or(props.scroll_margin.map(Into::into)))
                    .unwrap_or_else(|| 5.0.into()),
            });
        }
        Ok(())
    }

    pub fn dispatch_event(&self, event: &Event) -> Result<(), JsValue> {
        input::dispatch_event(self, event)
    }

    /// Process DOM records already delivered by the browser or queued on its observer.
    pub fn flush_dom_changes(&self) -> Result<(), JsValue> {
        let records = self.0.borrow().observer.as_ref().map_or_else(Array::new, |observer| observer.dom.take_records());
        self.observe(records, true)
    }

    fn flush_dom(&self) -> Result<(), JsValue> {
        self.flush_dom_changes()
    }

    fn queue_pending_dom_records(&self) -> bool {
        self.0.borrow().observer.as_ref().is_some_and(|observer| {
            let mut pending = observer.pending.borrow_mut();
            pending.extend(observer.dom.take_records().iter().filter_map(|record| record.dyn_into().ok()));
            !pending.is_empty()
        })
    }

    fn dom_selection(&self) -> Option<web_sys::Selection> {
        selection::native_selection(&self.dom())
    }

    fn sync_selection(&self) -> Result<(), JsValue> {
        self.write_selection(true)
    }

    fn with_doc_view<T>(&self, read: impl FnOnce(&DocView) -> T) -> Option<T> {
        let inner = self.0.borrow();
        (!inner.destroyed).then(|| read(&inner.doc_view))
    }

    fn with_input<T>(&self, update: impl FnOnce(&mut domchange::InputState) -> T) -> T {
        update(&mut self.0.borrow_mut().dom_input)
    }

    fn dispatch_key(&self, key: &str) -> Result<bool, JsValue> {
        input::dispatch_key(self, key)
    }

    fn handle_text_input(
        &self, from: usize, to: usize, text: &str, default: &dyn Fn() -> Transaction,
    ) -> Result<bool, JsValue> {
        let config = self.props();
        if config.providers().any(|props| props.handle_text_input.as_ref()
            .is_some_and(|handler| handler(self, from, to, text, default)))
        {
            return Ok(true);
        }
        config.state.handle_text_input(from, to, text, self.composing())
            .map_or(Ok(false), |transaction| self.dispatch(transaction))
    }

    pub fn destroy(&self) {
        if self.is_destroyed() {
            return;
        }
        self.destroy_plugin_views();
        self.0.borrow_mut().destroy();
    }

    pub fn is_destroyed(&self) -> bool {
        self.0.borrow().destroyed
    }

    fn observe(&self, records: Array, selection_changed: bool) -> Result<(), JsValue> {
        if self.is_destroyed() {
            return Ok(());
        }
        let mut pending = self.0.borrow().observer.as_ref().map_or_else(Vec::new, |observer| {
            std::mem::take(&mut *observer.pending.borrow_mut())
        });
        pending.extend(records.iter().filter_map(|record| record.dyn_into::<MutationRecord>().ok()));
        domobserver::observe_mutations(self, &pending, selection_changed)
    }

    fn read_selection(&self, origin: SelectionOrigin) -> Option<Selection> {
        let config = self.props();
        let between = RefCell::new(None);
        let create = |anchor: &ResolvedPos, head: &ResolvedPos| {
            *between.borrow_mut() = Some((anchor.clone(), head.clone()));
            None
        };
        let selection = {
            let inner = self.0.borrow();
            selection::read_selection(&inner.root, &config.state, &inner.doc_view, origin, Some(&create))
        };
        between.into_inner().and_then(|(anchor, head)| {
            config.some_prop(|props| props.create_selection_between.as_ref().and_then(|create| create(self, &anchor, &head)))
        }).or(selection)
    }

    fn write_selection(&self, force: bool) -> Result<(), JsValue> {
        let editable = self.editable();
        let mut inner = self.0.borrow_mut();
        if inner.dom_input.composing() && inner.dom_input.composition_node.is_some()
            && selection::has_selection(&inner.root, editable)
        {
            return Ok(());
        }
        let BrowserViewInner { root, doc_view, state, selected_node, .. } = &mut *inner;
        selection::sync_node_selection(doc_view, state, selected_node)?;
        selection::write_selection(root, state, doc_view, WriteSelectionOptions {
            editable, force, cursor_wrapper: None,
        })
    }

    fn render(&self, write_selection: bool) -> Result<(), JsValue> {
        let config = self.props();
        let editable = config.editable();
        let decorations = DecorationGroup {
            members: config.providers().flat_map(|props| {
                props.decorations.iter().filter_map(|get| get(&config.state))
                    .chain(props.decoration_groups.iter().flat_map(|get| get(&config.state).members))
            })
                .filter(|set| !set.is_empty()).collect(),
        };
        let mut rendering = BrowserViewProps::default();
        for props in config.providers() {
            for (name, factory) in &props.node_views {
                rendering.node_views.entry(name.clone()).or_insert_with(|| factory.clone());
            }
            for (name, factory) in &props.mark_views {
                rendering.mark_views.entry(name.clone()).or_insert_with(|| factory.clone());
            }
            for (id, factory) in &props.widgets {
                rendering.widgets.entry(*id).or_insert_with(|| factory.clone());
            }
        }
        {
            let mut inner = self.0.borrow_mut();
            if let Some(observer) = &inner.observer {
                observer.dom.take_records();
                observer.dom.disconnect();
            }
            let composition_node = inner.dom_input.composition_node.clone();
            let composition_range = inner.dom_input.composition_range;
            inner.doc_view.set_composition_node(composition_node);
            inner.doc_view.set_composition_range(composition_range);
            let result = inner.update_attributes(editable, &config)
                .and_then(|()| inner.doc_view.update(&config.state, &decorations, &rendering));
            if let Some(observer) = &inner.observer {
                observer.start(&inner.root)?;
            }
            result?;
        }
        if write_selection {
            self.write_selection(false)?;
            if let Some(observer) = &self.0.borrow().observer {
                observer.dom.take_records();
            }
        }
        Ok(())
    }

    fn create_plugin_views(&self) {
        let config = self.props();
        let views = config.providers().flat_map(|props| props.plugin_views.iter())
            .map(|factory| factory(self)).collect();
        self.0.borrow_mut().plugin_views = views;
    }

    fn destroy_plugin_views(&self) {
        let views = std::mem::take(&mut self.0.borrow_mut().plugin_views);
        for mut view in views.into_iter().rev() {
            view.destroy();
        }
    }

    fn ensure_listeners(&self) -> Result<(), JsValue> {
        let root = self.dom();
        let config = self.props();
        let events = input::EVENTS.iter().copied()
            .chain(config.providers().flat_map(|props| props.handle_dom_events.keys().map(String::as_str)));
        let target: EventTarget = root.into();
        let mut registered = self.0.borrow().listeners.iter().filter(|listener| listener.target == target)
            .map(|listener| listener.name.clone()).collect::<HashSet<_>>();
        for name in events {
            if !registered.insert(name.to_owned()) {
                continue;
            }
            let weak = Rc::downgrade(&self.0);
            let listener = Listener::new(target.clone(), name, move |event| {
                if let Some(inner) = weak.upgrade() {
                    report_error(input::handle_event(&BrowserView(inner), &event));
                }
            })?;
            self.0.borrow_mut().listeners.push(listener);
        }
        Ok(())
    }

    fn start_observer(&self) -> Result<(), JsValue> {
        let root = self.dom();
        let document = root.owner_document().ok_or_else(|| error("No owner document"))?;
        let weak = Rc::downgrade(&self.0);
        let callback = Closure::new(move |records: Array, _: MutationObserver| {
            if let Some(inner) = weak.upgrade() {
                report_error(BrowserView(inner).observe(records, true));
            }
        });
        let observer = Observer {
            dom: MutationObserver::new(callback.as_ref().unchecked_ref())?,
            pending: RefCell::default(),
            _callback: callback,
        };
        observer.start(&root)?;
        let selection_listener = self.selection_listener(document)?;
        let mut inner = self.0.borrow_mut();
        inner.listeners.push(selection_listener);
        inner.observer = Some(observer);
        Ok(())
    }

    fn selection_listener(&self, document: Document) -> Result<Listener, JsValue> {
        let weak = Rc::downgrade(&self.0);
        Listener::new(document.into(), "selectionchange", move |_| {
            if let Some(inner) = weak.upgrade() {
                report_error(BrowserView(inner).flush_dom_changes());
            }
        })
    }
}

impl BrowserViewInner {
    fn update_attributes(&mut self, editable: bool, config: &BrowserViewConfig) -> Result<(), JsValue> {
        let mut attrs = HashMap::from([
            ("class".to_owned(), "ProseMirror".to_owned()),
            ("contenteditable".to_owned(), editable.to_string()),
        ]);
        let plugin_attributes = config.state.plugins.iter().filter_map(|plugin| {
            plugin.call_prop("attributes")?.as_object().map(|attrs| {
                attrs.iter().filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_owned()))).collect()
            })
        });
        for provider_attrs in config.providers().flat_map(|props| props.attributes.iter())
            .map(|provider| provider(&config.state)).chain(plugin_attributes)
        {
            for (name, value) in provider_attrs {
                match name.as_str() {
                    "class" => {
                        let classes = attrs.entry(name).or_default();
                        classes.push(' ');
                        classes.push_str(&value);
                    }
                    "style" => {
                        let style = attrs.entry(name).or_default();
                        if !style.is_empty() {
                            style.push(';');
                        }
                        style.push_str(&value);
                    }
                    "contenteditable" | "nodeName" => continue,
                    _ => { attrs.entry(name).or_insert(value); }
                };
            }
        }
        attrs.entry("translate".into()).or_insert_with(|| "no".into());
        let old_names = self.applied_attributes.keys().cloned().collect::<Vec<_>>();
        for name in old_names.iter().filter(|name| !attrs.contains_key(*name)) {
            self.restore_attribute(name)?;
        }
        for (name, mut value) in attrs {
            let original = self.applied_attributes.entry(name.clone()).or_insert_with(|| self.root.get_attribute(&name));
            if let Some(original) = original {
                if name == "class" {
                    value = format!("{original} {value}");
                } else if name == "style" {
                    value = format!("{original};{value}");
                }
            }
            if name == "class" {
                for class in ["ProseMirror-focused", "ProseMirror-hideselection"] {
                    if self.root.class_list().contains(class) {
                        value.push(' ');
                        value.push_str(class);
                    }
                }
            }
            if self.root.get_attribute(&name).as_deref() != Some(&value) {
                self.root.set_attribute(&name, &value)?;
            }
        }
        Ok(())
    }

    fn restore_attribute(&mut self, name: &str) -> Result<(), JsValue> {
        if let Some(original) = self.applied_attributes.remove(name) {
            if let Some(original) = original {
                self.root.set_attribute(name, &original)?;
            } else {
                self.root.remove_attribute(name)?;
            }
        }
        Ok(())
    }

    fn destroy(&mut self) {
        if self.destroyed {
            return;
        }
        self.destroyed = true;
        self.observer.take();
        self.listeners.clear();
        for mut view in std::mem::take(&mut self.plugin_views).into_iter().rev() {
            view.destroy();
        }
        self.doc_view.destroy();
        if self.mounted {
            for name in self.applied_attributes.keys().cloned().collect::<Vec<_>>() {
                report_error(self.restore_attribute(&name));
            }
            self.root.set_text_content(None);
        } else {
            self.root.remove();
        }
    }
}

impl Drop for BrowserViewInner {
    fn drop(&mut self) {
        self.destroy();
    }
}

struct Listener {
    target: EventTarget,
    name: String,
    callback: Closure<dyn FnMut(Event)>,
}

impl Listener {
    fn new(target: EventTarget, name: &str, callback: impl FnMut(Event) + 'static) -> Result<Self, JsValue> {
        let callback = Closure::new(callback);
        target.add_event_listener_with_callback(name, callback.as_ref().unchecked_ref())?;
        Ok(Self { target, name: name.into(), callback })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self.target.remove_event_listener_with_callback(&self.name, self.callback.as_ref().unchecked_ref());
    }
}

struct Observer {
    dom: MutationObserver,
    pending: RefCell<Vec<MutationRecord>>,
    _callback: Closure<dyn FnMut(Array, MutationObserver)>,
}

impl Observer {
    fn start(&self, root: &HtmlElement) -> Result<(), JsValue> {
        let options = MutationObserverInit::new();
        options.set_child_list(true);
        options.set_character_data(true);
        options.set_character_data_old_value(true);
        options.set_attributes(true);
        options.set_attribute_old_value(true);
        options.set_subtree(true);
        self.dom.observe_with_options(root, &options)
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        self.dom.disconnect();
    }
}

fn window_document() -> Result<Document, JsValue> {
    web_sys::window().and_then(|window| window.document()).ok_or_else(|| error("No browser document"))
}

pub(super) fn error(message: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&message.to_string()).into()
}

fn report_error(result: Result<(), JsValue>) {
    if let Err(error) = result {
        web_sys::console::error_1(&error);
    }
}
