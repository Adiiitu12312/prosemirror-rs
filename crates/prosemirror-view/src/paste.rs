use prosemirror_model::{Fragment, Node, Schema, Slice};

pub struct GoogleDocsParser;

impl GoogleDocsParser {
    pub fn is_google_docs_html(html: &str) -> bool {
        html.contains("docs-internal-guid") || html.contains("id=\"docs-internal-guid")
    }

    pub fn parse_clipboard_html(html: &str, schema: &Schema) -> Slice {
        let chunks = parse_styled_chunks(html);

        let mut inline_nodes = Vec::new();
        for (text, is_bold, is_italic) in chunks {
            if text.is_empty() {
                continue;
            }
            let mut marks = Vec::new();
            if is_bold {
                if let Some(m) = schema.mark_type("strong") {
                    marks.push(m.create(None));
                }
            }
            if is_italic {
                if let Some(m) = schema.mark_type("em") {
                    marks.push(m.create(None));
                }
            }
            inline_nodes.push(Node::new_text(text, marks));
        }

        if inline_nodes.is_empty() {
            return Slice::empty();
        }

        let paragraph = schema
            .node(
                "paragraph",
                None,
                Some(Fragment::from_nodes(inline_nodes)),
                None,
            )
            .unwrap();

        Slice::new(Fragment::from_nodes(vec![paragraph]), 1, 1)
    }
}

fn parse_styled_chunks(html: &str) -> Vec<(String, bool, bool)> {
    let mut chunks = Vec::new();
    let mut cur_text = String::new();
    let mut style_stack: Vec<(bool, bool)> = vec![(false, false)];

    let mut chars = html.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            let mut tag = String::new();
            for inner in chars.by_ref() {
                if inner == '>' {
                    break;
                }
                tag.push(inner);
            }

            let tag_lower = tag.to_lowercase();
            let is_closing = tag_lower.starts_with('/');

            if is_closing {
                if !cur_text.is_empty() {
                    let &(bold, italic) = style_stack.last().unwrap_or(&(false, false));
                    chunks.push((cur_text.clone(), bold, italic));
                    cur_text.clear();
                }
                if style_stack.len() > 1 {
                    style_stack.pop();
                }
            } else {
                if !cur_text.is_empty() {
                    let &(bold, italic) = style_stack.last().unwrap_or(&(false, false));
                    chunks.push((cur_text.clone(), bold, italic));
                    cur_text.clear();
                }

                let parent_style = *style_stack.last().unwrap_or(&(false, false));
                let mut bold = parent_style.0;
                let mut italic = parent_style.1;

                if tag_lower.starts_with("b ") || tag_lower == "b" || tag_lower.starts_with("strong") {
                    bold = true;
                }
                if tag_lower.starts_with("i ") || tag_lower == "i" || tag_lower.starts_with("em") {
                    italic = true;
                }
                if tag_lower.contains("font-weight:700") || tag_lower.contains("font-weight: 700") || tag_lower.contains("font-weight:bold") {
                    bold = true;
                }
                if tag_lower.contains("font-weight:normal") || tag_lower.contains("font-weight: 400") {
                    bold = false;
                }
                if tag_lower.contains("font-style:italic") || tag_lower.contains("font-style: italic") {
                    italic = true;
                }

                style_stack.push((bold, italic));
            }
        } else {
            cur_text.push(ch);
        }
    }

    if !cur_text.is_empty() {
        let &(bold, italic) = style_stack.last().unwrap_or(&(false, false));
        let trimmed = cur_text.trim();
        if !trimmed.is_empty() {
            chunks.push((trimmed.to_string(), bold, italic));
        }
    }

    chunks
}
