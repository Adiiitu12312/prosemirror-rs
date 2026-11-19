# Contributing to prosemirror-rs

Thank you for your interest in contributing to `prosemirror-rs`.

## Development Principles

1. **Parity First**: The JavaScript ProseMirror reference implementation is authoritative. Any discrepancy in JSON formatting, position mapping, or step application between Rust and JS is treated as a defect.
2. **Differential Verification**: All modifications to `model`, `transform`, or `state` must pass differential fuzzing against `oracle/server.js`.
3. **No DOM/Browser Assumptions**: The core engine must remain free of Web APIs, Chromium dependencies, and DOM assumptions. Native rendering is managed through `prosemirror-view`.
4. **Source and test parity**. Use the pinned upstream implementation as the reference. Preserve source attribution and keep corresponding upstream test cases identifiable in Rust.

## Submitting Pull Requests

1. Ensure the Node oracle is tested:
   ```bash
   node oracle/test_oracle.js
   ```
2. Verify Rust compilation and tests in GitHub Actions across all three platforms (Linux, macOS, Windows).
3. If introducing changes to step application or position mapping, run the fuzzer with a high iteration count.
