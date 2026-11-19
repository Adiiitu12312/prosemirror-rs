use std::sync::OnceLock;
use prosemirror_model::{MarkSpec, NodeSpec, Schema};

pub fn test_schema() -> &'static Schema {
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        let nodes = vec![
            ("doc".to_string(), NodeSpec { content: Some("block+".to_string()), ..Default::default() }),
            ("paragraph".to_string(), NodeSpec { content: Some("inline*".to_string()), group: Some("block".to_string()), ..Default::default() }),
            ("blockquote".to_string(), NodeSpec { content: Some("block+".to_string()), group: Some("block".to_string()), ..Default::default() }),
            ("heading".to_string(), NodeSpec { content: Some("inline*".to_string()), group: Some("block".to_string()), ..Default::default() }),
            ("code_block".to_string(), NodeSpec { content: Some("text*".to_string()), group: Some("block".to_string()), code: true, ..Default::default() }),
            ("horizontal_rule".to_string(), NodeSpec { group: Some("block".to_string()), ..Default::default() }),
            ("text".to_string(), NodeSpec { inline: true, group: Some("inline".to_string()), ..Default::default() }),
        ];
        let marks = vec![
            ("em".to_string(), MarkSpec { ..Default::default() }),
            ("strong".to_string(), MarkSpec { ..Default::default() }),
            ("code".to_string(), MarkSpec { ..Default::default() }),
            ("link".to_string(), MarkSpec { ..Default::default() }),
        ];
        Schema::new(nodes, marks)
    })
}
