//! TYP.2 where/bounds + SL_S std.runtime import typechecks.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::process::Command;

fn run_cli_in(cwd: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_arandu_cli"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("cli")
}

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn where_and_colon_bounds_check_ok() {
    let root = workspace_root();
    let file = root.join("tests/ui/type_checker/where_ok.aru");
    let out = run_cli_in(&root, &["check", file.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "where_ok: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn import_std_runtime_scaffold_checks() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_std_runtime.aru");
    fs::write(
        &file,
        r#"
module tests.cli.std_runtime
import std.runtime as rt
func main(): int {
    let _ = rt.noop_executor_hint()
    return 0
}
"#,
    )
    .unwrap();
    let root = workspace_root();
    let out = run_cli_in(&root, &["check", file.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "std.runtime: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
