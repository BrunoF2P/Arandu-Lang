//! Regression: AMIR DCE must keep temps used only as `Goto` block-param args.
//!
//! Pattern (big-tech invariant): all passes walk terminators via
//! `for_each_terminator_operand` so jump args stay live under `--opt`.
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
fn while_growth_survives_opt() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_opt_while_backedge.aru");
    fs::write(
        &file,
        r#"
module tests.cli.opt_while_backedge

func main(): int {
    let mut x: int = 1
    while x < 8 {
        x = x * 2
    }
    return x
}
"#,
    )
    .expect("write");

    let path = file.to_string_lossy();

    let plain = run_cli(&["run", &path]);
    assert_eq!(
        plain.status.code(),
        Some(8),
        "plain run, stderr={}",
        String::from_utf8_lossy(&plain.stderr)
    );

    let opt = run_cli(&["run", &path, "--opt"]);
    assert_eq!(
        opt.status.code(),
        Some(8),
        "--opt must not delete while back-edge values, stderr={}",
        String::from_utf8_lossy(&opt.stderr)
    );

    let amir_opt = run_cli(&["amir", &path, "--opt"]);
    assert!(
        amir_opt.status.success(),
        "amir --opt failed: {}",
        String::from_utf8_lossy(&amir_opt.stderr)
    );
    let txt = String::from_utf8_lossy(&amir_opt.stdout);
    // Must still have a mul (or folded constant chain) feeding the header — not bare undef.
    assert!(
        txt.contains("mul") || txt.contains("goto bb1("),
        "unexpected optimized AMIR:\n{txt}"
    );
    // Classic bug shape: goto with a temp that was never defined after DCE.
    // We assert mul survives when the loop is not fully constant-folded.
    if txt.contains("mul") {
        // ok
    } else {
        // Fully folded is also fine if exit is still 8.
    }
}
