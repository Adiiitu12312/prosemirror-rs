//! Host-neutral table column layout from prosemirror-tables@1.8.5/src/tableview.ts.

use prosemirror_model::{DomAttribute, DomElement, DomNode, Node};
use serde_json::Value;

use crate::commands::get_span;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColumnLayout {
    pub columns: Vec<Option<f64>>,
    pub width: Option<f64>,
    pub min_width: Option<f64>,
}

pub fn update_columns_on_resize(table: &Node, default_min_width: f64, override_width: Option<(usize, f64)>) -> ColumnLayout {
    let Some(row) = table.content().children.first() else { return ColumnLayout::default() };
    let mut columns = Vec::new();
    for cell in &row.content().children {
        let widths = cell.attrs().and_then(|attrs| attrs["colwidth"].as_array());
        for col in 0..get_span(cell, "colspan") {
            let width = match override_width {
                Some((index, width)) if index == columns.len() => Some(width),
                _ => widths.and_then(|widths| widths.get(col)).and_then(Value::as_f64),
            };
            columns.push(width.filter(|width| *width != 0.0));
        }
    }
    let total = columns.iter().map(|width| width.unwrap_or(default_min_width)).sum();
    if columns.iter().all(Option::is_some) {
        ColumnLayout { columns, width: Some(total), min_width: None }
    } else {
        ColumnLayout { columns, width: None, min_width: Some(total) }
    }
}

pub struct TableView {
    pub node: Node,
    pub default_cell_min_width: f64,
    pub layout: ColumnLayout,
}

impl TableView {
    pub fn new(node: Node, default_cell_min_width: f64) -> Self {
        let layout = update_columns_on_resize(&node, default_cell_min_width, None);
        Self { node, default_cell_min_width, layout }
    }

    pub fn update(&mut self, node: Node) -> bool {
        if self.node.type_name() != node.type_name() {
            return false;
        }
        self.layout = update_columns_on_resize(&node, self.default_cell_min_width, None);
        self.node = node;
        true
    }

    pub fn render(&self, rows: Vec<DomNode>) -> DomNode {
        let mut style = format!("--default-cell-min-width: {}px", self.default_cell_min_width);
        if let Some(width) = self.layout.width {
            style.push_str(&format!("; width: {width}px"));
        }
        if let Some(width) = self.layout.min_width {
            style.push_str(&format!("; min-width: {width}px"));
        }
        let cols = self.layout.columns.iter().map(|width| {
            let attrs = width.map_or_else(Vec::new, |width| vec![DomAttribute::new("style", format!("width: {width}px"))]);
            element("col", attrs, Vec::new())
        }).collect();
        let table = element("table", vec![DomAttribute::new("style", style)], vec![
            DomNode::element("colgroup", cols),
            DomNode::element("tbody", rows),
        ]);
        element("div", vec![DomAttribute::new("class", "tableWrapper")], vec![table])
    }
}

fn element(name: &str, attributes: Vec<DomAttribute>, children: Vec<DomNode>) -> DomNode {
    let mut element = DomElement::new(name, children);
    element.attributes = attributes;
    DomNode::Element(element)
}
