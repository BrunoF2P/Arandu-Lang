//! PROMOTE-L6: `Vec<T>` type surface + host-backed int run.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::process::Command;

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_arandu_cli"))
        .current_dir(workspace_root())
        .args(args)
        .output()
        .expect("cli should run")
}

#[test]
fn check_vec_t_with_default_allocator() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_vec_defaults.aru");
    fs::write(
        &file,
        r#"
module tests.cli.vec_defaults

import std.alloc.vec as vec

func main(): int {
    let v: vec.Vec<int> = vec.new<int>()
    return 0
}
"#,
    )
    .expect("write fixture");

    let out = run_cli(&["check", &file.to_string_lossy()]);
    assert!(
        out.status.success(),
        "check Vec<int> defaults failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn run_vec_push_get_destroy_exits_78() {
    let out = run_cli(&["run", "examples/minimal/m13_vec.aru"]);
    assert_eq!(
        out.status.code(),
        Some(78),
        "m13_vec run failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn check_alloc_vec_module_clean() {
    let out = run_cli(&["check", "stdlib/alloc/vec.aru"]);
    assert!(
        out.status.success(),
        "stdlib/alloc/vec.aru must be check-clean for HIR link: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}
