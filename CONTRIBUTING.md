# Contributing to prosemirror-rs

Thank you for your interest in contributing to `prosemirror-rs`.

## Development Principles

1. **Parity First**: The JavaScript ProseMirror reference implementation is authoritative. Any discrepancy in JSON formatting, position mapping, or step application between Rust and JS is treated as a defect.
2. **Differential Verification**: All modifications to `model`, `transform`, or `state` must pass differential fuzzing against `oracle/server.js`.
3. **Core portability**. Build new editor and view behavior for native hosts. Keep the existing WebAssembly adapters behind the opt-in `browser` feature.
4. **Source and test parity**. Use the pinned upstream implementation as the reference. Preserve source attribution and keep corresponding upstream test cases identifiable in Rust.

## Submitting Pull Requests

1. Ensure the Node oracle is tested:
   ```bash
   node oracle/test_oracle.js
   ```
2. Verify Rust compilation, the test suites, and the JavaScript oracle in GitHub Actions. Merge each verified step into `main`.
3. If introducing changes to step application or position mapping, run the fuzzer with a high iteration count.
