#![cfg(not(target_arch = "wasm32"))]

// Reference PNGs were captured on Ubuntu in GitHub Actions run 44564352590.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;

struct TemporaryImage(PathBuf);

impl Drop for TemporaryImage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn read_rgb(path: &Path) -> ((u32, u32), Vec<u8>) {
    let file = File::open(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let mut png = png::Decoder::new(BufReader::new(file)).read_info().unwrap();
    let mut bytes = vec![0; png.output_buffer_size().unwrap()];
    let frame = png.next_frame(&mut bytes).unwrap();
    assert_eq!(frame.color_type, png::ColorType::Rgb);
    assert_eq!(frame.bit_depth, png::BitDepth::Eight);
    bytes.truncate(frame.buffer_size());
    ((frame.width, frame.height), bytes)
}

fn compare(name: &str, scenario: Option<&str>) {
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");
    let output = TemporaryImage(
        std::env::temp_dir().join(format!("prosemirror-{name}-{}.png", std::process::id())),
    );
    let mut command = Command::new(env!("CARGO_BIN_EXE_editor-app"));
    if let Some(scenario) = scenario {
        command
            .arg("--replay")
            .arg(fixture_root.join(format!("{scenario}.json")))
            .arg("--png");
    } else {
        command.arg("--snapshot");
    }
    let result = command.arg(&output.0).output().unwrap();
    assert!(
        result.status.success(),
        "rendering {name}: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let (expected_size, expected) = read_rgb(&fixture_root.join("golden").join(format!("{name}.png")));
    let (actual_size, actual) = read_rgb(&output.0);
    assert_eq!(actual_size, expected_size, "{name} dimensions");
    assert_eq!(actual.len(), expected.len(), "{name} decoded byte count");
    let mut differences = actual
        .as_chunks::<3>()
        .0
        .iter()
        .zip(expected.as_chunks::<3>().0)
        .enumerate()
        .filter(|(_, (actual, expected))| actual != expected);
    if let Some((index, (actual, expected))) = differences.next() {
        let width = actual_size.0 as usize;
        panic!(
            "{name}: {} pixel(s) differ, first at ({},{}): {actual:?}, expected {expected:?}",
            differences.count() + 1,
            index % width,
            index / width
        );
    }
}

#[test]
fn renders_the_sample_document_at_reference_pixels() {
    compare("editor", None);
}

#[test]
fn renders_typing_at_reference_pixels() {
    compare("typing", Some("typing"));
}

#[test]
fn renders_selection_at_reference_pixels() {
    compare("selection", Some("selection"));
}

#[test]
fn renders_block_drag_at_reference_pixels() {
    compare("block_drag", Some("block_drag"));
}
