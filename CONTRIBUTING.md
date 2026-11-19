# Contributing to prosemirror-rs

Thank you for your interest in contributing to `prosemirror-rs`.

## Development Principles

1. **Parity First**: The JavaScript ProseMirror reference implementation is authoritative. Any discrepancy in JSON formatting, position mapping, or step application between Rust and JS is treated as a defect.
2. **Differential Verification**: All modifications to `model`, `transform`, or `state` must pass differential fuzzing against `oracle/server.js`.
3. **Core portability**. Keep DOM and browser APIs in the view adapter. The document, transform, and state APIs are shared by native and WebAssembly editors.
4. **Source and test parity**. Use the pinned upstream implementation as the reference. Preserve source attribution and keep corresponding upstream test cases identifiable in Rust.

## Submitting Pull Requests

1. Ensure the Node oracle is tested:
   ```bash
   node oracle/test_oracle.js
   ```
2. Verify Rust compilation and tests in GitHub Actions across Linux, macOS, and Windows and in the Chrome and Firefox WebAssembly jobs.
3. If introducing changes to step application or position mapping, run the fuzzer with a high iteration count.
