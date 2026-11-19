mod generator;
mod oracle_client;

use std::fs;
use generator::RandomGen;
use oracle_client::OracleClient;
use prosemirror_transform::StepResult;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let iterations: usize = args
        .iter()
        .position(|a| a == "--iterations")
        .and_then(|idx| args.get(idx + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    let oracle_path = args
        .iter()
        .position(|a| a == "--oracle")
        .and_then(|idx| args.get(idx + 1))
        .cloned()
        .unwrap_or_else(|| "oracle/server.js".to_string());

    println!("Starting ProseMirror Differential Fuzzer...");
    println!("Oracle: {}", oracle_path);
    println!("Target iterations: {}", iterations);

    let client = OracleClient::new(&oracle_path);
    let mut rng = RandomGen::new(42);
    let mut passed = 0;

    for i in 1..=iterations {
        let doc = rng.random_doc();
        let step = rng.random_step(&doc);

        let rust_result = step.apply(&doc);
        let doc_json = serde_json::to_value(&doc).unwrap();
        let step_json = serde_json::to_value(&step).unwrap();

        match client.apply_step(&doc_json, &step_json) {
            Ok(oracle_resp) => {
                match rust_result {
                    StepResult::Ok(rust_doc) => {
                        if !oracle_resp.success {
                            panic_repro(i, &doc_json, &step_json, &format!(
                                "Rust succeeded but JS failed: {:?}", oracle_resp.error,
                            ));
                        }
                        let rust_doc_json = serde_json::to_value(&rust_doc).unwrap();
                        let js_doc_json = oracle_resp.doc.unwrap();
                        if rust_doc_json != js_doc_json {
                            panic_repro(
                                i,
                                &doc_json,
                                &step_json,
                                &format!("Doc JSON mismatch!\nRust: {}\nJS:   {}", rust_doc_json, js_doc_json),
                            );
                        }

                        // Also verify step map ranges
                        let rust_map = step.get_map();
                        if Some(&rust_map.ranges) != oracle_resp.step_map.as_ref() {
                                panic_repro(
                                    i,
                                    &doc_json,
                                    &step_json,
                                    &format!("StepMap mismatch!\nRust: {:?}\nJS:   {:?}", rust_map.ranges, oracle_resp.step_map),
                                );
                        }
                        let positions: Vec<_> = (0..=doc.content().size())
                            .flat_map(|pos| [-1, 1].map(|assoc| {
                                let mapped = rust_map.map_result(pos, assoc);
                                (mapped.pos, mapped.deleted(), mapped.deleted_before(), mapped.deleted_after(), mapped.deleted_across())
                            }))
                            .collect();
                        if Some(positions) != oracle_resp.positions {
                            panic_repro(i, &doc_json, &step_json, "StepMap position or deletion flag mismatch");
                        }
                        let inverse = step.invert(&doc);
                        if Some(serde_json::to_value(&inverse).unwrap()) != oracle_resp.inverse {
                            panic_repro(i, &doc_json, &step_json, "Inverse step JSON mismatch");
                        }
                        let undone = match inverse.apply(&rust_doc) {
                            StepResult::Ok(doc) => Some(serde_json::to_value(doc).unwrap()),
                            StepResult::Failed(_) => None,
                        };
                        if undone != oracle_resp.undone {
                            panic_repro(i, &doc_json, &step_json, "Inverse step application mismatch");
                        }
                    }
                    StepResult::Failed(err) => {
                        if oracle_resp.success {
                            panic_repro(
                                i,
                                &doc_json,
                                &step_json,
                                &format!("Rust failed ({err}) but JS Oracle succeeded"),
                            );
                        }
                    }
                }
            }
            Err(e) => {
                panic_repro(i, &doc_json, &step_json, &format!("Oracle failed: {e}"));
            }
        }

        passed += 1;
        if i % 25 == 0 {
            println!("  Fuzzed {}/{} cases successfully", i, iterations);
        }
    }

    println!("[SUCCESS] Differential fuzzing completed: {}/{} tests matched 100%!", passed, iterations);
}

fn panic_repro(iteration: usize, doc: &serde_json::Value, step: &serde_json::Value, reason: &str) {
    let repro_data = serde_json::json!({
        "iteration": iteration,
        "reason": reason,
        "doc": doc,
        "step": step,
    });
    let filename = format!("repro_case_{}.json", iteration);
    let _ = fs::write(&filename, serde_json::to_string_pretty(&repro_data).unwrap());
    panic!("Differential Fuzzing Mismatch at iteration {}! Saved reproducer to {}. Reason: {}", iteration, filename, reason);
}
