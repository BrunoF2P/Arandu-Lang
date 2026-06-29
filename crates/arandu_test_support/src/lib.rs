use std::fs;
use std::path::PathBuf;

pub mod ui;
pub use ui::assert_diagnostic_golden;

/// Returns the absolute path to the workspace root directory.
#[must_use]
pub fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .expect("crate should be under workspace/crates")
        .to_path_buf()
}

/// Reads a golden fixture source or expected text file.
#[must_use]
pub fn read_golden_text(phase: &str, name: &str, ext: &str) -> String {
    let root = workspace_root();
    let file_path = root.join("tests").join(phase).join(format!("{name}.{ext}"));
    fs::read_to_string(&file_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", file_path.display()))
}

/// Asserts that actual output matches golden file, or updates it if `UPDATE_GOLDEN=1`.
pub fn assert_golden_text(phase: &str, name: &str, ext: &str, actual: &str) {
    let root = workspace_root();
    let file_path = root.join("tests").join(phase).join(format!("{name}.{ext}"));
    let update_golden = std::env::var("UPDATE_GOLDEN").is_ok();

    if update_golden {
        fs::write(&file_path, actual).unwrap_or_else(|err| {
            panic!("failed to write golden file {}: {err}", file_path.display())
        });
    } else {
        assert!(
            file_path.exists(),
            "Golden file missing for {name}. Run with UPDATE_GOLDEN=1 to create it."
        );
        let expected = fs::read_to_string(&file_path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", file_path.display()));
        let actual_norm = actual.replace("\r\n", "\n");
        let expected_norm = expected.replace("\r\n", "\n");
        assert_eq!(
            actual_norm.trim(),
            expected_norm.trim(),
            "Golden output mismatch for {name}. Run with UPDATE_GOLDEN=1 to update."
        );
    }
}
