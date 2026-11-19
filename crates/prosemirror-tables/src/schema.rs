//! Port of prosemirror-tables@1.8.5/src/schema.ts.

use std::collections::HashMap;

use prosemirror_model::{AttributeSpec, DomAttribute, DomElement, DomOutputSpec, DomParseRule, Node, NodeSpec, NodeType, ParseAttrs, Schema};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TableRole {
    Table,
    Row,
    Cell,
    HeaderCell,
}

impl TableRole {
    pub(crate) fn name(self) -> &'static str {
        match self { Self::Table => "table", Self::Row => "row", Self::Cell => "cell", Self::HeaderCell => "header_cell" }
    }
}

pub struct TableNodesOptions {
    pub table_group: Option<String>,
    pub cell_content: String,
    pub cell_attributes: HashMap<String, AttributeSpec>,
}

impl Default for TableNodesOptions {
    fn default() -> Self {
        Self { table_group: Some("block".into()), cell_content: "block+".into(), cell_attributes: HashMap::new() }
    }
}

pub fn table_nodes(options: TableNodesOptions) -> Vec<(String, NodeSpec)> {
    let mut attrs = HashMap::from([
        ("colspan".into(), AttributeSpec::new(Some(json!(1)), Some("number"))),
        ("rowspan".into(), AttributeSpec::new(Some(json!(1)), Some("number"))),
        ("colwidth".into(), AttributeSpec::with_default(Value::Null).with_validator(|value| {
            value.is_null() || value.as_array().is_some_and(|widths| widths.iter().all(Value::is_number))
        })),
    ]);
    attrs.extend(options.cell_attributes);
    let cell = |role: TableRole, tag: &str, format: fn(&Node) -> DomOutputSpec| NodeSpec {
        content: Some(options.cell_content.clone()),
        attrs: attrs.clone(),
        table_role: Some(role.name().into()),
        isolating: true,
        parse_dom: vec![DomParseRule { get_tag_attrs: Some(parse_cell), ..DomParseRule::tag(tag) }],
        to_dom: Some(format),
        ..Default::default()
    };
    vec![
        ("table".into(), NodeSpec {
            content: Some("table_row+".into()),
            table_role: Some(TableRole::Table.name().into()),
            isolating: true,
            group: options.table_group.clone(),
            parse_dom: vec![DomParseRule::tag("table")],
            to_dom: Some(|_| DomOutputSpec::element("table", vec![DomOutputSpec::element("tbody", vec![DomOutputSpec::Hole])])),
            ..Default::default()
        }),
        ("table_row".into(), NodeSpec {
            content: Some("(table_cell | table_header)*".into()),
            table_role: Some(TableRole::Row.name().into()),
            parse_dom: vec![DomParseRule::tag("tr")],
            to_dom: Some(|_| DomOutputSpec::element("tr", vec![DomOutputSpec::Hole])),
            ..Default::default()
        }),
        ("table_cell".into(), cell(TableRole::Cell, "td", |node| cell_dom(node, "td"))),
        ("table_header".into(), cell(TableRole::HeaderCell, "th", |node| cell_dom(node, "th"))),
    ]
}

pub fn table_node_types(schema: &Schema) -> HashMap<TableRole, &NodeType> {
    let roles = [TableRole::Table, TableRole::Row, TableRole::Cell, TableRole::HeaderCell];
    schema.node_types().filter_map(|kind| {
        let role = kind.spec.table_role.as_deref()?;
        roles.iter().copied().find(|variant| variant.name() == role).map(|role| (role, kind))
    }).collect()
}

fn parse_cell(dom: &DomElement) -> ParseAttrs {
    let colspan = number_attr(dom, "colspan");
    let widths = dom.attr("data-colwidth").and_then(|widths| {
        let parsed: Option<Vec<u64>> = widths.split(',').map(|width| {
            (!width.is_empty() && width.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| width.parse().ok()).flatten()
        }).collect();
        parsed.filter(|widths| widths.len() as u64 == colspan)
    });
    ParseAttrs::Accept(Some(json!({"colspan": colspan, "rowspan": number_attr(dom, "rowspan"), "colwidth": widths})))
}

fn number_attr(dom: &DomElement, name: &str) -> u64 {
    dom.attr(name).and_then(|value| value.parse().ok()).unwrap_or(1)
}

fn cell_dom(node: &Node, tag: &str) -> DomOutputSpec {
    let mut attrs = Vec::new();
    if let Some(cell) = node.attrs() {
        for name in ["colspan", "rowspan"] {
            if let Some(span) = cell[name].as_u64().filter(|span| *span != 1) {
                attrs.push(DomAttribute::new(name, span.to_string()));
            }
        }
        if let Some(widths) = cell["colwidth"].as_array() {
            let widths = widths.iter().map(|width| {
                width.as_f64().map_or_else(|| width.to_string(), |number| number.to_string())
            }).collect::<Vec<_>>().join(",");
            attrs.push(DomAttribute::new("data-colwidth", widths));
        }
    }
    DomOutputSpec::element_with_attrs(tag, attrs, vec![DomOutputSpec::Hole])
}
