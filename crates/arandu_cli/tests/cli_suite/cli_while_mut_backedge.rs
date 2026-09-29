//! Regression: while header multi-phi / back-edge args must not duplicate.
//!
//! Free-func `mut` params used after a `while` that also mutates another local
//! used to miscompile: body back-edge filled `goto header(newCap, newCap)`
//! instead of `(newCap, v)`, corrupting the mut-ref base (SEGV on field load).
//! Root cause: `build_target_args` pre-filled unsealed targets, then seal
//! appended again (Braun incomplete-phi fill).
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
fn while_mut_field_store_and_load_after_loop() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_while_mut_backedge.aru");
    fs::write(
        &file,
        r#"
module tests.cli.while_mut_backedge

struct V {
    data: ptr[int]
    capacity: u64
}

func grow(mut v: V, minCap: u64): void {
    let mut newCap: u64 = v.capacity
    if newCap == 0 {
        newCap = 8 as u64
    }
    while newCap < minCap {
        newCap = newCap * 2
    }
    v.capacity = newCap
    // Field load of mut param after while (residual SEGV path).
    let d = v.data
    let nullp: ptr[int] = nil
    if d == nullp {
        return
    }
}

func main(): int {
    let mut v = V { data: nil, capacity: 1 as u64 }
    grow(v, 20 as u64)
    // 1→2→4→8→16→32
    return v.capacity as int
}
"#,
    )
    .expect("write");

    let path = file.to_string_lossy();

    let amir = run_cli(&["amir", &path]);
    assert!(
        amir.status.success(),
        "amir dump failed: {}",
        String::from_utf8_lossy(&amir.stderr)
    );
    let amir_txt = String::from_utf8_lossy(&amir.stdout);
    // Body back-edge must not pass the same operand twice (old bug: newCap for v).
    let dup_backedge = amir_txt.lines().any(|line| {
        let t = line.trim();
        if !t.starts_with("goto bb") {
            return false;
        }
        let Some(args) = t.split_once('(').and_then(|(_, r)| r.strip_suffix(')')) else {
            return false;
        };
        let parts: Vec<_> = args.split(',').map(str::trim).collect();
        parts.len() >= 2 && parts.windows(2).any(|w| w[0] == w[1] && !w[0].is_empty())
    });
    assert!(
        !dup_backedge,
        "duplicate back-edge terminator args still present:\n{amir_txt}"
    );

    let run = run_cli(&["run", &path]);
    assert_eq!(
        run.status.code(),
        Some(32),
        "while+mut residual run exit, stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );

    // `--opt` must keep values that only appear as Goto jump args (DCE visitor).
    let run_opt = run_cli(&["run", &path, "--opt"]);
    assert_eq!(
        run_opt.status.code(),
        Some(32),
        "while+mut under --opt (DCE jump-args), stderr={}",
        String::from_utf8_lossy(&run_opt.stderr)
    );
}
