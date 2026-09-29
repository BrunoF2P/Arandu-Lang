//! Regression: `--opt` must not merge across `Suspend` (await frontiers).
//!
//! simplify_cfg used to treat Suspend→resume as single-pred/single-succ merge
//! fodder, dropping resume block params and panicking in the Cranelift translator.
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
fn multi_await_survives_opt() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_opt_await_suspend.aru");
    fs::write(
        &file,
        r#"
module tests.cli.opt_await_suspend

async func step(x: int): int {
    return x + 1
}

async func chain(): int {
    let a = await step(1)
    let b = await step(a)
    return b
}

func main(): int {
    return await chain()
}
"#,
    )
    .expect("write");

    let path = file.to_string_lossy();

    let plain = run_cli(&["run", &path]);
    assert_eq!(
        plain.status.code(),
        Some(3),
        "plain multi-await, stderr={}",
        String::from_utf8_lossy(&plain.stderr)
    );

    let opt = run_cli(&["run", &path, "--opt"]);
    assert_eq!(
        opt.status.code(),
        Some(3),
        "--opt multi-await must not ICE, stderr={}",
        String::from_utf8_lossy(&opt.stderr)
    );

    let amir_opt = run_cli(&["amir", &path, "--opt"]);
    assert!(amir_opt.status.success());
    let txt = String::from_utf8_lossy(&amir_opt.stdout);
    assert!(
        txt.contains("suspend"),
        "Suspend frontier must remain after --opt:\n{txt}"
    );
}
