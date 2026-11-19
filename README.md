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

Host interface version 2 is exposed as `HOST_INTERFACE_VERSION`. Positions use UTF-16 code units and each host chooses its coordinate units. The host first measures the display list via `Host::update_layout` and then draws it via `Host::draw`. `BlockUpdate` supplies the previous and current changed top-level block index ranges. Retained hosts opt in through `incremental_display_list` and receive a `block_range` for local replacements or an empty patch for paint-only changes. The host resolves block geometry on demand. The display list carries blocks, marked text runs, decorations, widgets, selections, composition, and drag feedback.

- `Layout` supplies caret and point hit testing, visual line boxes, block bounds, selection rectangles, and preedit caret placement. `viewport_selection_rects` lets retained hosts limit painting to visible selection geometry. The host owns Unicode text shaping and maps grapheme clusters and bidirectional visual runs to editor positions.
- `Host` connects clipboard, IME candidate positioning, cursor icons, focus, viewport scrolling, and a structured accessibility outline. A host can retain the accessibility state and generate the outline when requested. Display geometry uses a single coordinate space. A windowed host translates its pointer and IME coordinates at the platform boundary.
- `NodeView` supports native controllers with update, selection, event filtering, mutation filtering, and destruction callbacks. Direct view props take precedence over view plugin props and attached state plugin props.

The clipboard codec writes plain text, an HTML fragment, and an opaque native slice envelope. The HTML carries ProseMirror's `data-pm-slice` open-start, open-end, and ancestor context and is parsed against the active schema. Paste can also read ordinary HTML from other applications.

## Native reference application

The sample opens a native document with headings, nested lists and task items, quotations, code, and a table. Use the left gutter to drag blocks. Select and move text with the mouse or long-press and drag touch handles. The sample uses the system's standard modifier for bold, italic, code, strike, link, copy, paste, and undo or redo, and supports Markdown-style prefixes such as `# `, `> `, `- `, `1. `, and `[ ] `.

Use Enter to add a task item and Ctrl+Shift+Enter or Cmd+Shift+Enter to toggle the current item.

- **Winit** creates the window and reports input, focus, and IME events. The application positions the system IME candidate window using the measured preedit caret.
- **Cosmic Text** shapes and measures marked runs and its Swash integration rasterizes glyphs for the CPU display list. **fontdb** supplies the bundled Inter and JetBrains Mono faces for regular, bold, italic, and bold italic text. The desktop also loads system fallback fonts. Renderer tests check resolved style and shaped font metadata independently of the screenshots.
- **Softbuffer** presents the same CPU buffer used by headless replay. The **png** crate saves that buffer as an offscreen snapshot without starting a window.
- **clipboard-rs** publishes plain text, HTML, and the native slice in one OS clipboard write. The HTML format lets other applications read and write rich content. CI exchanges these formats with a separate process on Linux under Xvfb, macOS, and Windows.

The scripted binary accepts a JSON document, selection, and native event sequence through `--replay INPUT`. It prints the resulting state JSON and can produce an offscreen PNG with `--png OUTPUT`. Use `--snapshot PNG` to render the sample. CI compares decoded pixels against the checked-in sample, typing, selection, block-drag, and task-list images, then publishes screenshots from all three desktop platforms.

The release-mode benchmark takes `--bench core [BLOCKS]` or `--bench surface [BLOCKS]`. Each run starts a fresh process and reports wall time, mean time per input, and the current and peak resident memory on Linux. The core run includes state, history, and display-list projection. The surface run additionally includes layout and rasterization at 900 × 760. CI runs both at 5,000, 10,000, and 50,000 blocks and publishes the raw trials.

### Benchmark results

The 5,000-block measurements are milliseconds per operation. Each typing trial averages 1,000 characters and each undo or redo trial averages 200 steps. The optimized columns are the median of three release trials on GitHub Actions' Ubuntu runner. [Original release run](https://github.com/Adiiitu12312/prosemirror-rs/actions/runs/44569027482) · [Optimized raw trials](https://github.com/Adiiitu12312/prosemirror-rs/actions/runs/44577055176/artifacts/14556607772)

| Workload | Headless before | Headless after | Sample before | Sample after |
| --- | ---: | ---: | ---: | ---: |
| Open 5,000 blocks | 18.46 | 9.71 | 177.72 | 13.88 |
| Type one character | 6.73 | 0.234 | 10.26 | 1.003 |
| Paste a 500-block slice | 11.41 | 6.41 | 35.62 | 11.35 |
| Undo one step | 12.77 | 0.364 | 15.81 | 0.764 |
| Redo one step | 11.99 | 0.350 | 16.03 | 0.756 |

| Document size | Host | Open | Type | Paste | Undo | Redo |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 10,000 | Headless | 21.99 | 0.445 | 10.58 | 0.594 | 0.575 |
| 10,000 | Sample | 26.44 | 1.212 | 17.13 | 0.982 | 0.973 |
| 50,000 | Headless | 128.77 | 2.228 | 40.91 | 2.283 | 2.314 |
| 50,000 | Sample | 136.84 | 2.880 | 61.94 | 2.658 | 2.648 |

CI checks the median of three trials at each size. It fails for a slowdown above 20% relative to the committed reference, or for missing a 5,000-block budget. The headless budgets are 0.5 ms per character and 1 ms per undo or redo. The sample budgets are 100 ms to open, 4 ms per character, and 5 ms per undo or redo. [Benchmark reference](benchmarks/editor-reference.json)

### Release profile

Instrumented stage timings on the original 5,000-block revision showed that headless typing spent 2.99 ms per character projecting the full display list, 3.90 ms rebuilding the accessibility outline, and 0.60 ms discovering node views. The sample also spent 3.15 ms updating layout and 0.67 ms drawing per character. Initial sample layout consumed 180.10 ms. Constructing the undo or redo transaction cost about 1.8 ms per step, and applying either direction to headless state cost 3.7–4.0 ms. Those timings come from a separate instrumented release run. [Baseline profile](https://github.com/Adiiitu12312/prosemirror-rs/actions/runs/44575205911)

Single-block replacements use the step's affected block to project and shape only the changed scene. The sample indexes model positions and vertical extents, shapes the viewport lazily, and projects accessibility on demand. History keeps typed state and undo metadata and produces the existing JSON on request. Arbitrary decoration changes and structural transactions can still require a wider projection. Editing the model still copies its flat child vector, which is one remaining linear cost.

### Behavior differences

The differential suite names and asserts all three exceptions against [one fixture](oracle/parity-exceptions.json). Run `npm run test:parity-exceptions` for the [repros](oracle/parity-exceptions.mjs), or pass one case name after `--`. The generic differential fuzzer still fails on every mismatch.

- Transform 1.12.7 computes inverted mirror indices from the full backing map array. The Rust port computes mirrors relative to the appended slice so they stay in range when the source is a sliced mapping. [Upstream `Mapping.appendMappingInverted`](https://code.haverbeke.berlin/prosemirror/prosemirror-transform/src/commit/fe4882e9aca29ae9a9841cb747624cf12c4da719/src/map.ts#L236)
- Upstream can emit nested mark sets when inverting a group, which its JSON decoder rejects. The Rust port flattens the inversion while preserving operation order, so history can round-trip the result. [Upstream mark set inversion and JSON decoder](https://code.haverbeke.berlin/prosemirror/prosemirror-transform/src/commit/fe4882e9aca29ae9a9841cb747624cf12c4da719/src/mark_step.ts#L173)
- With the pinned History 1.5.2, the unmodified Collab 1.3.1 redo scenario produces `ABDCE`. The Rust port retains the scenario and checks that result. [Collab redo test](https://github.com/ProseMirror/prosemirror-collab/blob/c019e4cd1e05504d403d98e6bfec67fe1a80c895/test/test-collab.ts) · [History selection restoration](https://code.haverbeke.berlin/prosemirror/prosemirror-history/src/commit/4042c6ce7be9fac86c850760cbbbf2651fc79017/src/history.ts)
- Native IME and touch use host events with transaction comparisons in the differential suite. [Upstream View input](https://code.haverbeke.berlin/prosemirror/prosemirror-view/src/commit/2a07ebc03ccfb526b43e60297465129a2cee8ca6/src/input.ts)

## Verification

Run the JavaScript reference checks locally.

```shell
npm ci
npm test
```

GitHub Actions compiles Rust and runs the workspace tests, doctests, differential fuzzer, and Clippy on Linux, macOS, and Windows. The browser jobs compile the WebAssembly view and run its Rust integration tests in headless Chrome and Firefox. The fuzzer compares application outcomes, document JSON, step maps, position mappings and deletion flags, and inverse steps with the pinned JavaScript oracle. A mismatch writes `repro_case_<iteration>.json`.

The original Rust integration tests are registered in `crates/prosemirror-integration-tests`. Crate-specific tests live alongside the ports. Fixture-driven tests retain the upstream test names and evaluate the behavior in Rust against the upstream expectations.

The native editor differential suite uses the same pinned ProseMirror packages. It checks document JSON, selection, stored marks, mapped decorations, and clipboard text after each scripted or seeded event. For native IME and touch it also compares the resulting transactions with the upstream model and asserts the native preedit and handle display state. The seeded replays drain undo history and verify the original document is restored exactly.
