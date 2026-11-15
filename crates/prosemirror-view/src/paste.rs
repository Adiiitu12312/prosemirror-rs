use prosemirror_model::{Fragment, Mark, Node, Schema, Slice};

pub struct GoogleDocsParser;

impl GoogleDocsParser {
    pub fn is_google_docs_html(html: &str) -> bool {
        html.contains("docs-internal-guid") || html.contains("id=\"docs-internal-guid")
    }

    pub fn parse_clipboard_html(html: &str, schema: &Schema) -> Slice {
        let is_docs = Self::is_google_docs_html(html);

        // Extract text and inline formatting
        let mut blocks = Vec::new();
        let cleaned = clean_html(html);

        for line in cleaned.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let mut marks = Vec::new();
            if is_docs {
                if html.contains("font-weight:700") || html.contains("font-weight: 700") || html.contains("<b>") {
                    if let Some(m) = schema.mark_type("strong") {
                        marks.push(m.create(None));
                    }
                }
                if html.contains("font-style:italic") || html.contains("font-style: italic") || html.contains("<i>") {
                    if let Some(m) = schema.mark_type("em") {
                        marks.push(m.create(None));
                    }
                }
            }

            let text_node = Node::new_text(trimmed, marks);
            let paragraph = schema.node(
                "paragraph",
                None,
                Some(Fragment::from_nodes(vec![text_node])),
                None,
            ).unwrap();
            blocks.push(paragraph);
        }

        if blocks.is_empty() {
            Slice::empty()
        } else {
            Slice::new(Fragment::from_nodes(blocks), 1, 1)
        }
    }
}

fn clean_html(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            text.push(ch);
        }
    }
    text
}
