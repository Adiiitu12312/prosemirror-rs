use std::collections::HashMap;
use std::sync::OnceLock;

use prosemirror_model::{DomAttribute, DomElement, DomNode};

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconSpec {
    Svg { path: String, width: u32, height: u32 },
    Text { text: String, css: Option<String> },
    Dom(DomNode),
}

impl IconSpec {
    pub fn svg(path: impl Into<String>, width: u32, height: u32) -> Self {
        Self::Svg { path: path.into(), width, height }
    }

    pub fn text(text: impl Into<String>, css: Option<String>) -> Self {
        Self::Text { text: text.into(), css }
    }

    /// The identifier used for this path by the upstream SVG symbol cache.
    pub fn symbol_id(&self) -> Option<String> {
        let Self::Svg { path, .. } = self else { return None };
        let hash = path.encode_utf16().fold(0_i32, |hash, unit| {
            hash.wrapping_mul(31).wrapping_add(i32::from(unit))
        });
        let sign = if hash.is_negative() { "-" } else { "" };
        Some(format!("pm-icon-{sign}{:x}", hash.unsigned_abs()))
    }

    /// Builds an icon button for hosts that render portable DOM. SVG paths are
    /// embedded so the result can be rendered without a document-global cache.
    pub fn to_dom(&self) -> DomNode {
        let icon = match self {
            Self::Svg { path, width, height } => {
                let ratio = f64::from(*width) / f64::from((*height).max(1));
                DomNode::Element(DomElement {
                    namespace: Some(SVG_NAMESPACE.to_owned()),
                    attributes: vec![
                        DomAttribute::new("viewBox", format!("0 0 {width} {height}")),
                        DomAttribute::new("style", format!("width: {ratio}em")),
                    ],
                    ..DomElement::new("svg", vec![DomNode::Element(DomElement {
                        namespace: Some(SVG_NAMESPACE.to_owned()),
                        attributes: vec![DomAttribute::new("d", path)],
                        ..DomElement::new("path", Vec::new())
                    })])
                })
            }
            Self::Text { text, css } => DomNode::Element(DomElement {
                attributes: css.as_ref().map(|css| DomAttribute::new("style", css)).into_iter().collect(),
                ..DomElement::new("span", vec![DomNode::text(text)])
            }),
            Self::Dom(dom) => dom.clone(),
        };
        DomNode::Element(DomElement {
            attributes: vec![DomAttribute::new("class", "ProseMirror-icon")],
            ..DomElement::new("button", vec![icon])
        })
    }
}

/// Upstream glyphs for join, lift, parent selection, undo, redo, marks, and lists.
pub fn icons() -> &'static HashMap<&'static str, IconSpec> {
    static ICONS: OnceLock<HashMap<&'static str, IconSpec>> = OnceLock::new();
    ICONS.get_or_init(|| HashMap::from(include!("stock_icons.in")))
}
