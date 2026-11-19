# prosemirror-rs

A Rust port of [ProseMirror](https://prosemirror.net/) for native editors. The workspace contains the document model, transforms, editor state, editing commands, and editor plugins. The existing WebAssembly adapter is opt-in.

[![CI Pipeline](https://github.com/Adiiitu12312/prosemirror-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/Adiiitu12312/prosemirror-rs/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## Upstream references

| Module | Reference version | Rust implementation and tests |
| --- | --- | --- |
| `prosemirror-model` | 1.25.13 | Schema, content expressions, document tree, UTF-16 positions, marks, replacement, model DOM parsing and serialization |
| `prosemirror-transform` | 1.12.7 | Built-in and registered custom steps, maps, slice fitting, structural transforms, and mark operations |
| `prosemirror-state` | 1.4.4 | Selections, transactions, editor state, and plugin lifecycle |
| `prosemirror-history` | 1.5.2 | Undo, redo, history grouping, remapping, and RopeSequence 1.3.4 |
| `prosemirror-commands` | 1.7.2 | Editing commands and desktop base keymaps |
| `prosemirror-schema-basic` | 1.2.3 | Basic node, mark, and DOM specifications |
| `prosemirror-schema-list` | 1.4.1 | List specifications and editing commands |
| `prosemirror-test-builder` | 1.1.1 | Document builders, mark builders, and named positions |
| `prosemirror-collab` | 1.3.1 | Client step queue, remote changes, rebasing, and synchronization |
| `prosemirror-keymap` | 1.2.3 | Key normalization and command dispatch |
| `prosemirror-inputrules` | 1.7.0 | Text input rules, rule builders, and undo |
| `prosemirror-gapcursor` | 1.4.0 | Gap selection, navigation, and browser cursor rendering |
| `prosemirror-dropcursor` | 1.8.4 native / 1.8.2 browser | Drop target mapping, timing, and opt-in browser overlays |
| `prosemirror-tables` | 1.8.5 | Table schema, grid mapping, cell selection and editing, clipboard, and browser column resizing |
| `prosemirror-changeset` | 2.4.0 | Changed ranges, span merging, token diffs, and change simplification |
| `prosemirror-markdown` | 1.13.4 | Markdown schema, parsing, and serialization |
| `prosemirror-search` | 1.1.0 | Query matching, search commands, replacement, and highlights |
| `prosemirror-menu` | 1.3.0 | Native and browser menus, dropdowns, focus navigation, and stock commands |
| `prosemirror-example-setup` | 1.2.3 | Standard input rules, key bindings, menus, prompts, and plugin configuration |
| `prosemirror-view` | 1.42.9 | Native and browser editor controllers, decoration trees, DOM reconciliation, and clipboard logic |
| `prosemirror-editor` | View 1.42.9 | Native event controller and versioned renderer interface |
| `prosemirror-fuzz` | — | Rust and JavaScript differential test harness |
| `prosemirror-integration-tests` | — | Workspace integration tests |
| `editor-app` | — | Native reference application at `examples/editor-app` |

The schema uses the ordering semantics of `orderedmap` 2.1.1. The JavaScript history reference uses `rope-sequence` 1.3.4. The Rust key name tables match `w3c-keyname` 2.2.8.

The model, transform, state, history, commands, list, test-builder, collaboration, Markdown, search, changeset, and table suites carry cases from the corresponding upstream test files. All eleven ProseMirror View 1.42.9 test files have Rust ports, with portable decoration and clipboard tests and WebAssembly equivalents for the browser DOM cases.

The default workspace build is native-first and has no browser dependencies. Enable a crate's `browser` Cargo feature on `wasm32-unknown-unknown` to use its existing browser adapter. The CI browser jobs enable `browser-tests` explicitly.

Positions and internal text edits use UTF-16 code units. The model exposes `Node::text_utf16()` for exact access to isolated surrogate code units. Rust UTF-8, DOM text, and JSON APIs represent those isolated units with replacement characters.

## Native editor and host interface

`prosemirror-editor` wraps an `EditorState` in an `Editor<H>` where `H` implements `Host`. Send typed keyboard, composition, pointer, touch, clipboard, focus, scroll, or drag events through `Editor::handle`. An application can dispatch transactions directly, install direct and plugin view props, and register native node view factories. State plugins continue to own transaction filtering and appending.

Host interface version 1 is exposed as `HOST_INTERFACE_VERSION`. Positions use UTF-16 code units and each host chooses its coordinate units. The host first measures the display list via `Host::update_layout` and then draws it via `Host::draw`. `BlockUpdate` supplies the previous and current changed top-level block index ranges. Hosts can reuse layout outside those ranges and reconcile paint-only changes from the current display list. The display list carries blocks, shaped-text inputs with marks, decorations, widgets, selections, composition, and drag feedback.

- `Layout` supplies caret and point hit testing, visual line boxes, block bounds, selection rectangles, and preedit caret placement. The host owns Unicode text shaping and maps grapheme clusters and bidirectional visual runs to editor positions.
- `Host` connects clipboard, IME candidate positioning, cursor icons, focus, viewport scrolling, and a structured accessibility outline. Display geometry uses a single coordinate space. A windowed host translates its pointer and IME coordinates at the platform boundary.
- `NodeView` supports native controllers with update, selection, event filtering, mutation filtering, and destruction callbacks. Direct view props take precedence over view plugin props and attached state plugin props.

The clipboard codec writes plain text and an opaque native slice envelope with its open depths intact. Browser-specific parsing and selection behavior remain in the opt-in browser adapter.

## Native reference application

The sample opens a native document with headings, nested lists and task items, quotations, code, and a table. Use the left gutter to drag blocks. Select and move text with the mouse or long-press and drag touch handles. The sample uses the system's standard modifier for bold, italic, code, strike, link, copy, paste, and undo or redo, and supports Markdown-style prefixes such as `# `, `> `, `- `, `1. `, and `[ ] `.

- **Winit** creates the window and reports input, focus, and IME events. The application positions the system IME candidate window using the measured preedit caret.
- **Cosmic Text** shapes and measures marked runs using native font fallback. Its Swash integration rasterizes glyphs for the CPU display list. **fontdb** supplies the isolated, bundled Inter font database used in scripted tests.
- **Softbuffer** presents the same CPU buffer used by headless replay. The **png** crate saves that buffer as an offscreen snapshot without starting a window.
- **Arboard** reads and writes system text clipboard data. The application retains the opaque rich slice inside the process while it owns the matching text, so rich copy and paste between independent processes uses the plain-text representation.

The scripted binary accepts a JSON document, selection, and native event sequence through `--replay INPUT`. It prints the resulting state JSON and can produce an offscreen PNG with `--png OUTPUT`. Use `--snapshot PNG` to render the sample. CI compares decoded pixels against the checked-in sample, typing, selection, and block-drag images, then publishes screenshots from all three desktop platforms.

The release-mode benchmark is invoked separately as `--bench core` and `--bench surface` so each host starts in a fresh process. It reports total time, mean time per input, and on Linux the process's current and peak resident memory as JSON lines. The core run includes state, history, and display-list construction. The surface run additionally includes layout and rasterization at 900 × 760. CI publishes measurements from the Linux runner.

## Verification

Run the JavaScript reference checks locally.

```shell
npm ci
npm test
```

GitHub Actions compiles Rust and runs the workspace tests, doctests, differential fuzzer, and Clippy on Linux, macOS, and Windows. The browser jobs compile the WebAssembly view and run its Rust integration tests in headless Chrome and Firefox. The fuzzer compares application outcomes, document JSON, step maps, position mappings and deletion flags, and inverse steps with the pinned JavaScript oracle. A mismatch writes `repro_case_<iteration>.json`.

The original Rust integration tests are registered in `crates/prosemirror-integration-tests`. Crate-specific tests live alongside the ports. Fixture-driven tests retain the upstream test names and evaluate the behavior in Rust against the upstream expectations.
