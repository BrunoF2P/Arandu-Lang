//! RFC 0021 stdlib prelude facade tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::process::Command;

fn run_cli_in(cwd: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_arandu_cli"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("cli")
}

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn prelude_public_use_facade_check_ok() {
    let dir = std::env::temp_dir();
    let file = dir.join(format!("arandu_rfc0021_prelude_{}.aru", std::process::id()));
    fs::write(
        &file,
        r#"
module tests.rfc0021.prelude

from std.core.prelude import { Iterator, PhantomData, phantom, Cell, UnsafeCell, cellNew, Copy, Send, Sync }

func test_cell(): int {
    let c = cellNew(42)
    return c.value
}

func test_phantom(): PhantomData<int> {
    return phantom<int>()
}

func test_bounds<T: Copy + Send + Sync>(x: T): T {
    return x
}

func main(): int {
    let p = test_phantom()
    let b = test_bounds(10)
    return test_cell() - 42
}
"#,
    )
    .unwrap();

    let root = workspace_root();
    let out = run_cli_in(&root, &["check", file.to_str().unwrap()]);
    let _ = fs::remove_file(&file);

    assert!(
        out.status.success(),
        "prelude facade check failed: status: {:?}, stdout: {}, stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn prelude_facade_program_runs_successfully() {
    let dir = std::env::temp_dir();
    let file = dir.join(format!("arandu_rfc0021_run_{}.aru", std::process::id()));
    fs::write(
        &file,
        r#"
module tests.rfc0021.run

from std.core.prelude import { cellNew }

func main(): int {
    let c = cellNew(84)
    if c.value / 2 == 42 {
        return 0
    }
    return 1
}
"#,
    )
    .unwrap();

    let root = workspace_root();
    let out = run_cli_in(&root, &["run", file.to_str().unwrap()]);
    let _ = fs::remove_file(&file);

    assert!(
        out.status.success(),
        "prelude run failed: status: {:?}, stdout: {}, stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
