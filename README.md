# prosemirror-rs

A Rust port of [ProseMirror](https://prosemirror.net/) for native editors. The workspace contains the document model, transforms, editor state, editing commands, and editor plugins. The existing WebAssembly adapter is opt-in.

[![CI Pipeline](https://github.com/Adiiitu12312/prosemirror-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/Adiiitu12312/prosemirror-rs/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## Upstream references

| Module | Reference version | Rust implementation and tests |
| --- | --- | --- |
| `prosemirror-model` | 1.25.13 | Schema, content expressions, document tree, UTF-16 positions, marks, replacement, model DOM parsing and serialization |
| `prosemirror-transform` | 1.10.2 | Built-in and registered custom steps, maps, slice fitting, structural transforms, and mark operations |
| `prosemirror-state` | 1.4.3 | Selections, transactions, editor state, and plugin lifecycle |
| `prosemirror-history` | 1.4.1 | Undo, redo, history grouping, and remapping |
| `prosemirror-commands` | 1.6.2 | Editing commands and desktop base keymaps |
| `prosemirror-schema-basic` | 1.2.3 | Basic node, mark, and DOM specifications |
| `prosemirror-schema-list` | 1.4.1 | List specifications and editing commands |
| `prosemirror-test-builder` | 1.1.1 | Document builders, mark builders, and named positions |
| `prosemirror-collab` | 1.3.1 | Client step queue, remote changes, rebasing, and synchronization |
| `prosemirror-keymap` | 1.2.3 | Key normalization and command dispatch |
| `prosemirror-inputrules` | 1.5.0 | Text input rules, rule builders, and undo |
| `prosemirror-gapcursor` | 1.4.0 | Gap selection, navigation, and browser cursor rendering |
| `prosemirror-dropcursor` | 1.8.2 | Drop target mapping, timing, and browser overlays |
| `prosemirror-tables` | 1.8.5 | Table schema, grid mapping, cell selection and editing, clipboard, and browser column resizing |
| `prosemirror-changeset` | 2.4.0 | Changed ranges, span merging, token diffs, and change simplification |
| `prosemirror-markdown` | 1.13.4 | Markdown schema, parsing, and serialization |
| `prosemirror-search` | 1.1.0 | Query matching, search commands, replacement, and highlights |
| `prosemirror-menu` | 1.3.0 | Native and browser menus, dropdowns, focus navigation, and stock commands |
| `prosemirror-example-setup` | 1.2.3 | Standard input rules, key bindings, menus, prompts, and plugin configuration |
| `prosemirror-view` | 1.42.9 | Native and browser editor controllers, decoration trees, DOM reconciliation, and clipboard logic |
| `prosemirror-fuzz` | — | Rust and JavaScript differential test harness |
| `prosemirror-integration-tests` | — | Workspace integration tests |

The schema uses the ordering semantics of `orderedmap` 2.1.1. The JavaScript history reference uses `rope-sequence` 1.3.4. The Rust key name tables match `w3c-keyname` 2.2.8.

The model, transform, state, history, commands, list, test-builder, collaboration, Markdown, search, changeset, and table suites carry cases from the corresponding upstream test files. All eleven ProseMirror View 1.42.9 test files have Rust ports, with portable decoration and clipboard tests and WebAssembly equivalents for the browser DOM cases.

The default workspace build is native-first and has no browser dependencies. Enable a crate's `browser` Cargo feature on `wasm32-unknown-unknown` to use its existing browser adapter. The CI browser jobs enable `browser-tests` explicitly.

Positions and internal text edits use UTF-16 code units. The model exposes `Node::text_utf16()` for exact access to isolated surrogate code units. Rust UTF-8, DOM text, and JSON APIs represent those isolated units with replacement characters.

## Verification

Run the JavaScript reference checks locally.

```shell
npm ci
npm test
```

GitHub Actions compiles Rust and runs the workspace tests, doctests, differential fuzzer, and Clippy on Linux, macOS, and Windows. The browser jobs compile the WebAssembly view and run its Rust integration tests in headless Chrome and Firefox. The fuzzer compares application outcomes, document JSON, step maps, position mappings and deletion flags, and inverse steps with the pinned JavaScript oracle. A mismatch writes `repro_case_<iteration>.json`.

The original Rust integration tests are registered in `crates/prosemirror-integration-tests`. Crate-specific tests live alongside the ports. Fixture-driven tests retain the upstream test names and evaluate the behavior in Rust against the upstream expectations.
