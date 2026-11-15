# prosemirror-rs

A pure-Rust, DOM-independent implementation of the **ProseMirror** logic layer with 1-to-1 parity, differential fuzzing against the JavaScript reference oracle, and a native view architecture.

[![CI Pipeline](https://github.com/prosemirror-rs/prosemirror-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/prosemirror-rs/prosemirror-rs/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

---

## Architecture Overview

```
                      +-----------------------------+
                      |   prosemirror-commands      |
                      +--------------+--------------+
                                     |
                      +--------------v--------------+
                      |    prosemirror-history      |
                      +--------------+--------------+
                                     |
                      +--------------v--------------+
                      |     prosemirror-state       |
                      +--------------+--------------+
                                     |
                      +--------------v--------------+
                      |   prosemirror-transform     |
                      +--------------+--------------+
                                     |
                      +--------------v--------------+
                      |     prosemirror-model       |
                      +-----------------------------+
                                     ^
         +---------------------------+---------------------------+
         |                                                       |
+--------+---------------+                             +---------+---------+
|    prosemirror-view    |                             | prosemirror-fuzz  |
|  (Parley, AccessKit,   |                             |   (Differential   |
|      cosmic-text)      |                             |   Node Oracle)    |
+------------------------+                             +-------------------+
```

### Crate Dependency Order

1. **`prosemirror-model`**: Document tree representation (`Node`, `Fragment`, `Mark`, `Slice`, `ResolvedPos`, `Schema`). Fully interoperable with ProseMirror JSON format.
2. **`prosemirror-transform`**: Document modification primitives (`StepMap`, `Mapping`, `Step`, `ReplaceStep`, `Transform`). Exact position mapping parity with ProseMirror.
3. **`prosemirror-state`**: Immutable editor state (`EditorState`, `Selection`, `Transaction`, `Plugin`, `StateField`).
4. **`prosemirror-history`**: Undo and redo history manager with branch compression and selection bookmarks.
5. **`prosemirror-commands`**: Core editing commands (`split_block`, `join_backward`, `delete_selection`, `toggle_mark`, `chain_commands`).
6. **`prosemirror-test-builder`**: Test DSL and macros (`doc!`, `p!`, tagged positions `<a>...<b>`) matching `prosemirror-test-builder`.
7. **`prosemirror-view`**: Native view architecture (no Chromium or DOM):
   - Text layout & shaping via **Parley** and **cosmic-text**.
   - Accessibility tree synchronization with **AccessKit**.
   - Windows IME / TSF composition state management.
   - Mobile touch selection handles with 44pt touch targets and loupe magnifier.
   - Google Docs clipboard parser and HTML sanitizer.
8. **`prosemirror-fuzz`**: Continuous differential fuzzer testing Rust implementation against the Node.js ProseMirror oracle.

---

## Pinned Upstream Reference Versions

To guarantee 1-to-1 behavioral parity and prevent divergence, the Rust logic layer is pinned against the following ProseMirror npm releases:

| Upstream Package | Pinned Version | Role |
| :--- | :--- | :--- |
| `prosemirror-model` | `1.24.1` | Document tree, schema, positions |
| `prosemirror-transform` | `1.10.2` | Steps, step maps, mapping algorithms |
| `prosemirror-state` | `1.4.3` | Selections, transactions, plugin lifecycle |
| `prosemirror-history` | `1.4.1` | Undo/redo stack management |
| `prosemirror-commands` | `1.6.2` | Keymap commands & text manipulation |
| `prosemirror-test-builder` | `1.1.1` | Declarative test structure and tags |

---

## Differential Fuzzing with Node Oracle

Differential fuzzing ensures 100% parity by comparing Rust outcomes directly against upstream JavaScript ProseMirror:

1. The fuzzer generates arbitrary valid ProseMirror document trees.
2. It generates valid transactions and steps (`ReplaceStep`, `AddMarkStep`, `RemoveMarkStep`).
3. It executes the steps concurrently in Rust and in Node.js via `oracle/server.js`.
4. It compares:
   - Output document JSON (must be identical).
   - `StepMap` ranges and position mappings (must be identical).
   - Inversion outcomes (must be identical).
5. If any mismatch occurs, it emits a standalone `repro_case_<id>.json` reproducer file.

### Running Differential Fuzzing

```bash
# Verify the Node oracle
node oracle/test_oracle.js

# Run differential fuzzing harness (via GitHub Actions CI or locally if toolchains present)
cargo run --bin prosemirror-fuzz -- --iterations 500 --oracle oracle/server.js
```

---

## Native View Spikes

`prosemirror-view` addresses 5 notoriously complex rich-text editing challenges in a native context:

1. **Hindi via Gboard**: Employs Unicode extended grapheme cluster segmentation (`unicode-segmentation`) to prevent splitting Devanagari conjuncts and halant viramas.
2. **iOS Selection Handles**: Apple Human Interface Guidelines compliant 44pt hit targets, teardrop grabbers, and a floating loupe magnifier.
3. **Windows IME (TSF)**: Non-destructive composition strings anchored to caret screen coordinates without polluting the undo history until committed.
4. **Screen Readers via AccessKit**: Continuous projection of `EditorState` into an accessible `AccessKit` tree (`Role::Document`, `Role::Paragraph`, `Role::InlineTextBox`).
5. **Google Docs Clipboard**: Extracts semantic structures, stripping `docs-internal-guid` wrappers while preserving bold, italic, and heading marks.

---

## Continuous Integration

Compilation and verification run in GitHub Actions on every push and pull request across Linux, Windows, and macOS:

- `cargo fmt --check`
- `cargo check --workspace --all-targets`
- `cargo test --workspace --verbose`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo run --bin prosemirror-fuzz -- --iterations 200`
