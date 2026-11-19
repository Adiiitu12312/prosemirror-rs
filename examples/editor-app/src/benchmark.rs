use std::{error::Error, fs, time::Instant};

use prosemirror_editor::{BlockUpdate, ClipboardEvent, DisplayList, Editor, Event, Host, Layout, Point, PositionAtCoords, Rect};
use prosemirror_history::{close_history, redo, undo, HistoryOptions};
use prosemirror_keymap::Platform;
use prosemirror_model::{Fragment, Slice};
use serde_json::{json, Value};

use crate::{setup, surface::SurfaceHost};

struct CoreHost;

impl Layout for CoreHost {
    fn coords_at_pos(&self, _: usize, _: i32) -> Option<Rect> { None }
    fn pos_at_coords(&self, _: Point) -> Option<PositionAtCoords> { None }
}

impl Host for CoreHost {
    fn incremental_display_list(&self) -> bool { true }
    fn draw(&mut self, _: &DisplayList, _: &BlockUpdate) {}
    fn update_accessibility_state(&mut self, _: &prosemirror_state::EditorState) {}
}

pub(super) fn run_core(blocks: usize) -> Result<(), Box<dyn Error>> {
    run("core", blocks, || CoreHost)
}

pub(super) fn run_surface(blocks: usize) -> Result<(), Box<dyn Error>> {
    run("surface", blocks, || SurfaceHost::deterministic(900, 760))
}

fn document(blocks: usize) -> Value {
    let paragraphs: Vec<_> = (0..blocks).map(|index| json!({
        "type": "paragraph",
        "content": [{"type": "text", "text": format!("Paragraph {index:04} with editable text.")}],
    })).collect();
    json!({"type": "doc", "content": paragraphs})
}

fn report(host: &str, blocks: usize, operation: &str, count: usize, started: Instant) {
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let memory = fs::read_to_string("/proc/self/status").unwrap_or_default();
    let memory_kib = |label: &str| {
        memory.lines().find_map(|line| {
            line.strip_prefix(label)?.split_whitespace().next()?.parse::<u64>().ok()
        })
    };
    println!("{}", json!({
        "host": host,
        "blocks": blocks,
        "operation": operation,
        "count": count,
        "total_ms": elapsed_ms,
        "mean_ms": elapsed_ms / count as f64,
        "rss_kib": memory_kib("VmRSS:"),
        "peak_rss_kib": memory_kib("VmHWM:"),
    }));
}

fn run<H: Host>(host_name: &str, blocks: usize, host: impl FnOnce() -> H) -> Result<(), Box<dyn Error>> {
    const CHARACTERS: usize = 1_000;
    const PASTE_BLOCKS: usize = 500;
    const HISTORY_STEPS: usize = 200;
    let saved = serde_json::to_vec(&document(blocks))?;

    let open_start = Instant::now();
    let doc = setup::schema().node_from_json(&serde_json::from_slice(&saved)?)?;
    let history = HistoryOptions { depth: HISTORY_STEPS + 20, ..HistoryOptions::default() };
    let state = setup::initialize_with_history(doc, None, history);
    let platform = Platform::Other;
    let mut editor = Editor::with_platform(state, host(), setup::props(platform), platform);
    editor.handle(Event::Focus)?;
    report(host_name, blocks, "open", 1, open_start);

    let type_start = Instant::now();
    for _ in 0..CHARACTERS {
        if !editor.handle(Event::TextInput("x".into()))? {
            return Err("Typing benchmark did not apply an input event".into());
        }
    }
    report(host_name, blocks, "type_1000_characters", CHARACTERS, type_start);

    let source = setup::schema().node_from_json(&document(PASTE_BLOCKS))?;
    let slice = Slice::new(Fragment::from_nodes(source.content().children.clone()), 0, 0);
    let clipboard = prosemirror_editor::serialize_clipboard(&slice)?;
    let paste_start = Instant::now();
    let old_children = editor.state().doc.child_count();
    editor.handle(Event::Clipboard(ClipboardEvent::PasteContent { content: clipboard, plain_text: false }))?;
    if editor.state().doc.child_count() < old_children + PASTE_BLOCKS - 1 {
        return Err("Paste benchmark did not insert its block slice".into());
    }
    report(host_name, blocks, "paste_500_blocks", 1, paste_start);

    let before_history = editor.state().doc.clone();
    for _ in 0..HISTORY_STEPS {
        let mut transaction = editor.state().tr();
        close_history(&mut transaction).insert_text_at_selection("h");
        editor.dispatch(transaction)?;
    }
    let after_history = editor.state().doc.clone();
    let undo_start = Instant::now();
    for _ in 0..HISTORY_STEPS {
        let mut transaction = None;
        if !undo(editor.state(), |tr| transaction = Some(tr)) {
            return Err("Undo benchmark ran out of history".into());
        }
        editor.dispatch(transaction.expect("undo dispatches a history transaction"))?;
    }
    report(host_name, blocks, "undo_200", HISTORY_STEPS, undo_start);
    if editor.state().doc != before_history {
        return Err("Undo benchmark failed to restore the pre-edit document".into());
    }
    let redo_start = Instant::now();
    for _ in 0..HISTORY_STEPS {
        let mut transaction = None;
        if !redo(editor.state(), |tr| transaction = Some(tr)) {
            return Err("Redo benchmark ran out of history".into());
        }
        editor.dispatch(transaction.expect("redo dispatches a history transaction"))?;
    }
    report(host_name, blocks, "redo_200", HISTORY_STEPS, redo_start);
    if editor.state().doc != after_history {
        return Err("Redo benchmark failed to restore the edited document".into());
    }
    Ok(())
}
