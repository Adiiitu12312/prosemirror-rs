//! Requires a desktop clipboard. The same test binary runs in a second
//! process to perform independent reads and writes against the OS.

#![cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]

use std::io::{self, BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use clipboard_rs::{Clipboard, ClipboardContent, ClipboardContext};
use prosemirror_editor::{ClipboardContent as EditorClipboard, paste_slice, serialize_clipboard};
use prosemirror_model::{Fragment, Schema, Slice};
use prosemirror_schema_basic::schema;
use serde_json::json;

#[path = "../src/system_clipboard.rs"]
mod system_clipboard;
use system_clipboard::{NATIVE_FORMAT, SystemClipboard};

const CASE: &str = "exchanges_clipboard_formats_across_processes";
const PEER: &str = "PROSEMIRROR_OS_CLIPBOARD_PEER";
const READY: &str = "prosemirror-clipboard-ready";
const TEXT: &str = "A&B 🍃";

#[test]
#[ignore = "requires an active OS clipboard (Xvfb on Linux)"]
fn exchanges_clipboard_formats_across_processes() {
    if let Ok(role) = std::env::var(PEER) {
        let clipboard = ClipboardContext::new().expect("system clipboard is unavailable to the peer");
        match role.as_str() {
            "read" => {
                assert_eq!(clipboard.get_text().unwrap(), TEXT);
                #[cfg(target_os = "linux")]
                if matches!(clipboard, ClipboardContext::X11(_)) {
                    assert_eq!(clipboard.get_buffer("text/plain").unwrap(), TEXT.as_bytes());
                }
                let html = clipboard.get_html().unwrap();
                assert!(html.contains("<strong>A&amp;B 🍃</strong>"), "{html}");
                assert!(html.contains("data-pm-slice"), "{html}");
                let bytes = clipboard.get_buffer(NATIVE_FORMAT).unwrap();
                let envelope = serde_json::Deserializer::from_slice(&bytes)
                    .into_iter::<serde_json::Value>().next().unwrap().unwrap();
                assert_eq!(envelope["format"], "prosemirror-native-slice");
            }
            "echo" => {
                let text = clipboard.get_text().unwrap();
                let html = clipboard.get_html().unwrap();
                publish_until_checked(&clipboard, vec![
                    ClipboardContent::Text(text),
                    ClipboardContent::Html(html),
                ]);
            }
            "write" => {
                let text = ClipboardContent::Text(TEXT.into());
                #[cfg(target_os = "linux")]
                let text = if matches!(clipboard, ClipboardContext::X11(_)) {
                    ClipboardContent::Other("text/plain;charset=utf-8".into(), TEXT.as_bytes().to_vec())
                } else { text };
                publish_until_checked(&clipboard, vec![
                    text,
                    ClipboardContent::Html("<p><em>A&amp;B 🍃</em></p>".into()),
                ]);
            }
            _ => panic!("unexpected clipboard peer role {role}"),
        }
        return;
    }

    let schema = schema();
    let source = schema.node_from_json(&json!({
        "type": "doc",
        "content": [{
            "type": "blockquote",
            "content": [{
                "type": "blockquote",
                "content": [{
                    "type": "paragraph",
                    "content": [{"type": "text", "text": TEXT, "marks": [{"type": "strong"}]}]
                }]
            }]
        }]
    })).unwrap();
    let original = source.slice(3, 3 + TEXT.encode_utf16().count(), true).unwrap();
    assert_eq!((original.open_start, original.open_end), (3, 3));
    let mut clipboard = SystemClipboard::default();
    clipboard.write(&serialize_clipboard(&original).unwrap());

    let output = peer("read").output().unwrap();
    assert!(
        output.status.success(), "{}\n{}",
        String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr),
    );

    let context = source.resolve(3).unwrap();
    with_clipboard_writer("echo", || {
        let incoming = clipboard.read().expect("echoed clipboard is empty");
        assert_eq!(incoming.plain_text, TEXT);
        assert!(incoming.internal.is_none(), "the peer echoes public formats only");
        let restored = paste_slice(schema, &incoming, false, &context).unwrap();
        assert_eq!(restored, Some(original));
    });

    with_clipboard_writer("write", || {
        let incoming = clipboard.read().expect("external clipboard is empty");
        assert_eq!(incoming.plain_text, TEXT);
        assert!(incoming.internal.is_none(), "external copy must replace the native representation");
        let slice = paste_slice(schema, &incoming, false, &context).unwrap().unwrap();
        let text = slice.content.first_child().unwrap().first_child().unwrap();
        assert_eq!(text.text(), Some(TEXT));
        assert_eq!(text.marks()[0].type_name, "em");
        let plain = paste_slice(schema, &incoming, true, &context).unwrap().unwrap();
        let text = plain.content.first_child().unwrap().first_child().unwrap();
        assert_eq!(text.text(), Some(TEXT));
        assert!(text.marks().iter().all(|mark| mark.type_name != "em"));
    });
}

fn publish_until_checked(clipboard: &ClipboardContext, formats: Vec<ClipboardContent>) {
    clipboard.set(formats).unwrap();
    println!("{READY}");
    io::stdin().read_to_end(&mut Vec::new()).unwrap();
}

fn with_clipboard_writer(role: &str, check: impl FnOnce()) {
    let mut writer = Peer(peer(role).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap());
    let stdout = BufReader::new(writer.0.stdout.take().unwrap());
    let (announce, ready) = mpsc::channel();
    let report = thread::spawn(move || {
        let mut report = String::new();
        for line in stdout.lines() {
            let line = line.unwrap();
            if line.ends_with(READY) { let _ = announce.send(()); }
            report.push_str(&line);
            report.push('\n');
        }
        report
    });
    ready.recv_timeout(Duration::from_secs(15)).expect("clipboard writer did not become ready");

    check();
    drop(writer.0.stdin.take());
    let status = writer.0.wait().unwrap();
    let report = report.join().unwrap();
    assert!(status.success(), "{report}");
}

fn peer(role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([CASE, "--exact", "--ignored", "--nocapture"]).env(PEER, role);
    command
}

struct Peer(Child);

impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn table_clipboard_preserves_row_wrappers_and_fragment_comments() {
    let mut nodes = prosemirror_schema_basic::nodes();
    nodes.extend(prosemirror_tables::table_nodes(prosemirror_tables::TableNodesOptions::default()));
    let schema = Schema::new(nodes, prosemirror_schema_basic::marks());
    let document = schema.top_node_type().create_and_fill(None, None, None).unwrap().unwrap();
    let row = json!({
        "type": "table_row",
        "content": [{
            "type": "table_cell",
            "content": [{"type": "paragraph", "content": [{"type": "text", "text": "123"}]}]
        }]
    });
    let expected = schema.slice_from_json(&json!({ "content": [row.clone(), row.clone()], "openStart": 1, "openEnd": 1 })).unwrap();
    let html = concat!(
        "<html><body><!--StartFragment--><table data-pm-slice=\"1 1 -2 []\"><tbody>",
        "<tr><td><p>123</p></td></tr><tr><td><p>123</p></td></tr>",
        "</tbody></table><!--EndFragment--></body></html>",
    );
    let context = document.resolve(1).unwrap();
    let incoming = EditorClipboard { html: Some(html.into()), ..EditorClipboard::default() };
    assert_eq!(paste_slice(&schema, &incoming, false, &context).unwrap(), Some(expected));

    let slice = Slice::new(Fragment::from_nodes(vec![schema.node_from_json(&row).unwrap()]), 0, 0);
    let mut copied = serialize_clipboard(&slice).unwrap();
    assert!(copied.html.as_deref().unwrap().starts_with("<table data-pm-slice=\"0 0 -2 []\"><tbody>"));
    copied.internal = None;
    assert_eq!(paste_slice(&schema, &copied, false, &context).unwrap(), Some(slice));
}
