//! W2: CLI smoke for `Vec<T>` with default allocator (T2.1 consumed in stdlib).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::process::Command;

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_arandu_cli"))
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
