//! TYP.2 where/bounds + SL_S std.runtime import typechecks.
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
fn where_and_colon_bounds_check_ok() {
    let root = workspace_root();
    let file = root.join("tests/ui/type_checker/where_ok.aru");
    let out = run_cli_in(&root, &["check", file.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "where_ok: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn import_std_runtime_scaffold_checks() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_std_runtime.aru");
    fs::write(
        &file,
        r#"
module tests.cli.std_runtime
import std.runtime as rt
func main(): int {
    let ex = rt.new_sync_executor()
    return ex.flags
}
"#,
    )
    .unwrap();
    let root = workspace_root();
    let out = run_cli_in(&root, &["check", file.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "std.runtime: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn run_path_absolute_and_empty() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_path_abs.aru");
    fs::write(
        &file,
        r#"
module tests.cli.path_abs
import std.path as path
func main(): int {
    if !path.is_empty("") {
        return 1
    }
    if !path.is_absolute("/tmp") {
        return 2
    }
    if path.is_absolute("rel") {
        return 3
    }
    return 0
}
"#,
    )
    .unwrap();
    let root = workspace_root();
    let out = run_cli_in(&root, &["run", file.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "path abs: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn run_sync_executor_new() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_sync_ex.aru");
    fs::write(
        &file,
        r#"
module tests.cli.sync_ex
import std.runtime as rt
func main(): int {
    let ex = rt.new_sync_executor()
    return ex.flags
}
"#,
    )
    .unwrap();
    let root = workspace_root();
    let out = run_cli_in(&root, &["run", file.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "sync ex: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// SL_R.0 end-to-end: multi-file `std.runtime` bodies + host `ar_rt_spawn/join`
/// driving a Ready coroutine blob (`ar_co_make_ready_i64`).
///
/// Uses statement-form `unsafe { … }` (AMIR supports that path; expr-form U001).
#[test]
fn run_sync_executor_spawn_join_ready() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_slr_spawn_join.aru");
    fs::write(
        &file,
        r#"
module tests.cli.slr_spawn_join
import std.runtime as rt

extern "C" {
    func ar_co_make_ready_i64(payload: int): ptr[u8]
}

func make_ready(payload: int): ptr[u8] {
    unsafe {
        return ar_co_make_ready_i64(payload)
    }
}

func main(): int {
    let ex = rt.new_sync_executor()
    let state = make_ready(42)
    let h = rt.spawn_i64(ex, state)
    return rt.join_i64(ex, h)
}
"#,
    )
    .unwrap();
    let root = workspace_root();
    let out = run_cli_in(&root, &["run", file.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(42),
        "slr spawn/join: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Typed spawn/join over A3 `async func` → `Coroutine<int>`.
#[test]
fn run_typed_spawn_int_async_func() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_typed_spawn.aru");
    fs::write(
        &file,
        r#"
module tests.cli.typed_spawn
import std.runtime as rt

async func answer(): int {
    return 42
}

func main(): int {
    let ex = rt.new_sync_executor()
    let h = rt.spawn_int(ex, answer())
    return rt.join_int(ex, h)
}
"#,
    )
    .unwrap();
    let root = workspace_root();
    let out = run_cli_in(&root, &["run", file.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(42),
        "typed spawn: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Typed block_on over async func (no spawn).
#[test]
fn run_typed_block_on_int() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_typed_block_on.aru");
    fs::write(
        &file,
        r#"
module tests.cli.typed_block_on
import std.runtime as rt

async func answer(): int {
    return 7
}

func main(): int {
    let ex = rt.new_sync_executor()
    return rt.block_on_int(ex, answer())
}
"#,
    )
    .unwrap();
    let root = workspace_root();
    let out = run_cli_in(&root, &["run", file.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(7),
        "typed block_on: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// SL_R.2: EpollReactor sleep_ms returns success.
#[test]
fn run_reactor_sleep_ms() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_reactor_sleep.aru");
    fs::write(
        &file,
        r#"
module tests.cli.reactor_sleep
import std.runtime as rt

func main(): int {
    let r = rt.new_epoll_reactor()
    if r.id < 0 {
        return 1
    }
    let rc = rt.reactor_sleep_ms(r, 5)
    rt.destroy_reactor(r)
    if rc != 0 {
        return 2
    }
    return 0
}
"#,
    )
    .unwrap();
    let root = workspace_root();
    let out = run_cli_in(&root, &["run", file.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "reactor sleep: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// SL_R.2 + SL_R.0: arm timer, poll, and join a spawned coroutine.
#[test]
fn run_reactor_arm_poll_with_spawn() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_reactor_spawn.aru");
    fs::write(
        &file,
        r#"
module tests.cli.reactor_spawn
import std.runtime as rt

async func ready(): int {
    return 99
}

func main(): int {
    let r = rt.new_epoll_reactor()
    let ex = rt.new_sync_executor()
    if r.id < 0 {
        return 1
    }
    let h = rt.spawn_int(ex, ready())
    let arm = rt.reactor_arm_timer_ms(r, 5)
    if arm != 0 {
        return 2
    }
    let fired = rt.reactor_poll_ms(r, 200)
    if fired != 1 {
        return 3
    }
    let v = rt.join_int(ex, h)
    rt.destroy_reactor(r)
    return v
}
"#,
    )
    .unwrap();
    let root = workspace_root();
    let out = run_cli_in(&root, &["run", file.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(99),
        "reactor+spawn: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
