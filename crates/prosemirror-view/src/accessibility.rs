use prosemirror_state::EditorState;
use crate::layout::ViewLayout;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccessibilityRole {
    Document,
    Paragraph,
    Heading,
    Blockquote,
    InlineTextBox,
}

#[derive(Clone, Debug)]
pub struct AccessibilityNode {
    pub id: u64,
    pub role: AccessibilityRole,
    pub label: Option<String>,
    pub value: Option<String>,
    pub children: Vec<u64>,
}

#[derive(Clone, Debug)]
pub struct AccessibilityTreeUpdate {
    pub nodes: Vec<AccessibilityNode>,
    pub root_id: u64,
    pub focus_id: u64,
}

pub fn build_accessibility_tree(state: &EditorState, _layout: &ViewLayout) -> AccessibilityTreeUpdate {
    let mut nodes = Vec::new();
    let root_id = 1;
    let mut next_id = 2;
    let mut root_children = Vec::new();

    for block in &state.doc.content().children {
        let block_id = next_id;
        next_id += 1;
        root_children.push(block_id);

        let role = match block.type_name() {
            "heading" => AccessibilityRole::Heading,
            "blockquote" => AccessibilityRole::Blockquote,
            _ => AccessibilityRole::Paragraph,
        };

        let text_content = block.text_between(0, block.content().size());
        let text_box_id = next_id;
        next_id += 1;

        nodes.push(AccessibilityNode {
            id: text_box_id,
            role: AccessibilityRole::InlineTextBox,
            label: None,
            value: Some(text_content.clone()),
            children: Vec::new(),
        });

        nodes.push(AccessibilityNode {
            id: block_id,
            role,
            label: Some(text_content),
            value: None,
            children: vec![text_box_id],
        });
    }

    nodes.push(AccessibilityNode {
        id: root_id,
        role: AccessibilityRole::Document,
        label: Some("ProseMirror Editor".to_string()),
        value: None,
        children: root_children,
    });

    AccessibilityTreeUpdate {
        nodes,
        root_id,
        focus_id: root_id,
    }
}
