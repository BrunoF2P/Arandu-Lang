use std::fs;
use std::path::Path;

/// Reads a golden fixture as UTF-8 text, rejecting corrupted binary files early.
pub fn read_golden_text(path: &Path) -> String {
    let bytes = fs::read(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    if bytes.is_empty() {
        panic!(
            "golden file is empty (likely corrupted): {}",
            path.display()
        );
    }
    if bytes.contains(&0) {
        panic!(
            "golden file contains null bytes (likely corrupted): {}",
            path.display()
        );
    }
    String::from_utf8(bytes).unwrap_or_else(|err| {
        panic!(
            "golden file is not valid UTF-8 at {}: {err}",
            path.display()
        )
    })
}
