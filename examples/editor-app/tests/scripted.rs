#![cfg(not(target_arch = "wasm32"))]

use std::{fs, path::PathBuf, process::Command};

use serde_json::{json, Value};

const APP: &str = env!("CARGO_BIN_EXE_editor-app");

fn replay(name: &str, width: u32, height: u32) -> Value {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(format!("{name}.json"));
    let png = std::env::temp_dir().join(format!("prosemirror-editor-app-{}-{name}.png", std::process::id()));
    let output = Command::new(APP).arg("--replay").arg(input).arg("--png").arg(&png)
        .output().expect("start the real editor application");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    let bytes = fs::read(&png).expect("rendered offscreen image");
    fs::remove_file(png).expect("remove the temporary image");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), width);
    assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), height);
    serde_json::from_slice(&output.stdout).expect("document and selection from replay")
}

#[test]
fn typing_shortcut_ime_and_plain_paste() {
    let output = replay("typing", 420, 200);
    assert_eq!(output["doc"], json!({"type":"doc","content":[{"type":"paragraph","content":[
        {"type":"text","text":"ab"}, {"type":"text","text":"!","marks":[{"type":"strong"}]},
        {"type":"text","text":"é?"}
    ]}]}));
    assert_eq!(output["selection"], json!({"type":"text","anchor":6,"head":6}));
}

#[test]
fn mouse_drag_and_touch_handle_drag() {
    let output = replay("selection", 420, 220);
    assert_eq!(output["doc"], json!({"type":"doc","content":[
        {"type":"paragraph","content":[{"type":"text","text":"alpha beta"}]},
        {"type":"paragraph","content":[{"type":"text","text":"gamma"}]}
    ]}));
    assert_eq!(output["selection"], json!({"type":"text","anchor":7,"head":16}));
}

#[test]
fn block_drag_reorders_document() {
    let output = replay("block_drag", 420, 220);
    assert_eq!(output["doc"], json!({"type":"doc","content":[
        {"type":"paragraph","content":[{"type":"text","text":"second"}]},
        {"type":"paragraph","content":[{"type":"text","text":"first"}]}
    ]}));
    assert_eq!(output["selection"], json!({"type":"node","anchor":8}));
}

#[test]
fn markdown_task_can_be_checked_and_followed_by_an_unchecked_item() {
    let output = replay("task_list", 420, 220);
    assert_eq!(output["doc"], json!({"type":"doc","content":[{"type":"task_list","content":[
        {"type":"task_item","attrs":{"checked":true},"content":[
            {"type":"paragraph","content":[{"type":"text","text":"Buy milk"}]}
        ]},
        {"type":"task_item","attrs":{"checked":false},"content":[
            {"type":"paragraph","content":[{"type":"text","text":"Call"}]}
        ]}
    ]}]}));
    assert_eq!(output["selection"], json!({"type":"text","anchor":19,"head":19}));
}
