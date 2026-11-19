//! Requires a desktop clipboard. The same test binary runs in a second
//! process to perform independent reads and writes against the OS.

#![cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]

use std::io::{self, BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use clipboard_rs::{Clipboard, ClipboardContent, ClipboardContext};
use prosemirror_editor::{paste_slice, serialize_clipboard};
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
                let html = clipboard.get_html().unwrap();
                assert!(html.contains("<strong>A&amp;B 🍃</strong>"), "{html}");
                assert!(html.contains("data-pm-slice"), "{html}");
                let bytes = clipboard.get_buffer(NATIVE_FORMAT).unwrap();
                let envelope = serde_json::Deserializer::from_slice(&bytes)
                    .into_iter::<serde_json::Value>().next().unwrap().unwrap();
                assert_eq!(envelope["format"], "prosemirror-native-slice");
            }
            "write" => {
                clipboard.set(vec![
                    ClipboardContent::Text(TEXT.into()),
                    ClipboardContent::Html("<p><em>A&amp;B 🍃</em></p>".into()),
                ]).unwrap();
                println!("{READY}");
                io::stdin().read_to_end(&mut Vec::new()).unwrap();
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
                "type": "paragraph",
                "content": [{"type": "text", "text": TEXT, "marks": [{"type": "strong"}]}]
            }]
        }]
    })).unwrap();
    let original = source.slice(2, 2 + TEXT.encode_utf16().count(), true).unwrap();
    let mut clipboard = SystemClipboard::default();
    clipboard.write(&serialize_clipboard(&original).unwrap());

    let output = peer("read").output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));

    let mut writer = Peer(peer("write").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap());
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

    let incoming = clipboard.read().expect("external clipboard is empty");
    assert_eq!(incoming.plain_text, TEXT);
    assert!(incoming.internal.is_none(), "external copy must replace the native representation");
    let context = source.resolve(2).unwrap();
    let slice = paste_slice(schema, &incoming, false, &context).unwrap().unwrap();
    let text = slice.content.first_child().unwrap().first_child().unwrap();
    assert_eq!(text.text(), Some(TEXT));
    assert_eq!(text.marks()[0].type_name, "em");
    let plain = paste_slice(schema, &incoming, true, &context).unwrap().unwrap();
    let text = plain.content.first_child().unwrap().first_child().unwrap();
    assert_eq!(text.text(), Some(TEXT));
    assert!(text.marks().iter().all(|mark| mark.type_name != "em"));

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
