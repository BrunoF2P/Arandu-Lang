//! W3.3: auto-ref — `takes_ref(x)` when formal is `&int` and actual is `int`.
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
fn auto_ref_call_arg_check_and_run() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_autoref.aru");
    fs::write(
        &file,
        r#"
module tests.cli.autoref

func takes_ref(p: &int): int {
    return *p
}

func main(): int {
    let x: int = 41
    return takes_ref(x)
}
"#,
    )
    .expect("write");

    let path = file.to_string_lossy();
    let check = run_cli(&["check", &path]);
    assert!(
        check.status.success(),
        "auto-ref check failed: {}",
        String::from_utf8_lossy(&check.stderr)
    );
    let run = run_cli(&["run", &path]);
    assert_eq!(
        run.status.code(),
        Some(41),
        "auto-ref run exit, stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
}
