use std::collections::HashMap;

use prosemirror_commands as commands;
use prosemirror_model::{AttributeSpec, MarkSpec, NodeSpec, Schema};
use prosemirror_schema_list as lists;
use prosemirror_state::{EditorState, EditorStateConfig, Selection};
use prosemirror_test_builder::test_schema;
use serde_json::Value;

pub fn verify_case(json: &str, index: usize) {
    let cases: Value = serde_json::from_str(json).expect("valid upstream fixture file");
    let case = &cases[index];
    for step in case["steps"].as_array().unwrap() {
        let schema = schema(&step["schema"]);
        let input = &step["before"];
        let doc = schema.node_from_json(&input["doc"]).unwrap();
        let selection: Selection = serde_json::from_value(input["selection"].clone()).unwrap();
        let stored_marks = input["stored_marks"].as_array().map(|marks| {
            marks.iter().map(|mark| schema.mark_from_json(mark).unwrap()).collect()
        });
        let state = EditorState::create(EditorStateConfig {
            schema,
            doc: Some(doc),
            selection: Some(selection),
            stored_marks,
            plugins: Vec::new(),
        });
        let mut next = state.clone();
        let mut dispatch_count = 0;
        let handled = run(&step["command"], &state, Some(&mut |tr| {
            dispatch_count += 1;
            next = next.apply(&tr);
        }));
        let label = format!("{} / {}", case["group"], case["title"]);
        assert_eq!(handled, step["handled"].as_bool().unwrap(), "{label}: handled");
        assert_eq!(dispatch_count, step["count"].as_u64().unwrap(), "{label}: dispatch count");
        assert_eq!(serde_json::to_value(&next.doc).unwrap(), step["after"]["doc"], "{label}: document");
        assert_eq!(serde_json::to_value(&next.selection).unwrap(), step["after"]["selection"], "{label}: selection");
        assert_eq!(serde_json::to_value(&next.stored_marks).unwrap(), step["after"]["stored_marks"], "{label}: stored marks");
    }
}

fn run(command: &Value, state: &EditorState, dispatch: commands::DispatchFn<'_>) -> bool {
    let args = &command["args"];
    let name = || args[0].as_str().expect("a type name");
    let attrs = || value(&args[1]);
    match command["name"].as_str().unwrap() {
        "joinBackward" => commands::join_backward(state, dispatch),
        "joinForward" => commands::join_forward(state, dispatch),
        "joinTextblockBackward" => commands::join_textblock_backward(state, dispatch),
        "joinTextblockForward" => commands::join_textblock_forward(state, dispatch),
        "selectNodeBackward" => commands::select_node_backward(state, dispatch),
        "selectNodeForward" => commands::select_node_forward(state, dispatch),
        "deleteSelection" => commands::delete_selection(state, dispatch),
        "joinUp" => commands::join_up(state, dispatch),
        "joinDown" => commands::join_down(state, dispatch),
        "lift" => commands::lift(state, dispatch),
        "wrapIn" => commands::wrap_in(name(), attrs())(state, dispatch),
        "splitBlock" => commands::split_block(state, dispatch),
        "splitBlockAs" => commands::split_block_as(|_, at_end, _| {
            let arg = &args[0];
            if arg["only_at_end"].as_bool().unwrap_or(false) && !at_end {
                return None;
            }
            let kind = state.schema.node_type(arg["type"].as_str()?)?;
            Some(kind.create(value(&arg["attrs"]), None, None))
        })(state, dispatch),
        "splitBlockKeepMarks" => commands::split_block_keep_marks(state, dispatch),
        "liftEmptyBlock" => commands::lift_empty_block(state, dispatch),
        "createParagraphNear" => commands::create_paragraph_near(state, dispatch),
        "setBlockType" => commands::set_block_type(name(), attrs())(state, dispatch),
        "selectParentNode" => commands::select_parent_node(state, dispatch),
        "selectTextblockStart" => commands::select_textblock_start(state, dispatch),
        "selectTextblockEnd" => commands::select_textblock_end(state, dispatch),
        "toggleMark" => {
            let options = commands::ToggleMarkOptions {
                remove_when_present: args[2]["removeWhenPresent"].as_bool().unwrap_or(true),
                enter_inline_atoms: args[2]["enterInlineAtoms"].as_bool().unwrap_or(true),
                include_whitespace: args[2]["includeWhitespace"].as_bool().unwrap_or(false),
            };
            commands::toggle_mark_with_options(name(), attrs(), options)(state, dispatch)
        }
        "autoJoin" => {
            let names = args[1].as_array().unwrap();
            commands::auto_join(
                |state, dispatch| run(&args[0], state, dispatch),
                |before, _| names.iter().any(|name| name.as_str() == Some(before.type_name())),
            )(state, dispatch)
        }
        "wrapInList" => lists::wrap_in_list(name(), attrs())(state, dispatch),
        "splitListItem" => lists::split_list_item(name(), attrs())(state, dispatch),
        "splitListItemKeepMarks" => lists::split_list_item_keep_marks(name(), attrs())(state, dispatch),
        "liftListItem" => lists::lift_list_item(name())(state, dispatch),
        "sinkListItem" => lists::sink_list_item(name())(state, dispatch),
        unknown => panic!("unrecognized upstream command {unknown}"),
    }
}

fn schema(spec: &Value) -> Schema {
    if spec.is_null() {
        return test_schema().clone();
    }
    let nodes = spec["nodes"].as_array().unwrap().iter().map(|pair| {
        let spec = &pair[1];
        let node = NodeSpec {
            content: string(&spec["content"]),
            marks: string(&spec["marks"]),
            group: string(&spec["group"]),
            attrs: attributes(&spec["attrs"]),
            inline: flag(spec, "inline", false),
            atom: flag(spec, "atom", false),
            selectable: flag(spec, "selectable", true),
            draggable: flag(spec, "draggable", false),
            code: flag(spec, "code", false),
            whitespace: string(&spec["whitespace"]),
            defining: flag(spec, "defining", false),
            defining_as_context: flag(spec, "definingAsContext", false),
            defining_for_content: flag(spec, "definingForContent", false),
            isolating: flag(spec, "isolating", false),
            linebreak_replacement: flag(spec, "linebreakReplacement", false),
            ..NodeSpec::default()
        };
        (pair[0].as_str().unwrap().to_owned(), node)
    }).collect();
    let marks = spec["marks"].as_array().unwrap().iter().map(|pair| {
        let spec = &pair[1];
        let mark = MarkSpec {
            attributes: attributes(&spec["attrs"]),
            inclusive: flag(spec, "inclusive", true),
            excludes: string(&spec["excludes"]),
            group: string(&spec["group"]),
            spanning: flag(spec, "spanning", true),
            ..MarkSpec::default()
        };
        (pair[0].as_str().unwrap().to_owned(), mark)
    }).collect();
    Schema::try_new(nodes, marks, spec["top_node"].as_str()).unwrap()
}

fn attributes(spec: &Value) -> HashMap<String, AttributeSpec> {
    spec.as_object().into_iter().flatten().map(|(name, attr)| {
        (name.clone(), AttributeSpec::new(attr.get("default").cloned(), attr["validate"].as_str()))
    }).collect()
}

fn value(value: &Value) -> Option<Value> {
    (!value.is_null()).then(|| value.clone())
}

fn string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

fn flag(value: &Value, name: &str, default: bool) -> bool {
    value[name].as_bool().unwrap_or(default)
}
