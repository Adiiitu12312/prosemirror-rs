//! Plain text and versioned native Slice clipboard data.

use prosemirror_model::{Fragment, ModelError, ResolvedPos, Schema, Slice};
use serde_json::{json, Value};

use crate::ClipboardContent;

const FORMAT: &str = "prosemirror-native-slice";
const VERSION: u64 = 1;

pub fn serialize_clipboard(slice: &Slice) -> Result<ClipboardContent, ModelError> {
    let plain_text = slice.content.text_between(0, slice.content.size(), Some("\n\n"), None);
    let internal = serde_json::to_vec(&json!({
        "format": FORMAT,
        "version": VERSION,
        "slice": slice,
    })).map_err(|error| ModelError::Serialization(error.to_string()))?;
    Ok(ClipboardContent { plain_text, internal: Some(internal) })
}

/// Recover a native slice, or interpret plain text in the insertion context.
pub fn paste_slice(
    schema: &Schema, data: &ClipboardContent, plain_text: bool, context: &ResolvedPos,
) -> Result<Option<Slice>, ModelError> {
    let in_code = context.parent().node_type().is_some_and(|kind| kind.spec.code);
    if !plain_text && !in_code {
        if let Some(slice) = native_slice(schema, data.internal.as_deref()) {
            return Ok(Some(slice));
        }
    }
    if data.plain_text.is_empty() {
        return Ok(None);
    }
    let text = data.plain_text.replace("\r\n", "\n").replace('\r', "\n");
    if in_code {
        return Ok(Some(Slice::new(Fragment::from_nodes(vec![schema.try_text(&text, None)?]), 0, 0)));
    }

    let kind = (0..=context.depth).rev().find_map(|depth| {
        let parent = context.node(depth);
        let matched = parent.content_match_at(context.index_after(depth))?;
        let default = schema.node_type(matched.default_type()?)?;
        (default.is_textblock() && !default.has_required_attrs()).then_some(default)
    }).or_else(|| {
        schema.node_type("paragraph").filter(|kind| kind.is_textblock())
    }).or_else(|| {
        schema.node_types().find(|kind| kind.is_textblock() && !kind.has_required_attrs())
    });

    let marks = context.marks();
    let Some(kind) = kind else {
        let text = text.replace('\n', " ");
        return Ok(Some(Slice::new(Fragment::from_nodes(vec![schema.try_text(&text, Some(marks))?]), 0, 0)));
    };
    let mut blocks = Vec::new();
    let mut start = 0;
    let mut newline_run = false;
    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            if !newline_run {
                blocks.push(textblock(schema, kind, &text[start..index], &marks)?);
            }
            start = index + 1;
        }
        newline_run = ch == '\n';
    }
    blocks.push(textblock(schema, kind, &text[start..], &marks)?);
    Ok(Some(Slice::max_open(Fragment::from_nodes(blocks), true)))
}

fn textblock(
    schema: &Schema, kind: &prosemirror_model::NodeType, text: &str, marks: &[prosemirror_model::Mark],
) -> Result<prosemirror_model::Node, ModelError> {
    let content = if text.is_empty() {
        Fragment::empty()
    } else {
        let marks = kind.allowed_marks(marks);
        Fragment::from_nodes(vec![schema.try_text(text, Some(marks))?])
    };
    kind.create_and_fill(None, Some(content), None)?
        .ok_or_else(|| ModelError::InvalidContent(kind.name.clone()))
}

fn native_slice(schema: &Schema, data: Option<&[u8]>) -> Option<Slice> {
    let encoded: Value = serde_json::from_slice(data?).ok()?;
    if encoded["format"].as_str() != Some(FORMAT) || encoded["version"].as_u64() != Some(VERSION) {
        return None;
    }
    let slice = schema.slice_from_json(encoded.get("slice")?).ok()?;
    let allowed = Slice::max_open(slice.content.clone(), true);
    (slice.open_start <= allowed.open_start && slice.open_end <= allowed.open_end).then_some(slice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prosemirror_test_builder::{blockquote, doc, p, test_schema};

    #[test]
    fn preserves_an_open_slice_and_uses_plain_text_as_a_fallback() {
        let doc = doc!(blockquote!(p!("a"), p!("b")), p!("c")).node;
        let context = doc.resolve(3).unwrap();
        let original = doc.slice(2, 6, true).unwrap();
        let data = serialize_clipboard(&original).unwrap();
        assert_eq!(paste_slice(test_schema(), &data, false, &context).unwrap(), Some(original));
        assert_eq!(data.plain_text, "a\n\nb");
        let plain = paste_slice(test_schema(), &data, true, &context).unwrap().unwrap();
        assert_eq!(plain.open_start, 1);
        assert_eq!(plain.content.child_count(), 2);
    }
}
