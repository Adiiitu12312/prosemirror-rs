use std::{collections::HashMap, rc::Rc, sync::OnceLock};

use prosemirror_editor::{EditorProps, EventAction, NodeView, PluginProps};
use prosemirror_history::{history, HistoryOptions};
use prosemirror_inputrules::{input_rules, wrapping_input_rule, Attrs};
use prosemirror_keymap::{KeyEvent, Modifiers, Platform};
use prosemirror_model::{AttributeSpec, MarkSpec, Node, NodeSpec, Schema};
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_tables::{table_editing, table_nodes, TableEditingOptions, TableNodesOptions};
use serde_json::{json, Value};

pub fn schema() -> &'static Schema {
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        let mut nodes = prosemirror_schema_list::add_list_nodes(
            prosemirror_schema_basic::nodes(), "paragraph block*", Some("block"),
        );
        nodes.extend(table_nodes(TableNodesOptions::default()));
        nodes.extend([
            ("task_list".into(), NodeSpec { content: Some("task_item+".into()), group: Some("block".into()), ..Default::default() }),
            ("task_item".into(), NodeSpec {
                content: Some("paragraph block*".into()),
                attrs: HashMap::from([("checked".into(), AttributeSpec::with_default(json!(false)))]),
                ..Default::default()
            }),
        ]);
        let mut marks = prosemirror_schema_basic::marks();
        marks.push(("strike".into(), MarkSpec::default()));
        Schema::new(nodes, marks)
    })
}

pub fn initialize(doc: Node, selection: Option<Selection>) -> EditorState {
    initialize_with_history(doc, selection, HistoryOptions::default())
}

pub fn initialize_with_history(doc: Node, selection: Option<Selection>, history_options: HistoryOptions) -> EditorState {
    let task_list = wrapping_input_rule(r"^\[ \] $", "", "task_list", Attrs::default(), None)
        .expect("valid task rule");
    EditorState::create(EditorStateConfig {
        schema: schema().clone(),
        doc: Some(doc),
        selection,
        plugins: vec![
            input_rules(vec![task_list]),
            prosemirror_example_setup::build_input_rules(schema()),
            history(history_options),
            table_editing(TableEditingOptions::default()),
        ],
        stored_marks: None,
    })
}

pub fn props(platform: Platform) -> EditorProps {
    let keymap = prosemirror_example_setup::build_keymap::<()>(schema(), &Default::default(), platform)
        .expect("valid editor keymap");
    let mac = matches!(platform, Platform::MacOs | Platform::Ios);
    let mut direct = PluginProps::default();
    direct.node_views.insert("table".into(), Rc::new(|_, _, _| Box::new(TableView)));
    direct.handle_key_down = Some(Rc::new(move |state, key| {
        let flags = key.modifiers;
        let modifier = if mac { flags.meta } else { flags.control };
        let mut transaction = None;
        let mut dispatch = |tr| transaction = Some(tr);
        if modifier && flags.shift && key.key.eq_ignore_ascii_case("x") {
            prosemirror_commands::toggle_mark("strike", None)(state, Some(&mut dispatch));
        } else if modifier && key.key.eq_ignore_ascii_case("k") {
            prosemirror_commands::toggle_mark("link", Some(json!({"href":"https://prosemirror.net/"})))(state, Some(&mut dispatch));
        } else {
            keymap.handle_keydown(state, &mut dispatch, &(), KeyEvent {
                key: &key.key,
                key_code: key.code,
                modifiers: Modifiers { alt: flags.alt, ctrl: flags.control, meta: flags.meta, shift: flags.shift },
                legacy_ie: false,
            });
        }
        transaction.map_or(EventAction::Continue, EventAction::transaction)
    }));
    EditorProps { direct, ..EditorProps::default() }
}

struct TableView;

impl NodeView for TableView {
    fn update(&mut self, node: &Node, _: usize, _: &prosemirror_editor::DecorationGroup) -> bool {
        node.type_name() == "table"
    }
}

pub fn sample() -> Node {
    let paragraph = |text: &str| json!({"type":"paragraph","content":[{"type":"text","text":text}]});
    let item = |text: &str| json!({"type":"list_item","content":[paragraph(text)]});
    let cell = |kind: &str, text: &str| json!({"type":kind,"content":[paragraph(text)]});
    let data = json!({"type":"doc","content":[
        {"type":"heading","attrs":{"level":1},"content":[{"type":"text","text":"Native editor"}]},
        {"type":"paragraph","content":[
            {"type":"text","text":"A Rust editing surface with "},
            {"type":"text","text":"strong","marks":[{"type":"strong"}]},
            {"type":"text","text":", "},
            {"type":"text","text":"emphasis","marks":[{"type":"em"}]},
            {"type":"text","text":", "},
            {"type":"text","text":"code","marks":[{"type":"code"}]},
            {"type":"text","text":" and "},
            {"type":"text","text":"links","marks":[{"type":"link","attrs":{"href":"https://prosemirror.net/"}}]},
            {"type":"text","text":"."}
        ]},
        {"type":"blockquote","content":[paragraph("Drag blocks by their left edge.")]},
        {"type":"bullet_list","content":[item("Paragraphs and lists")]},
        {"type":"ordered_list","content":[item("Keyboard shortcuts and undo")]},
        {"type":"task_list","content":[
            {"type":"task_item","attrs":{"checked":true},"content":[paragraph("Open the native sample")]},
            {"type":"task_item","content":[paragraph("Try a markdown shortcut")]}
        ]},
        {"type":"horizontal_rule"},
        {"type":"code_block","content":[{"type":"text","text":"let editor = Editor::new(state, host, props);"}]},
        {"type":"table","content":[
            {"type":"table_row","content":[cell("table_header","Feature"),cell("table_header","Host")]},
            {"type":"table_row","content":[cell("table_cell","Shaping"),cell("table_cell","Native")]}
        ]}
    ]});
    read_doc(&data)
}

pub fn read_doc(value: &Value) -> Node {
    schema().node_from_json(value).expect("valid sample document")
}
