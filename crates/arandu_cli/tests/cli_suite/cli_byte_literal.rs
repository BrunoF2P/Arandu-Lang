//! Integration test for byte character literals (`b'/'`, `b'\n'`, `b'a'`).
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
fn byte_literals_execution_and_patterns() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_byte_literal.aru");
    fs::write(
        &file,
        r#"
module tests.cli.byteliteral

func is_slash(b: u8): bool {
    return b == b'/'
}

func byte_to_int(b: u8): int {
    match b {
        b'/' => {
            return 1
        }
        b'*' => {
            return 2
        }
        b'\n' => {
            return 3
        }
        _ => {
            return 0
        }
    }
}

func main(): int {
    let slash: u8 = b'/'
    if !is_slash(slash) {
        return 10
    }
    if is_slash(b'*') {
        return 11
    }
    let val1 = byte_to_int(b'/')
    let val2 = byte_to_int(b'*')
    let val3 = byte_to_int(b'\n')
    let val4 = byte_to_int(b'z')
    if val1 != 1 || val2 != 2 || val3 != 3 || val4 != 0 {
        return 12
    }
    let b: byte = b'A'
    if b != 65 as u8 {
        return 13
    }
    return 42
}
"#,
    )
    .expect("write");

    let path = file.to_string_lossy();
    let check = run_cli(&["check", &path]);
    assert!(
        check.status.success(),
        "byte literal check failed: {}",
        String::from_utf8_lossy(&check.stderr)
    );
    let run = run_cli(&["run", &path]);
    assert_eq!(
        run.status.code(),
        Some(42),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}
