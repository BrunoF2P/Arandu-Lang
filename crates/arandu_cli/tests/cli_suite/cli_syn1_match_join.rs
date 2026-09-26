//! Residual CFG fix (diverging match/if) + SYN.1 implicit tail return.
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
fn match_stmt_all_arms_return_exits_0() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_match_stmt_ret.aru");
    fs::write(
        &file,
        r#"
module tests.cli.match_stmt_ret

func main(): int {
    let x: Option<int> = nil
    match x {
        Some(v) => { return v }
        None => { return 0 }
    }
}
"#,
    )
    .expect("write");
    let path = file.to_string_lossy();
    let run = run_cli(&["run", &path]);
    assert_eq!(
        run.status.code(),
        Some(0),
        "match stmt return arms, stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn if_both_arms_return_exits_1() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_if_both_ret.aru");
    fs::write(
        &file,
        r#"
module tests.cli.if_both_ret

func main(): int {
    if true {
        return 1
    } else {
        return 2
    }
}
"#,
    )
    .expect("write");
    let path = file.to_string_lossy();
    let run = run_cli(&["run", &path]);
    assert_eq!(
        run.status.code(),
        Some(1),
        "if both return, stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn implicit_tail_return_exits_44() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_syn1.aru");
    fs::write(
        &file,
        r#"
module tests.cli.syn1

func answer(): int {
    42
}

func add(a: int, b: int): int {
    let s = a + b
    s
}

func main(): int {
    answer() + add(1, 1)
}
"#,
    )
    .expect("write");
    let path = file.to_string_lossy();
    let run = run_cli(&["run", &path]);
    assert_eq!(
        run.status.code(),
        Some(44),
        "SYN.1 implicit return, stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
}
