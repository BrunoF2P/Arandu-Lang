//! SYN.2 string interpolation + SYN.4 advanced patterns (ranges, `_`, or).
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
fn syn2_dollar_name_and_brace_interp_exits_42() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_syn2.aru");
    fs::write(
        &file,
        r#"
module tests.cli.syn2

import io

func main(): int {
    let name = "Ada"
    let n = 41
    let s = "hi $name count=${n + 1}"
    io.println(s)
    return n + 1
}
"#,
    )
    .expect("write");
    let path = file.to_string_lossy();
    let run = run_cli(&["run", &path]);
    assert_eq!(
        run.status.code(),
        Some(42),
        "SYN.2 run, stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("hi Ada count=42"),
        "expected interp stdout, got: {stdout}"
    );
}

#[test]
fn syn4_range_wildcard_or_exits_30() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_syn4.aru");
    fs::write(
        &file,
        r#"
module tests.cli.syn4

func f(x: int): int {
    return match x {
        1 | 2 | 3 => 10
        4..=6 => 20
        _ => 0
    }
}

func main(): int {
    return f(2) + f(5) + f(9)
}
"#,
    )
    .expect("write");
    let path = file.to_string_lossy();
    let run = run_cli(&["run", &path]);
    assert_eq!(
        run.status.code(),
        Some(30),
        "SYN.4 or/range/wild, stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
}
