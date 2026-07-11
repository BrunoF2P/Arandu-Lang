//! SYN.3: `nil` as Option.None + match Some/None end-to-end.
//! SL_S thin: `import std.path` typechecks via stdlib/std rewrite.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::process::Command;

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_arandu_cli"))
        .args(args)
        .output()
        .expect("cli should run")
}

fn run_cli_in(cwd: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_arandu_cli"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("cli should run")
}

#[test]
fn run_option_nil_and_some_exits_42() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_option_nil.aru");
    fs::write(
        &file,
        r#"
module tests.cli.option_nil

func none_int(): Option<int> {
    return nil
}

func some_int(): Option<int> {
    return Option.Some(42)
}

func main(): int {
    let a = none_int()
    let n = match a {
        Some(v) => v
        None => 0
    }
    if n != 0 {
        return n
    }
    let b = some_int()
    return match b {
        Some(v) => v
        None => 0
    }
}
"#,
    )
    .expect("write");

    let path = file.to_string_lossy();
    let run = run_cli(&["run", &path]);
    assert_eq!(
        run.status.code(),
        Some(42),
        "option nil/some run exit, stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn check_std_path_import_sls() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_sls_path.aru");
    fs::write(
        &file,
        r#"
module tests.cli.sls_path

import std.path as path

func main(): int {
    let _ = path.is_absolute("/tmp")
    return 0
}
"#,
    )
    .expect("write");

    // Walk-up from workspace root finds stdlib/std/path.aru.
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root");
    let path = file.to_string_lossy();
    let check = run_cli_in(&root, &["check", &path]);
    assert!(
        check.status.success(),
        "SL_S path import check failed: {}",
        String::from_utf8_lossy(&check.stderr)
    );
}
