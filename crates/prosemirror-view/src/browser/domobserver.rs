//! Mutation registration and selection observation from prosemirror-view 1.42.9.

use js_sys::Date;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Element, MutationRecord, Node, Selection as DomSelection};

use super::domchange::{self, dom_index, same_node, BrowserFlags, DomChange};
use super::{selection, BrowserView, DocView, ViewMutation};

/// Keep the native range last observed or written by this view.
#[derive(Default, PartialEq, Eq)]
pub(super) struct SelectionSnapshot {
    anchor: Option<Node>,
    anchor_offset: u32,
    focus: Option<Node>,
    focus_offset: u32,
}

impl SelectionSnapshot {
    pub(super) fn capture(selection: Option<&DomSelection>) -> Self {
        selection.map_or_else(Self::default, |selection| Self {
            anchor: selection.anchor_node(),
            anchor_offset: selection.anchor_offset(),
            focus: selection.focus_node(),
            focus_offset: selection.focus_offset(),
        })
    }
}

pub(super) fn has_focus_and_selection(view: &BrowserView, editable: bool) -> bool {
    (!editable || view.has_focus()) && selection::has_selection(&view.dom(), editable)
}

pub(crate) fn observe_mutations(
    view: &BrowserView,
    records: &[MutationRecord],
    selection_changed: bool,
) -> Result<(), JsValue> {
    let now = Date::now();
    let suppressed = view.with_input(|input| now < input.suppress_selection_until);
    let editable = view.editable();
    let owns_selection = selection_changed && has_focus_and_selection(view, editable);
    if suppressed && owns_selection {
        view.sync_selection()?;
    }
    let read_selection = owns_selection && !suppressed && !ignore_selection(view);
    let root = view.dom();
    let (change, last_text) = if editable {
        view.with_doc_view(|doc_view| {
            let mut change = DomChange::default();
            let mut last_text = None;
            for record in records {
                register_mutation(doc_view, root.as_ref(), record, &mut change, &mut last_text);
            }
            (change, last_text)
        }).unwrap_or_default()
    } else {
        (DomChange::default(), None)
    };
    if let Some(text) = last_text {
        view.with_input(|input| input.last_changed_text = Some(text));
    }
    let browser = BrowserFlags::get();
    if change.range.is_some() {
        clean_added_nodes(view, browser, records, &change.added);
        if view.composing() {
            let composing_node = domchange::find_composition_node(view);
            let range = composing_node.as_ref().and_then(|node| {
                view.with_doc_view(|doc_view| doc_view.nearest_desc(node)).flatten()
                    .filter(|desc| same_node(&desc.dom, node)).map(|desc| (desc.from, desc.to))
            }).or(change.range);
            view.with_input(|input| {
                input.composition_node = composing_node;
                input.composition_range = range;
            });
        }
    }
    if change.range.is_none() && !read_selection {
        return Ok(());
    }
    let before = view.state();
    let changed_dom = change.range.is_some();
    domchange::read_dom_change(view, change)?;
    if changed_dom && !view.is_destroyed() && view.state().doc.ptr_eq(&before.doc) {
        // A callback may have ignored the editor transaction, or the mutation
        // may have parsed to an equivalent document. Restore the live DOM.
        view.update_state(view.state())?;
    }
    Ok(())
}

fn register_mutation(
    doc_view: &DocView,
    root: &Node,
    record: &MutationRecord,
    batch: &mut DomChange,
    last_text: &mut Option<Node>,
) {
    let Some(target) = record.target() else { return };
    if batch.added.iter().any(|node| same_node(node, &target)) {
        return;
    }
    let Some(desc) = doc_view.nearest_desc(&target) else { return };
    let kind = record.type_();
    if kind == "attributes" {
        let attribute = record.attribute_name();
        if same_node(&desc.dom, root) || attribute.as_deref() == Some("contenteditable")
            || (attribute.as_deref() == Some("style")
                && record.old_value().is_none_or(|old| old.is_empty())
                && target.dyn_ref::<Element>()
                    .is_none_or(|element| element.get_attribute("style").is_none_or(|style| style.is_empty())))
        {
            return;
        }
    }
    if doc_view.ignore_mutation(&ViewMutation::Dom(record.clone())) {
        return;
    }

    let (from, to) = if kind == "childList" {
        let added = record.added_nodes();
        for node in (0..added.length()).filter_map(|index| added.item(index)) {
            if node.node_type() == Node::TEXT_NODE {
                *last_text = Some(node.clone());
            }
            batch.added.push(node);
        }
        if desc.content_dom.as_ref().is_some_and(|content| !same_node(content, &desc.dom) && !content.contains(Some(&target))) {
            (desc.from, desc.to)
        } else {
            let from_offset = record.previous_sibling().filter(|node| is_child_of(node, &target))
                .map_or(0, |node| dom_index(&node) + 1);
            let to_offset = record.next_sibling().filter(|node| is_child_of(node, &target))
                .map_or_else(|| target.child_nodes().length(), |node| dom_index(&node));
            let from = doc_view.pos_at_dom(&target, from_offset, -1).unwrap_or(desc.from);
            let to = doc_view.pos_at_dom(&target, to_offset, 1).unwrap_or(desc.to);
            (from, to)
        }
    } else {
        if kind == "characterData" {
            batch.type_over |= target.node_value() == record.old_value();
            *last_text = Some(target);
        }
        (desc.from, desc.to)
    };
    batch.range = Some(batch.range.map_or((from, to), |(old_from, old_to)| {
        (old_from.min(from), old_to.max(to))
    }));
}

fn is_child_of(node: &Node, parent: &Node) -> bool {
    node.parent_node().is_some_and(|actual| same_node(&actual, parent))
}

fn ignore_selection(view: &BrowserView) -> bool {
    let Some(selection) = view.dom_selection() else { return true };
    let (Some(anchor), Some(focus)) = (selection.anchor_node(), selection.focus_node()) else { return true };
    let mut chain = Vec::new();
    let mut parent = Some(focus);
    while let Some(node) = parent {
        parent = node.parent_node();
        chain.push(node);
    }
    let mut parent = Some(anchor);
    while let Some(node) = parent {
        if chain.iter().any(|focus| same_node(focus, &node)) {
            let target = if node.node_type() == Node::TEXT_NODE { node.parent_node().unwrap_or(node) } else { node };
            return view.with_doc_view(|doc_view| doc_view.ignore_mutation(&ViewMutation::Selection(target))).unwrap_or(true);
        }
        parent = node.parent_node();
    }
    true
}

fn clean_added_nodes(view: &BrowserView, browser: BrowserFlags, records: &[MutationRecord], added: &[Node]) {
    let now = Date::now();
    let (composing, ended, key) = view.with_input(|input| {
        (input.composing, input.composition_ended_at, input.last_key_code)
    });
    let extra_break = matches!(key, Some(8 | 46)) || (browser.chrome && (composing || ended > now - 50.0)
        && records.iter().any(|record| record.type_() == "childList" && record.removed_nodes().length() > 0));
    let breaks = added.iter().filter(|node| node.node_name() == "BR").collect::<Vec<_>>();
    if extra_break {
        for node in breaks {
            let mut after = node.next_sibling();
            while let Some(element) = after.as_ref().and_then(|node| node.dyn_ref::<Element>()) {
                if element.get_attribute("contenteditable").as_deref() == Some("false") {
                    remove(node);
                    break;
                }
                after = element.first_child();
            }
        }
    } else if browser.gecko {
        if let [first, second] = breaks.as_slice() {
            let nested = first.parent_node().and_then(|node| node.parent_node())
                .zip(second.parent_node()).is_some_and(|(a, b)| same_node(&a, &b));
            remove(if nested { second } else { first });
        } else {
            let focused = view.dom_selection().and_then(|selection| selection.focus_node());
            let block = focused.as_ref().and_then(|focus| block_parent(view, focus));
            for node in breaks {
                if let Some(parent) = node.parent_node().filter(|parent| parent.node_name() == "LI") {
                    if !block.as_ref().is_some_and(|block| same_node(&parent, block)) {
                        remove(node);
                    }
                }
            }
        }
    }
    if browser.safari && composing && added.iter().any(|node| node.parent_node().is_some_and(|parent| parent.node_name() == "TR")) {
        view.with_input(|input| input.bad_safari_composition = true);
    }
    if view.with_input(|input| std::mem::take(&mut input.bad_safari_composition)) {
        fix_safari_composition(view, added);
    }
}

fn remove(node: &Node) {
    if let Some(parent) = node.parent_node() {
        let _ = parent.remove_child(node);
    }
}

fn block_parent(view: &BrowserView, node: &Node) -> Option<Node> {
    let root = view.dom();
    view.with_doc_view(|doc_view| {
        let mut parent = node.parent_node();
        while let Some(node) = parent.filter(|node| !same_node(node, root.as_ref())) {
            if doc_view.nearest_desc(&node).is_some_and(|desc| {
                same_node(&desc.dom, &node) && desc.model.is_some_and(|model| model.is_block())
            }) {
                return Some(node);
            }
            parent = node.parent_node();
        }
        None
    }).flatten()
}

fn fix_safari_composition(view: &BrowserView, added: &[Node]) {
    let selection = view.dom_selection();
    for node in added {
        if !node.parent_node().is_some_and(|parent| parent.node_name() == "TR") {
            continue;
        }
        let mut cell = node.next_sibling();
        while cell.as_ref().is_some_and(|cell| !matches!(cell.node_name().as_str(), "TD" | "TH")) {
            cell = cell.and_then(|cell| cell.next_sibling());
        }
        if let Some(mut cell) = cell {
            loop {
                let child = cell.first_child();
                let element = child.as_ref().and_then(|child| child.dyn_ref::<Element>());
                let open = element.is_some_and(|element| {
                    element.get_attribute("contenteditable").as_deref() != Some("false")
                        && !matches!(element.node_name().as_str(), "BR" | "IMG")
                });
                if !open {
                    break;
                }
                cell = child.expect("open child");
            }
            let _ = cell.insert_before(node, cell.first_child().as_ref());
            if let Some(selection) = selection.as_ref().filter(|selection| {
                selection.focus_node().is_some_and(|focus| same_node(&focus, node))
            }) {
                let _ = selection.collapse_with_offset(Some(node), selection.focus_offset());
            }
        } else {
            remove(node);
        }
    }
}
