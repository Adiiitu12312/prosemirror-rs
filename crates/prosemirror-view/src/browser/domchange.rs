//! Reads browser edits using the DOM change algorithm from prosemirror-view 1.42.9.

use js_sys::Date;
use prosemirror_model::{
    DomAttribute, DomElement, DomNode, DomParseOptions, DomParseRule, DomParser, DomPosition,
    Fragment, Mark, Node, ResolvedPos, Whitespace,
};
use prosemirror_state::{EditorState, Selection, Transaction};
use serde_json::json;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Element, Node as WebNode};

use super::{selection::SelectionOrigin, viewdesc::DocView, BrowserView};

pub(crate) struct InputState {
    pub composing: bool,
    pub composition_node: Option<WebNode>,
    pub(crate) composition_range: Option<(usize, usize)>,
    pub(crate) composition_id: u64,
    pub(crate) composition_pending: u64,
    pub(crate) composition_ended_at: f64,
    pub(crate) last_key_code: Option<u32>,
    pub(crate) last_key_at: f64,
    pub(crate) last_changed_text: Option<WebNode>,
    pub(crate) last_chrome_delete: f64,
    pub(crate) suppress_selection_until: f64,
    pub(crate) last_selection: Option<(SelectionOrigin, f64)>,
    pub(crate) last_ios_enter: f64,
    pub(crate) dom_change_count: u64,
    pub(crate) bad_safari_composition: bool,
    composition_timer: u64,
    composition_finished: bool,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            composing: false,
            composition_node: None,
            composition_range: None,
            composition_id: 1,
            composition_pending: 0,
            composition_ended_at: -2e8,
            last_key_code: None,
            last_key_at: 0.0,
            last_changed_text: None,
            last_chrome_delete: -2e8,
            suppress_selection_until: 0.0,
            last_selection: None,
            last_ios_enter: -2e8,
            dom_change_count: 0,
            bad_safari_composition: false,
            composition_timer: 0,
            composition_finished: false,
        }
    }
}

impl InputState {
    pub fn composing(&self) -> bool {
        self.composing
    }

    pub fn record_key(&mut self, key: &str) {
        self.last_key_code = match key {
            "Backspace" => Some(8),
            "Delete" => Some(46),
            "Enter" => Some(13),
            _ => None,
        };
        self.last_key_at = Date::now();
        if key == "Enter" && BrowserFlags::get().ios {
            self.last_ios_enter = self.last_key_at;
        }
        self.record_selection(SelectionOrigin::Key);
    }

    pub fn record_selection(&mut self, origin: SelectionOrigin) {
        self.last_selection = Some((origin, Date::now()));
    }

    pub fn in_or_near_composition(&mut self) -> bool {
        if self.composing {
            return true;
        }
        if BrowserFlags::get().safari && (Date::now() - self.composition_ended_at).abs() < 500.0 {
            self.composition_ended_at = -2e8;
            return true;
        }
        false
    }

    /// Preserve the browser's composing text only until an external edit overlaps it.
    pub fn cancel_composition_for(&mut self, transaction: &Transaction) {
        let Some((mut from, mut to)) = self.composition_range else { return };
        let own = transaction.get_meta("composition").and_then(serde_json::Value::as_u64)
            == Some(self.composition_id);
        for map in &transaction.mapping().maps {
            let mut touches = false;
            map.for_each(|old_from, old_to, _, _| {
                touches |= old_from < to && old_to > from
                    || (old_from == old_to && old_from > from && old_from < to);
            });
            if touches && !own {
                self.composition_range = None;
                self.composition_node = None;
                return;
            }
            from = map.map(from, -1);
            to = map.map(to, 1);
        }
        self.composition_range = Some((from, to));
    }

    fn take_composition_id(&mut self) -> u64 {
        let pending = std::mem::take(&mut self.composition_pending);
        if pending != 0 { pending } else if self.composing { self.composition_id } else { 0 }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct BrowserFlags {
    pub chrome: bool,
    pub safari: bool,
    pub gecko: bool,
    pub android: bool,
    pub ios: bool,
}

impl BrowserFlags {
    pub fn get() -> Self {
        let Some(window) = web_sys::window() else { return Self::default() };
        let navigator = window.navigator();
        let agent = navigator.user_agent().unwrap_or_default();
        let safari = js_sys::Reflect::get(navigator.as_ref(), &JsValue::from_str("vendor"))
            .ok().and_then(|vendor| vendor.as_string())
            .is_some_and(|vendor| vendor.contains("Apple Computer"));
        Self {
            chrome: agent.contains("Chrome/"),
            gecko: agent.contains("Gecko/") && !agent.contains("WebKit"),
            android: agent.contains("Android "),
            ios: safari && (agent.contains("Mobile/") || navigator.max_touch_points() > 2),
            safari,
        }
    }
}

#[derive(Default)]
pub(crate) struct DomChange {
    pub range: Option<(usize, usize)>,
    pub type_over: bool,
    pub added: Vec<WebNode>,
}

struct ParsedChange {
    doc: Node,
    selection: Option<(usize, usize)>,
    from: usize,
    to: usize,
}

pub(crate) fn read_dom_change(view: &BrowserView, change: DomChange) -> Result<(), JsValue> {
    let browser = BrowserFlags::get();
    let config = view.props();
    let custom_parser = config.some_prop(|props| props.dom_parser.as_ref()).cloned();
    let state = config.state;
    let now = Date::now();
    let (composition, composing, backspace, ios_enter, last_chrome_delete, selection_origin) =
        view.with_input(|input| {
            let backspace = input.last_key_code == Some(8) && now - input.last_key_at < 100.0;
            let ios_enter = browser.ios && now - input.last_ios_enter < 225.0;
            let origin = input.last_selection.filter(|(_, time)| now - time < 50.0)
                .map(|(origin, _)| origin);
            (input.take_composition_id(), input.composing, backspace, ios_enter, input.last_chrome_delete, origin)
        });

    let Some((from, to)) = change.range else {
        if let Some(selection) = view.read_selection(selection_origin.unwrap_or_default())
            .filter(|selection| *selection != state.selection)
        {
            let mut tr = state.tr();
            tr.set_selection(selection);
            if selection_origin == Some(SelectionOrigin::Pointer) {
                tr.set_meta("pointer", json!(true));
            } else if selection_origin == Some(SelectionOrigin::Key) {
                tr.scroll_into_view();
            }
            set_composition_meta(&mut tr, composition);
            view.dispatch(tr)?;
        }
        return Ok(());
    };

    let before = state.doc.resolve(from).map_err(model_error)?;
    let shared = before.shared_depth(to);
    let from = before.before(shared + 1);
    let to = state.doc.resolve(to).map_err(model_error)?.after(shared + 1);
    let selection = view.dom_selection();
    let parsed = view.with_doc_view(|doc_view| {
        parse_between(doc_view, &state, (from, to), selection.as_ref(), custom_parser.as_deref(), browser, backspace)
    }).transpose()?.ok_or_else(|| JsValue::from_str("Editor DOM is unavailable"))?;
    let compare = state.doc.slice(parsed.from, parsed.to, false).map_err(model_error)?;
    let preferred = if backspace { state.selection.to(&state.doc) } else { state.selection.from() };
    let diff = find_diff(&compare.content, parsed.doc.content(), parsed.from, preferred, backspace);
    view.with_input(|input| {
        input.last_key_code = None;
        if diff.is_some() {
            input.dom_change_count += 1;
        }
    });
    if (ios_enter || browser.android)
        && change.added.iter().any(|node| node.node_type() == WebNode::ELEMENT_NODE && !is_inline_tag(&node.node_name()))
        && diff.is_none_or(|diff| diff.end_a >= diff.end_b)
        && view.dispatch_key("Enter")?
    {
        view.with_input(|input| input.last_ios_enter = 0.0);
        return Ok(());
    }
    let mut diff = match diff {
        Some(diff) => diff,
        None if change.type_over && !composing && text_selection_in_one_parent(&state)
            && parsed.selection.is_none_or(|(anchor, head)| anchor == head) =>
        {
            let to = state.selection.to(&state.doc);
            Diff { start: state.selection.from(), end_a: to, end_b: to }
        }
        None => {
            if let Some(selection) = resolve_selection(view, &state.doc, parsed.selection)
                .filter(|selection| *selection != state.selection)
            {
                let mut tr = state.tr();
                tr.set_selection(selection);
                set_composition_meta(&mut tr, composition);
                view.dispatch(tr)?;
            }
            return Ok(());
        }
    };
    expand_selection_deletion(&mut diff, &state, &parsed);

    let mut new_to = parsed.doc.resolve(diff.end_b.saturating_sub(parsed.from)).map_err(model_error)?;
    let new_from = parsed.doc.resolve(diff.start.saturating_sub(parsed.from)).map_err(model_error)?;
    let old_from = state.doc.resolve(diff.start).map_err(model_error)?;
    let inline = new_from.same_parent(&new_to) && new_from.parent().inline_content()
        && old_from.end(old_from.depth) >= diff.end_a;
    let ios_split = ios_enter && (!inline || change.added.iter().any(|node| matches!(node.node_name().as_str(), "DIV" | "P")));
    if (ios_split || (!inline && looks_like_enter(&parsed.doc, new_from.pos, new_to.pos)))
        && view.dispatch_key("Enter")?
    {
        view.with_input(|input| input.last_ios_enter = 0.0);
        return Ok(());
    }
    if state.selection.anchor() > diff.start
        && (if inline { diff.end_a > diff.start && diff.end_b == diff.start }
            else { looks_like_block_join(&state.doc, diff.start, diff.end_a, &new_from, &new_to) })
        && view.dispatch_key("Backspace")?
    {
        if browser.android && browser.chrome {
            view.with_input(|input| input.suppress_selection_until = Date::now() + 50.0);
        }
        return Ok(());
    }
    if browser.chrome && diff.end_b == diff.start {
        view.with_input(|input| input.last_chrome_delete = now);
    }
    if browser.android && !inline && new_from.start(new_from.depth) != new_to.start(new_to.depth)
        && new_to.parent_offset == 0 && new_from.depth == new_to.depth
        && parsed.selection == Some((diff.end_a, diff.end_a))
    {
        diff.end_b = diff.end_b.saturating_sub(2);
        new_to = parsed.doc.resolve(diff.end_b.saturating_sub(parsed.from)).map_err(model_error)?;
        let view = view.clone();
        schedule(20, move || { let _ = view.dispatch_key("Enter"); })?;
    }

    let setup = |mut tr: Transaction| {
        if let Some(selection) = resolve_selection(view, tr.doc(), parsed.selection) {
            let chrome_reports_old_cursor = browser.chrome && composing && selection.empty_in(tr.doc())
                && (diff.start != diff.end_b || last_chrome_delete < now - 100.0)
                && (selection.head(tr.doc()) == diff.start
                    || selection.head(tr.doc()) == tr.mapping().map(diff.end_a, 1).saturating_sub(1));
            if !chrome_reports_old_cursor {
                tr.set_selection(selection);
            }
        }
        set_composition_meta(&mut tr, composition);
        tr.scroll_into_view();
        tr
    };

    if inline {
        if new_from.pos == new_to.pos {
            let mut tr = state.tr();
            tr.delete(diff.start, diff.end_a);
            let mut tr = setup(tr);
            if let Some(marks) = state.doc.resolve(diff.start).ok()
                .zip(state.doc.resolve(diff.end_a).ok()).and_then(|(from, to)| from.marks_across(&to))
            {
                tr.ensure_marks(marks);
            }
            view.dispatch(tr)?;
            return Ok(());
        }
        if diff.end_a == diff.end_b {
            let current = new_from.parent().content().cut(new_from.parent_offset, new_to.parent_offset);
            let previous = old_from.parent().content().cut(
                old_from.parent_offset, diff.end_a - old_from.start(old_from.depth)
            );
            if let Some(mark) = mark_change(&current, &previous) {
                let mut tr = setup(state.tr());
                match mark {
                    MarkChange::Add(mark) => { tr.add_mark(diff.start, diff.end_a, mark); }
                    MarkChange::Remove(mark) => { tr.remove_mark(diff.start, diff.end_a, mark); }
                }
                view.dispatch(tr)?;
                return Ok(());
            }
        }
        let same_text = new_from.parent().maybe_child(new_from.index(new_from.depth)).is_some_and(Node::is_text)
            && Some(new_from.index(new_from.depth))
                == new_to.index(new_to.depth).checked_sub(usize::from(new_to.text_offset() == 0));
        if same_text {
            let text = new_from.parent().text_between(new_from.parent_offset, new_to.parent_offset);
            let default = || {
                let mut tr = state.tr();
                tr.insert_text(&text, diff.start, diff.end_a);
                setup(tr)
            };
            if !view.handle_text_input(diff.start, diff.end_a, &text, &default)? {
                view.dispatch(default())?;
            }
            return Ok(());
        }
    }
    let mut tr = state.tr();
    let slice = parsed.doc.slice(diff.start - parsed.from, diff.end_b - parsed.from, false).map_err(model_error)?;
    tr.replace(diff.start, diff.end_a, slice);
    view.dispatch(setup(tr))?;
    Ok(())
}

fn parse_between(
    doc_view: &DocView,
    state: &EditorState,
    (from, to): (usize, usize),
    selection: Option<&web_sys::Selection>,
    custom_parser: Option<&DomParser>,
    browser: BrowserFlags,
    backspace: bool,
) -> Result<ParsedChange, JsValue> {
    let range = doc_view.parse_range(from, to).ok_or_else(|| JsValue::from_str("Invalid DOM parse range"))?;
    let parent = range.dom_parent;
    let mut positions = selection.map(|selection| {
        let mut positions = Vec::new();
        if let Some(path) = selection.anchor_node().and_then(|node| dom_path(&parent, node)) {
            positions.push(DomPosition { path, offset: selection.anchor_offset() as usize, pos: None });
            if !selection.is_collapsed() {
                if let Some(path) = selection.focus_node().and_then(|node| dom_path(&parent, node)) {
                    positions.push(DomPosition { path, offset: selection.focus_offset() as usize, pos: None });
                }
            }
        }
        positions
    }).unwrap_or_default();
    let mut to_offset = range.to_offset;
    if browser.chrome && backspace {
        for offset in (range.from_offset..to_offset).rev() {
            let Some(node) = parent.child_nodes().item(offset as u32) else { continue };
            let desc = doc_view.nearest_desc(&node).filter(|desc| same_node(&node, &desc.dom));
            if node.node_name() == "BR" && desc.is_none() {
                to_offset = offset + 1;
                break;
            }
            if desc.is_none_or(|desc| desc.to != desc.from) {
                break;
            }
        }
    }
    let mut rules = custom_parser.map_or_else(
        || DomParser::schema_rules(&state.schema),
        |parser| parser.rules().to_vec(),
    );
    let mut adapter = DomAdapter { doc_view: Some(doc_view), rules: &mut rules, serial: 0, browser };
    let dom = adapter.convert(&parent, None, true);
    let start = state.doc.resolve(range.from).map_err(model_error)?;
    let top_match = start.parent().content_match_at(start.index(start.depth));
    let whitespace = if start.parent().node_type().is_some_and(|kind| kind.whitespace() == "pre") {
        Whitespace::Full
    } else {
        Whitespace::Preserve
    };
    let schema = custom_parser.map_or(&state.schema, |parser| &parser.schema);
    let parser = DomParser::new(schema.clone(), rules);
    let doc = parser.parse(&dom, DomParseOptions {
        from: Some(range.from_offset),
        to: Some(to_offset),
        top_node: Some(start.parent()),
        top_match: top_match.as_ref(),
        context: Some(&start),
        top_open: true,
        preserve_whitespace: Some(whitespace),
        find_positions: Some(&mut positions),
        ..Default::default()
    }).map_err(model_error)?;
    let selection = positions.first().and_then(|pos| pos.pos)
        .map(|anchor| (range.from + anchor, range.from + positions.get(1).and_then(|pos| pos.pos).unwrap_or(anchor)));
    Ok(ParsedChange { doc, selection, from: range.from, to: range.to })
}

pub(crate) fn browser_dom(root: &WebNode) -> DomNode {
    DomAdapter { doc_view: None, rules: &mut Vec::new(), serial: 0, browser: BrowserFlags::default() }
        .convert(root, None, true)
}

struct DomAdapter<'a> {
    doc_view: Option<&'a DocView>,
    rules: &'a mut Vec<DomParseRule>,
    serial: usize,
    browser: BrowserFlags,
}

impl DomAdapter<'_> {
    fn convert(&mut self, node: &WebNode, content_target: Option<(&WebNode, usize)>, container: bool) -> DomNode {
        match node.node_type() {
            WebNode::TEXT_NODE => DomNode::Text(node.node_value().unwrap_or_default()),
            WebNode::COMMENT_NODE => DomNode::Comment(node.node_value().unwrap_or_default()),
            WebNode::ELEMENT_NODE => {
                let element: &Element = node.unchecked_ref();
                let attributes = element.attributes();
                let mut portable = DomElement {
                    name: element.local_name(),
                    namespace: element.namespace_uri(),
                    attributes: (0..attributes.length()).filter_map(|index| attributes.item(index)).map(|attribute| {
                        DomAttribute { name: attribute.local_name(), value: attribute.value(), namespace: attribute.namespace_uri() }
                    }).collect(),
                    children: Vec::new(),
                };
                if let Some((_, id)) = content_target.filter(|(target, _)| same_node(target, node)) {
                    portable.attributes.push(DomAttribute::new("data-pm-content", id.to_string()));
                }
                let desc = self.doc_view.and_then(|doc_view| doc_view.nearest_desc(node))
                    .filter(|desc| same_node(node, &desc.dom));
                let mut active_content = None;
                if !container && self.doc_view.is_some() {
                    if let Some(desc) = desc.as_ref().filter(|desc| desc.model.is_some() || desc.mark.is_some()) {
                        if desc.model.as_ref().is_none_or(|model| !model.is_text()) {
                            let mut rule = self.rule(&mut portable);
                            if let Some(model) = &desc.model {
                                rule.node = Some(model.type_name().into());
                                rule.attrs = model.attrs().cloned();
                                if model.node_type().is_some_and(|kind| kind.whitespace() == "pre") {
                                    rule.preserve_whitespace = Some(Whitespace::Full);
                                }
                            } else if let Some(mark) = &desc.mark {
                                rule.mark = Some(mark.type_name.clone());
                                rule.attrs = mark.attrs.clone();
                            }
                            if let Some(content) = &desc.content_dom {
                                if !same_node(content, node) && node.contains(Some(content)) {
                                    rule.content_element = Some(format!("[data-pm-content=\"{}\"]", self.serial));
                                    active_content = Some((content, self.serial));
                                }
                            }
                            self.rules.insert(0, rule);
                        }
                    } else if (portable.name == "br" && (node.next_sibling().is_none()
                        || self.browser.safari && node.parent_node().is_some_and(|parent| matches!(parent.node_name().as_str(), "TR" | "TABLE"))))
                        || (portable.name == "img" && element.has_attribute("mark-placeholder"))
                        || desc.as_ref().is_some_and(|desc| desc.content_dom.is_none())
                    {
                        let mut rule = self.rule(&mut portable);
                        rule.ignore = true;
                        self.rules.insert(0, rule);
                    }
                }
                portable.children = self.children(node, active_content.or(content_target));
                DomNode::Element(portable)
            }
            _ => DomNode::Fragment(self.children(node, content_target)),
        }
    }

    fn children(&mut self, node: &WebNode, content_target: Option<(&WebNode, usize)>) -> Vec<DomNode> {
        let children = node.child_nodes();
        (0..children.length()).filter_map(|index| children.item(index))
            .map(|child| self.convert(&child, content_target, false)).collect()
    }

    fn rule(&mut self, portable: &mut DomElement) -> DomParseRule {
        self.serial += 1;
        portable.attributes.push(DomAttribute::new("data-pm-rule", self.serial.to_string()));
        DomParseRule::tag(format!("[data-pm-rule=\"{}\"]", self.serial))
    }
}

fn resolve_selection(view: &BrowserView, doc: &Node, selection: Option<(usize, usize)>) -> Option<Selection> {
    let (anchor, head) = selection?;
    let anchor = doc.resolve(anchor).ok()?;
    let head = doc.resolve(head).ok()?;
    Some(view.props().some_prop(|props| props.create_selection_between.as_ref()
        .and_then(|create| create(view, &anchor, &head)))
        .unwrap_or_else(|| Selection::between(&anchor, &head, 0)))
}

pub(crate) fn same_node(a: &WebNode, b: &WebNode) -> bool {
    a.is_same_node(Some(b))
}

fn dom_path(root: &WebNode, mut node: WebNode) -> Option<Vec<usize>> {
    let mut path = Vec::new();
    while !same_node(root, &node) {
        path.push(dom_index(&node) as usize);
        node = node.parent_node()?;
    }
    path.reverse();
    Some(path)
}

pub(crate) fn dom_index(node: &WebNode) -> u32 {
    let mut count = 0;
    let mut previous = node.previous_sibling();
    while let Some(node) = previous {
        count += 1;
        previous = node.previous_sibling();
    }
    count
}

fn set_composition_meta(tr: &mut Transaction, id: u64) {
    if id != 0 {
        tr.set_meta("composition", json!(id));
    }
}

fn text_selection_in_one_parent(state: &EditorState) -> bool {
    matches!(state.selection, Selection::Text { anchor, head } if anchor != head)
        && state.selection.range(&state.doc).is_ok_and(|range| range.from.same_parent(&range.to))
}

enum MarkChange {
    Add(Mark),
    Remove(Mark),
}

fn mark_change(current: &Fragment, previous: &Fragment) -> Option<MarkChange> {
    let current_marks = current.first_child()?.marks();
    let previous_marks = previous.first_child()?.marks();
    let added = current_marks.iter().filter(|mark| !previous_marks.contains(mark)).collect::<Vec<_>>();
    let removed = previous_marks.iter().filter(|mark| !current_marks.contains(mark)).collect::<Vec<_>>();
    let (mark, add) = match (added.as_slice(), removed.as_slice()) {
        ([mark], []) => (*mark, true),
        ([], [mark]) => (*mark, false),
        _ => return None,
    };
    let updated = Fragment::from_nodes(previous.children.iter().map(|node| {
        let marks = if add { Mark::add_to_set(node.marks(), mark.clone()) }
            else { node.marks().iter().filter(|other| *other != mark).cloned().collect() };
        node.with_marks(marks)
    }).collect());
    (updated == *current).then(|| if add { MarkChange::Add(mark.clone()) } else { MarkChange::Remove(mark.clone()) })
}

fn looks_like_block_join(old: &Node, start: usize, end: usize, new_from: &ResolvedPos, new_to: &ResolvedPos) -> bool {
    if end - start <= new_to.pos - new_from.pos || skip_boundaries(new_from, true, false) < new_to.pos {
        return false;
    }
    let Ok(start_pos) = old.resolve(start) else { return false };
    if !new_from.parent().inline_content() {
        return start_pos.node_after().is_some_and(|node| end == start + node.node_size());
    }
    if start_pos.parent_offset < start_pos.parent().content().size() || !start_pos.parent().is_textblock() {
        return false;
    }
    let Ok(next) = old.resolve(skip_boundaries(&start_pos, true, true)) else { return false };
    if !next.parent().is_textblock() || next.pos > end || skip_boundaries(&next, true, false) < end {
        return false;
    }
    new_from.parent().content().cut(new_from.parent_offset, new_from.parent().content().size())
        == *next.parent().content()
}

fn looks_like_enter(content: &Node, from: usize, to: usize) -> bool {
    let mut lines = 0;
    let mut leaves = 0;
    content.nodes_between(from, to, |node, pos, _, _| {
        if node.is_textblock() && pos >= from && pos < to {
            lines += 1;
        } else if node.text().is_some_and(|text| {
            prosemirror_model::slice_utf16(text, from.saturating_sub(pos), to.saturating_sub(pos))
                .chars().any(|ch| !ch.is_whitespace())
        }) || (!node.is_text() && node.is_leaf()) {
            leaves += 1;
        }
        true
    });
    leaves == 0 && lines == 1
}

fn skip_boundaries(pos: &ResolvedPos, mut from_end: bool, may_open: bool) -> usize {
    let mut depth = pos.depth;
    let mut end = if from_end { pos.end(pos.depth) } else { pos.pos };
    while depth > 0 && (from_end || pos.index_after(depth) == pos.node(depth).child_count()) {
        depth -= 1;
        end += 1;
        from_end = false;
    }
    if may_open {
        let mut next = pos.node(depth).maybe_child(pos.index_after(depth));
        while let Some(node) = next.filter(|node| !node.is_leaf()) {
            next = node.first_child();
            end += 1;
        }
    }
    end
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Diff {
    start: usize,
    end_a: usize,
    end_b: usize,
}

fn find_diff(a: &Fragment, b: &Fragment, pos: usize, preferred_pos: usize, prefer_end: bool) -> Option<Diff> {
    let mut start = a.find_diff_start_at(b, pos)?;
    let len_a = pos + a.size();
    let len_b = pos + b.size();
    let end = a.find_diff_end_at(b, len_a, len_b)?;
    let (mut end_a, mut end_b) = (end.a, end.b);
    if splits_surrogate(a, start - pos) || splits_surrogate(b, start - pos) {
        start -= 1;
    }
    if splits_surrogate(a, end_a - pos) || splits_surrogate(b, end_b - pos) {
        end_a = (end_a + 1).min(len_a);
        end_b = (end_b + 1).min(len_b);
    }
    let preferred_pos = if prefer_end {
        let adjust = start.saturating_sub(end_a.min(end_b));
        preferred_pos.checked_add(start)?.checked_sub(end_a + adjust)
    } else {
        Some(preferred_pos)
    };
    if end_a < start && len_a < len_b {
        if let Some(preferred) = preferred_pos.filter(|preferred| (end_a..=start).contains(preferred)) {
            start = preferred;
        }
        end_b = start + (end_b - end_a);
        end_a = start;
    } else if end_b < start {
        if let Some(preferred) = preferred_pos.filter(|preferred| (end_b..=start).contains(preferred)) {
            start = preferred;
        }
        end_a = start + (end_a - end_b);
        end_b = start;
    }
    if splits_surrogate(a, start - pos) || splits_surrogate(b, start - pos) {
        start -= 1;
        if splits_surrogate(a, end_a - pos) || splits_surrogate(b, end_b - pos) {
            end_a = (end_a + 1).min(len_a);
            end_b = (end_b + 1).min(len_b);
        }
    }
    Some(Diff { start, end_a, end_b })
}

fn splits_surrogate(fragment: &Fragment, pos: usize) -> bool {
    let (index, offset) = fragment.find_index(pos);
    let Some(node) = fragment.maybe_child(index) else { return false };
    if pos <= offset {
        return false;
    }
    let position = pos - offset;
    if let Some(units) = node.text_utf16() {
        return units.get(position - 1).zip(units.get(position)).is_some_and(|(&a, &b)| {
            (0xd800..=0xdbff).contains(&a) && (0xdc00..=0xdfff).contains(&b)
        });
    }
    splits_surrogate(node.content(), position - 1)
}

fn expand_selection_deletion(diff: &mut Diff, state: &EditorState, parsed: &ParsedChange) {
    let from = state.selection.from();
    let to = state.selection.to(&state.doc);
    if from >= to || diff.start != diff.end_b || !matches!(state.selection, Selection::Text { .. }) {
        return;
    }
    if diff.start > from && diff.start <= from + 2 && from >= parsed.from {
        diff.start = from;
    } else if diff.end_a < to && diff.end_a >= to.saturating_sub(2) && to <= parsed.to {
        diff.end_b += to - diff.end_a;
        diff.end_a = to;
    }
}

fn is_inline_tag(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(),
        "a" | "abbr" | "acronym" | "b" | "bdi" | "bdo" | "big" | "br" | "button" | "cite" |
        "code" | "data" | "datalist" | "del" | "dfn" | "em" | "i" | "img" | "ins" | "kbd" |
        "label" | "map" | "mark" | "meter" | "output" | "q" | "ruby" | "s" | "samp" | "small" |
        "span" | "strong" | "sub" | "sup" | "time" | "u" | "tt" | "var")
}

fn model_error(error: prosemirror_model::ModelError) -> JsValue {
    JsValue::from_str(&error.to_string())
}

fn schedule(delay: i32, callback: impl FnOnce() + 'static) -> Result<(), JsValue> {
    let callback = wasm_bindgen::closure::Closure::once_into_js(callback);
    web_sys::window().ok_or_else(|| JsValue::from_str("Missing browser window"))?
        .set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), delay)?;
    Ok(())
}

pub(crate) fn composition_start(view: &BrowserView) -> Result<(), JsValue> {
    if !view.composing() {
        view.flush_dom()?;
        let state = view.state();
        if !state.selection.empty_in(&state.doc) {
            if let Ok(range) = state.selection.range(&state.doc) {
                let shared = range.from.shared_depth(range.to.pos);
                if !range.from.node(shared).inline_content() {
                    let mut tr = state.tr();
                    tr.delete_selection();
                    view.dispatch(tr)?;
                }
            }
        } else if BrowserFlags::get().gecko {
            let selection = view.dom_selection();
            let resolved = state.doc.resolve(state.selection.to(&state.doc)).ok();
            if let Some((selection, _)) = selection.zip(resolved.filter(|pos| {
                pos.parent_offset > 0 && pos.text_offset() == 0
                    && pos.node_before().is_some_and(|node| !node.marks().is_empty())
            })) {
                if let Some(text) = selection.focus_node().and_then(|focus| {
                    text_node_near(view, focus, selection.focus_offset(), -1)
                }) {
                    selection.collapse_with_offset(Some(&text), web_size(&text))?;
                }
            }
        }
        view.with_input(|input| {
            input.composing = true;
            input.composition_finished = false;
        });
    }
    if BrowserFlags::get().android {
        schedule_composition_end(view, 5000)?;
    } else {
        view.with_input(|input| input.composition_timer = input.composition_timer.wrapping_add(1));
    }
    Ok(())
}

pub(crate) fn composition_update(view: &BrowserView) -> Result<(), JsValue> {
    composition_start(view)
}

pub(crate) fn composition_end(view: &BrowserView) -> Result<(), JsValue> {
    if !view.composing() {
        return Ok(());
    }
    let pending = view.queue_pending_dom_records();
    let bad_safari_composition = view.with_input(|input| {
        input.composing = false;
        input.composition_ended_at = Date::now();
        input.composition_pending = if pending { input.composition_id } else { 0 };
        input.composition_finished = true;
        input.composition_node = None;
        input.composition_range = None;
        input.composition_id = input.composition_id.wrapping_add(1).max(1);
        input.bad_safari_composition
    });
    if bad_safari_composition {
        view.flush_dom()?;
    } else if pending {
        let weak = std::rc::Rc::downgrade(&view.0);
        let callback = wasm_bindgen::closure::Closure::once_into_js(move || {
            if let Some(inner) = weak.upgrade() {
                super::report_error(BrowserView(inner).flush_dom());
            }
        });
        web_sys::window().ok_or_else(|| JsValue::from_str("Missing browser window"))?
            .queue_microtask(callback.unchecked_ref());
    }
    schedule_composition_end(view, 20)
}

pub(crate) fn end_composition(view: &BrowserView) -> Result<(), JsValue> {
    if view.is_destroyed() {
        return Ok(());
    }
    view.flush_dom()?;
    let (was_active, completed) = view.with_input(|input| {
        let ended = std::mem::take(&mut input.composition_finished);
        let was_active = input.composing || ended || input.composition_node.is_some();
        let completed = input.composing || ended;
        if input.composing {
            input.composition_ended_at = Date::now();
        }
        input.composing = false;
        input.composition_node = None;
        input.composition_range = None;
        input.composition_timer = input.composition_timer.wrapping_add(1);
        (was_active, completed)
    });
    let state = view.state();
    if was_active {
        view.update_state(state.clone())?;
    }
    if completed {
        if let Some(tr) = state.handle_composition_end() {
            view.dispatch(tr)?;
        }
    }
    Ok(())
}

fn schedule_composition_end(view: &BrowserView, delay: i32) -> Result<(), JsValue> {
    let id = view.with_input(|input| {
        input.composition_timer = input.composition_timer.wrapping_add(1);
        input.composition_timer
    });
    let weak = std::rc::Rc::downgrade(&view.0);
    schedule(delay, move || {
        if let Some(inner) = weak.upgrade() {
            let view = BrowserView(inner);
            if !view.is_destroyed() && view.with_input(|input| input.composition_timer == id) {
                let _ = end_composition(&view);
            }
        }
    })
}

/// Allows a host to synchronously finish the same composition path used by native events.
impl BrowserView {
    pub fn end_composition(&self) -> Result<(), JsValue> {
        end_composition(self)
    }

    /// The native text node currently pinned to protect an active composition.
    pub fn composition_node(&self) -> Option<WebNode> {
        self.with_input(|input| input.composition_node.clone())
    }
}

pub(crate) fn find_composition_node(view: &BrowserView) -> Option<WebNode> {
    let selection = view.dom_selection()?;
    let focus = selection.focus_node()?;
    if !view.dom().contains(Some(&focus)) {
        return None;
    }
    let before = text_node_near(view, focus.clone(), selection.focus_offset(), -1);
    let after = text_node_near(view, focus, selection.focus_offset(), 1);
    if let (Some(before), Some(after)) = (&before, &after) {
        if !same_node(before, after) {
            let (last, active) = view.with_input(|input| {
                (input.last_changed_text.clone(), input.composition_node.clone())
            });
            if let Some(last) = last.filter(|last| same_node(before, last) || same_node(after, last)) {
                return Some(last);
            }
            let changed = |node: &WebNode| {
                view.with_doc_view(|doc_view| doc_view.nearest_desc(node))
                    .flatten().filter(|desc| same_node(&desc.dom, node))
                    .and_then(|desc| desc.model).is_none_or(|model| model.text() != node.node_value().as_deref())
            };
            if changed(after) || active.is_some_and(|active| same_node(&active, after)) && !changed(before) {
                return Some(after.clone());
            }
        }
    }
    before.or(after)
}

fn text_node_near(view: &BrowserView, mut node: WebNode, mut offset: u32, dir: i32) -> Option<WebNode> {
    loop {
        let size = web_size(&node);
        if node.node_type() == WebNode::TEXT_NODE
            && (if dir < 0 { offset > 0 } else { offset < size })
        {
            return Some(node);
        }
        if node.node_type() == WebNode::ELEMENT_NODE
            && (if dir < 0 { offset > 0 } else { offset < size })
        {
            if node.dyn_ref::<Element>().is_some_and(|element| element.get_attribute("contenteditable").as_deref() == Some("false")) {
                return None;
            }
            node = node.child_nodes().item(if dir < 0 { offset - 1 } else { offset })?;
            offset = if dir < 0 { web_size(&node) } else { 0 };
            continue;
        }
        let block = view.with_doc_view(|doc_view| doc_view.nearest_desc(&node)).flatten()
            .filter(|desc| same_node(&desc.dom, &node))
            .and_then(|desc| desc.model)
            .is_some_and(|node| node.is_block());
        if block {
            return None;
        }
        offset = dom_index(&node) + u32::from(dir > 0);
        node = node.parent_node()?;
    }
}

pub(crate) fn web_size(node: &WebNode) -> u32 {
    if node.node_type() == WebNode::TEXT_NODE {
        node.unchecked_ref::<web_sys::Text>().length()
    } else {
        node.child_nodes().length()
    }
}
