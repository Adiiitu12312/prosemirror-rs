//! The table node view and live colgroup updates from upstream tableview.ts.

use std::cell::RefCell;
use std::rc::Rc;

use prosemirror_model::Node;
use prosemirror_view::browser::{NodeView, ViewMutation};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, HtmlElement};

use crate::tableview::{update_columns_on_resize, ColumnLayout};

pub struct BrowserTableView {
    pub node: Node,
    pub default_cell_min_width: f64,
    pub dom: HtmlElement,
    pub table: HtmlElement,
    pub colgroup: HtmlElement,
    pub content_dom: HtmlElement,
}

impl BrowserTableView {
    pub fn new(document: &Document, node: Node, default_cell_min_width: f64) -> Result<Self, JsValue> {
        let dom = html_element(document, "div")?;
        dom.set_class_name("tableWrapper");
        let table = html_element(document, "table")?;
        dom.append_child(&table)?;
        table.style().set_property("--default-cell-min-width", &pixels(default_cell_min_width))?;
        let colgroup = html_element(document, "colgroup")?;
        table.append_child(&colgroup)?;
        update_dom_columns_on_resize(&node, &colgroup, &table, default_cell_min_width, None)?;
        let content_dom = html_element(document, "tbody")?;
        table.append_child(&content_dom)?;
        Ok(Self { node, default_cell_min_width, dom, table, colgroup, content_dom })
    }

    pub fn update(&mut self, node: &Node) -> Result<bool, JsValue> {
        if node.type_name() != self.node.type_name() {
            return Ok(false);
        }
        update_dom_columns_on_resize(node, &self.colgroup, &self.table, self.default_cell_min_width, None)?;
        self.node = node.clone();
        Ok(true)
    }

    pub fn into_node_view(self) -> NodeView {
        let mut view = NodeView::new(self.dom.clone()).with_content_dom(self.content_dom.clone());
        let table = self.table.clone();
        let colgroup = self.colgroup.clone();
        let table_view = RefCell::new(self);
        view.update = Some(Rc::new(move |node, _, _| table_view.borrow_mut().update(node).unwrap_or_else(|error| {
            web_sys::console::error_1(&error);
            false
        })));
        view.ignore_mutation = Some(Rc::new(move |mutation| {
            let ViewMutation::Dom(record) = mutation else { return false };
            record.type_() == "attributes" && record.target().is_some_and(|target| {
                table.is_same_node(Some(&target)) || colgroup.contains(Some(&target))
            })
        }));
        view
    }
}

/// Reuse existing columns and update the table's fixed width or minimum width.
pub fn update_dom_columns_on_resize(
    node: &Node,
    colgroup: &HtmlElement,
    table: &HtmlElement,
    default_cell_min_width: f64,
    override_width: Option<(usize, f64)>,
) -> Result<(), JsValue> {
    if node.child_count() == 0 {
        return Ok(());
    }
    let layout = update_columns_on_resize(node, default_cell_min_width, override_width);
    apply_layout(colgroup, table, &layout)
}

pub(super) fn apply_layout(colgroup: &HtmlElement, table: &HtmlElement, layout: &ColumnLayout) -> Result<(), JsValue> {
    let document = table.owner_document().ok_or_else(|| JsValue::from_str("Table has no owner document"))?;
    let mut next = colgroup.first_child();
    for width in &layout.columns {
        let column = if let Some(existing) = next {
            next = existing.next_sibling();
            existing.dyn_into::<HtmlElement>()?
        } else {
            let column = html_element(&document, "col")?;
            colgroup.append_child(&column)?;
            column
        };
        set_width(&column, "width", *width)?;
    }
    while let Some(extra) = next {
        next = extra.next_sibling();
        colgroup.remove_child(&extra)?;
    }
    set_width(table, "width", layout.width)?;
    set_width(table, "min-width", layout.min_width)
}

fn set_width(element: &HtmlElement, property: &str, width: Option<f64>) -> Result<(), JsValue> {
    let css = width.map_or_else(String::new, pixels);
    let style = element.style();
    if style.get_property_value(property)? != css {
        style.set_property(property, &css)?;
    }
    Ok(())
}

fn pixels(width: f64) -> String {
    format!("{width}px")
}

fn html_element(document: &Document, tag: &str) -> Result<HtmlElement, JsValue> {
    document.create_element(tag)?.dyn_into().map_err(JsValue::from)
}
