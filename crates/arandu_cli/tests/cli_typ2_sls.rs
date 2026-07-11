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

/// Multi-file inferred generic `rt.spawn` / `rt.join` (no explicit type args).
#[test]
fn run_import_inferred_spawn_join() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_import_infer_spawn.aru");
    fs::write(
        &file,
        r#"
module tests.cli.import_infer_spawn
import std.runtime as rt

async func answer(): int {
    return 42
}

func main(): int {
    let ex = rt.new_sync_executor()
    let h = rt.spawn(ex, answer())
    return rt.join(ex, h)
}
"#,
    )
    .unwrap();
    let root = workspace_root();
    let out = run_cli_in(&root, &["run", file.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(42),
        "import infer spawn: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Same-module inferred join (no type args on join_g).
#[test]
fn run_local_inferred_join() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_local_infer_join.aru");
    fs::write(
        &file,
        r#"
module tests.cli.local_infer_join

extern "C" {
    func ar_rt_spawn_i64(state: ptr[u8]): int
    func ar_rt_join_i64(handle: int): int
}

struct SyncExecutor { flags: int }
struct TaskHandle { id: int }

func spawn_g<T>(shared ex: SyncExecutor, job: Coroutine<T>): TaskHandle {
    unsafe {
        let id = ar_rt_spawn_i64(job as ptr[u8])
        return TaskHandle { id: id }
    }
}

func join_g<T>(shared ex: SyncExecutor, handle: TaskHandle): T {
    unsafe {
        let v = ar_rt_join_i64(handle.id)
        return v as T
    }
}

async func answer(): int {
    return 42
}

func main(): int {
    let ex = SyncExecutor { flags: 0 }
    let h = spawn_g(ex, answer())
    return join_g(ex, h)
}
"#,
    )
    .unwrap();
    let root = workspace_root();
    let out = run_cli_in(&root, &["run", file.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(42),
        "local infer join: {}",
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

/// Same-module generic spawn/join with explicit type args (mono specialization).
#[test]
fn run_generic_spawn_join_explicit() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_generic_spawn.aru");
    fs::write(
        &file,
        r#"
module tests.cli.generic_spawn

extern "C" {
    func ar_rt_spawn_i64(state: ptr[u8]): int
    func ar_rt_join_i64(handle: int): int
}

struct SyncExecutor { flags: int }
struct TaskHandle { id: int }

func spawn_g<T>(shared ex: SyncExecutor, job: Coroutine<T>): TaskHandle {
    unsafe {
        let id = ar_rt_spawn_i64(job as ptr[u8])
        return TaskHandle { id: id }
    }
}

func join_g<T>(shared ex: SyncExecutor, handle: TaskHandle): T {
    unsafe {
        let v = ar_rt_join_i64(handle.id)
        return v as T
    }
}

async func answer(): int {
    return 42
}

func main(): int {
    let ex = SyncExecutor { flags: 0 }
    let h = spawn_g(ex, answer())
    return join_g<int>(ex, h)
}
"#,
    )
    .unwrap();
    let root = workspace_root();
    let out = run_cli_in(&root, &["run", file.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(42),
        "generic spawn: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn run_waker_wake_wait() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_waker.aru");
    fs::write(
        &file,
        r#"
module tests.cli.waker
import std.runtime as rt

func main(): int {
    let w = rt.new_waker()
    rt.waker_wake(w)
    let rc = rt.waker_wait(w, 100)
    rt.destroy_waker(w)
    if rc != 1 {
        return 1
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
        "waker: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn run_reactor_backend_linux() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_backend.aru");
    fs::write(
        &file,
        r#"
module tests.cli.backend
import std.runtime as rt

func main(): int {
    let b = rt.reactor_backend()
    // Linux: 1 = epoll, 2 = io_uring
    if b < 1 {
        return 1
    }
    if b > 2 {
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
        "backend: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn run_tcp_async_wait_wake() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_tcp_async.aru");
    fs::write(
        &file,
        r#"
module tests.cli.tcp_async
import std.runtime as rt

func main(): int {
    let lis = rt.tcp_listen(18770)
    if lis.id < 0 {
        return 1
    }
    let client = rt.tcp_connect(18770)
    if client.id < 0 {
        return 2
    }
    let server = rt.tcp_accept(lis)
    if server.id < 0 {
        return 3
    }
    let nb = rt.tcp_set_nonblocking(server, 1)
    if nb != 0 {
        return 4
    }
    let w = rt.new_waker()
    // Timeout with no data
    let t0 = rt.tcp_wait_wake(server, rt.tcp_wait_read_flag(), 5, w)
    if t0 != 0 {
        return 5
    }
    // Write then wait
    // Use write_async (io_uring when available)
    // We cannot easily pass string buffers without alloc; skip payload e2e here.
    // Wait writable on client should succeed.
    let wr = rt.tcp_wait(client, rt.tcp_wait_write_flag(), 100)
    if wr < 1 {
        return 6
    }
    rt.destroy_waker(w)
    rt.tcp_close_stream(client)
    rt.tcp_close_stream(server)
    rt.tcp_close_listener(lis)
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
        "tcp async: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn run_supervisor_true() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_supervisor.aru");
    fs::write(
        &file,
        r#"
module tests.cli.supervisor
import std.runtime as rt

func main(): int {
    let s = rt.new_supervisor()
    if s.id < 0 {
        return 1
    }
    let w = rt.supervisor_spawn(s, "/bin/true", 0)
    if w.id < 0 {
        return 2
    }
    let code = rt.supervisor_wait(s, w)
    rt.destroy_supervisor(s)
    return code
}
"#,
    )
    .unwrap();
    let root = workspace_root();
    let out = run_cli_in(&root, &["run", file.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "supervisor: {}",
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
