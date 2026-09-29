//! Regression coverage for the ownership holes reported by the drop/borrow audit.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::process::Command;

fn check(source: &str, name: &str) -> String {
    let file = std::env::temp_dir().join(format!(
        "arandu_ownership_{name}_{}.aru",
        std::process::id()
    ));
    fs::write(&file, source).expect("write regression source");
    let output = Command::new(env!("CARGO_BIN_EXE_arandu_cli"))
        .args(["check", file.to_str().expect("UTF-8 temp path")])
        .output()
        .expect("run compiler");
    let _ = fs::remove_file(&file);
    assert!(
        !output.status.success(),
        "unsafe ownership pattern compiled"
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn check_passes(source: &str, name: &str) {
    let file = std::env::temp_dir().join(format!(
        "arandu_ownership_{name}_{}.aru",
        std::process::id()
    ));
    fs::write(&file, source).expect("write regression source");
    let output = Command::new(env!("CARGO_BIN_EXE_arandu_cli"))
        .args(["check", file.to_str().expect("UTF-8 temp path")])
        .output()
        .expect("run compiler");
    let _ = fs::remove_file(&file);
    assert!(
        output.status.success(),
        "expected ownership pattern to compile, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run(source: &str, name: &str) -> String {
    let file = std::env::temp_dir().join(format!(
        "arandu_ownership_{name}_{}.aru",
        std::process::id()
    ));
    fs::write(&file, source).expect("write regression source");
    let output = Command::new(env!("CARGO_BIN_EXE_arandu_cli"))
        .args(["run", file.to_str().expect("UTF-8 temp path")])
        .output()
        .expect("run compiler");
    let _ = fs::remove_file(&file);
    assert!(
        output.status.success(),
        "expected program to execute without memory failure (status {}), stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn emit_c(source: &str, name: &str) -> String {
    let file = std::env::temp_dir().join(format!(
        "arandu_ownership_{name}_{}.aru",
        std::process::id()
    ));
    fs::write(&file, source).expect("write regression source");
    let output = Command::new(env!("CARGO_BIN_EXE_arandu_cli"))
        .args(["emit-c", file.to_str().expect("UTF-8 temp path")])
        .output()
        .expect("emit C");
    let _ = fs::remove_file(&file);
    assert!(
        output.status.success(),
        "C emission failed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn run_emitted_c_with_asan(c_source: &str, name: &str) {
    if !Command::new("clang")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }
    let c_file =
        std::env::temp_dir().join(format!("arandu_ownership_{name}_{}.c", std::process::id()));
    let binary =
        std::env::temp_dir().join(format!("arandu_ownership_{name}_{}", std::process::id()));
    fs::write(&c_file, c_source).expect("write emitted C");
    let output = Command::new("clang")
        .args(["-fsanitize=address", "-fno-omit-frame-pointer"])
        .arg(&c_file)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("compile emitted C with AddressSanitizer");
    assert!(
        output.status.success(),
        "emitted C must compile: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // LeakSanitizer is not available in the macOS AddressSanitizer runtime.
    // Keep ASan's UAF/double-free checks enabled there; Linux additionally
    // checks leaks, which is the platform supported by this test suite's LSan.
    let detect_leaks = if cfg!(target_os = "linux") {
        "detect_leaks=1"
    } else {
        "detect_leaks=0"
    };
    let mut command = Command::new(&binary);
    command.env("ASAN_OPTIONS", detect_leaks);
    if cfg!(windows) {
        add_clang_asan_runtime_to_path(&mut command);
    }
    let execution = command
        .output()
        .expect("run emitted C with AddressSanitizer");
    assert!(
        execution.status.success(),
        "emitted C must clean up all owned values (status {}), stdout: {}; stderr: {}",
        execution.status,
        String::from_utf8_lossy(&execution.stdout),
        String::from_utf8_lossy(&execution.stderr)
    );
    let _ = fs::remove_file(binary);
    let _ = fs::remove_file(c_file);
}

fn add_clang_asan_runtime_to_path(command: &mut Command) {
    // Clang's Windows ASan runtime DLL is under its resource directory, which
    // is not necessarily on PATH even when clang.exe itself is.
    let Ok(output) = Command::new("clang").arg("-print-resource-dir").output() else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let resource_dir = String::from_utf8_lossy(&output.stdout);
    let runtime_dir = std::path::PathBuf::from(resource_dir.trim())
        .join("lib")
        .join("windows");
    if !runtime_dir.is_dir() {
        return;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::split_paths(&path)
        .chain(std::iter::once(runtime_dir))
        .collect::<Vec<_>>();
    let Ok(path) = std::env::join_paths(path) else {
        return;
    };
    command.env("PATH", path);
}

#[test]
fn moving_owned_field_through_shared_reference_is_rejected() {
    let stderr = check(
        r#"
module tests.cli.ownership.borrowed_field
import std.alloc.string as strings
struct FileItem { path: strings.String }
func steal(item: ref FileItem): strings.String { return item.path }
func main(): int { return 0 }
"#,
        "borrowed_field",
    );
    assert!(stderr.contains("O002"), "expected O002, got: {stderr}");
}

#[test]
fn struct_pattern_cannot_bind_owned_field_from_shared_reference() {
    let stderr = check(
        r#"
module tests.cli.ownership.borrowed_pattern
import std.alloc.string as strings
struct FileItem { path: strings.String }
func steal(item: ref FileItem): strings.String {
    match item {
        FileItem { path } => { return path }
    }
}
func main(): int { return 0 }
"#,
        "borrowed_pattern",
    );
    assert!(stderr.contains("O002"), "expected O002, got: {stderr}");
}

#[test]
fn moving_owned_index_through_shared_reference_is_rejected() {
    let stderr = check(
        r#"
module tests.cli.ownership.borrowed_index
import std.alloc.string as strings
import std.alloc.vec as vec
struct Holder { items: vec.Vec<strings.String> }
func first(item: ref Holder): strings.String { return item.items[0] }
func main(): int { return 0 }
"#,
        "borrowed_index",
    );
    assert!(
        stderr.contains("O002") || stderr.contains("T025"),
        "expected a move/Copy rejection, got: {stderr}"
    );
}

#[test]
fn string_view_keeps_its_owner_borrow_live() {
    let stderr = check(
        r#"
module tests.cli.ownership.string_view
import std.alloc.string as strings
import std.core.io as io
func main(): int {
    let mut owner = strings.from("before")
    let view: str = owner.asStr()
    owner.pushStr("-may-reallocate")
    io.println(view)
    return 0
}
"#,
        "string_view",
    );
    assert!(
        stderr.contains("O003"),
        "expected mutable-borrow conflict, got: {stderr}"
    );
}

#[test]
fn string_view_borrow_ends_after_its_last_use() {
    check_passes(
        r#"
module tests.cli.ownership.string_view_nll
import std.alloc.string as strings
import std.core.io as io
func main(): int {
    let mut owner = strings.from("before")
    let view: str = owner.asStr()
    io.println(view)
    owner.pushStr("-after-last-use")
    return 0
}
"#,
        "string_view_nll",
    );
}

#[test]
fn borrowed_value_cannot_satisfy_owned_function_parameter() {
    let stderr = check(
        r#"
module tests.cli.ownership.own_argument
import std.alloc.string as strings
struct Resource { value: strings.String }
func consume(resource: own Resource): int { return 1 }
func main(): int {
    let resource = Resource { value: strings.new() }
    return consume(&resource)
}
"#,
        "own_argument",
    );
    assert!(
        stderr.contains("T003"),
        "expected incompatible argument, got: {stderr}"
    );
}

#[test]
fn moved_noncopy_field_cannot_be_destroyed_again() {
    let stderr = check(
        r#"
module tests.cli.ownership.field_move
import std.alloc.string as strings
struct Pair { first: strings.String, second: strings.String }
func main(): int {
    let mut pair = Pair { first: strings.new(), second: strings.new() }
    let moved = pair.first
    moved.destroy()
    pair.first.destroy()
    return 0
}

"#,
        "field_move",
    );
    assert!(
        stderr.contains("O001"),
        "expected use-after-move, got: {stderr}"
    );
}

#[test]
fn option_payload_pattern_drops_only_the_active_payload() {
    let stdout = run(
        r#"
module tests.cli.ownership.option_payload_drop
import std.alloc.string as strings
import std.core.io as io
func classify(value: Option<strings.String>): int {
    match value {
        Some(payload) => {
            io.println(payload.asStr())
            if payload.len() > 2 { return 7 }
            return 1
        }
        None => { return 0 }
    }
}

func main(): int {
    let mut index = 0
    while index < 4 {
        if index % 2 == 0 {
            io.println(classify(Option.Some(strings.from("payload-abc"))))
        } else {
            io.println(classify(Option.None))
        }
        index = index + 1
    }
    return 0
}
"#,
        "option_payload_drop",
    );
    assert!(stdout.contains("payload-abc"));
    assert!(stdout.lines().filter(|line| *line == "7").count() >= 2);
    assert!(stdout.lines().filter(|line| *line == "0").count() >= 2);
}

#[test]
fn nested_option_pattern_failure_falls_through_to_next_match_arm() {
    let stdout = run(
        r#"
module tests.cli.ownership.nested_option_pattern_fallback
import std.core.io as io
func classify(value: Option<Option<int>>): int {
    match value {
        Some(Some(7)) => { return 1 }
        Some(_) => { return 2 }
        None => { return 3 }
    }
}
func main(): int {
    io.println(classify(Option.Some(Option.Some(8))))
    io.println(classify(Option.Some(Option.None)))
    io.println(classify(Option.None))
    return 0
}
"#,
        "nested_option_pattern_fallback",
    );
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["2", "2", "3"]);
}

#[test]
fn false_match_guard_keeps_noncopy_scrutinee_available_to_later_arm() {
    let stdout = run(
        r#"
module tests.cli.ownership.match_guard_transaction
import std.alloc.string as strings
import std.core.io as io
func classify(value: Option<strings.String>): int {
    match value {
        Some(payload) if false => { return 1 }
        Some(_) => { return 2 }
        None => { return 3 }
    }
}
func main(): int {
    io.println(classify(Option.Some(strings.from("still-owned"))))
    io.println(classify(Option.None))
    return 0
}
"#,
        "match_guard_transaction",
    );
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["2", "3"]);
}

#[test]
fn nested_match_guard_tests_and_borrows_payload_before_committing_move() {
    let source = r#"
module tests.cli.ownership.nested_match_guard_transaction
import std.alloc.string as strings
import std.core.io as io
func noneString(): Option<strings.String> { return Option.None }
func classify(value: Option<Option<strings.String>>): int {
    match value {
        Some(Some(payload)) if false => { return 0 }
        Some(Some(payload)) if payload.len() > 0 => { return 1 }
        Some(Some(_)) => { return 2 }
        Some(None) => { return 3 }
        None => { return 4 }
    }
}
func main(): int {
    io.println(classify(Option.Some(Option.Some(strings.from("guarded")))))
    io.println(classify(Option.Some(noneString())))
    io.println(classify(Option.None))
    return 0
}
"#;
    let stdout = run(source, "nested_match_guard_transaction");
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["1", "3", "4"]);
    let c_source = emit_c(source, "nested_match_guard_transaction");
    run_emitted_c_with_asan(&c_source, "nested_match_guard_transaction");
}

#[test]
fn nested_custom_enum_pattern_falls_through_after_payload_test_fails() {
    let stdout = run(
        r#"
module tests.cli.ownership.nested_custom_enum_patterns
import std.core.io as io
enum Inner { Number(int, int), End }
enum Outer { Wrapped(Inner), End }
func classify(value: Outer): int {
    return match value {
        Outer.Wrapped(Inner.Number(7, 9)) => 1
        Outer.Wrapped(_) => 2
        Outer.End => 3
    }
}

func main(): int {
    io.println(classify(Outer.Wrapped(Inner.Number(8, 9))))
    io.println(classify(Outer.Wrapped(Inner.Number(7, 8))))
    io.println(classify(Outer.Wrapped(Inner.End)))
    io.println(classify(Outer.End))
    return 0
}
"#,
        "nested_custom_enum_patterns",
    );
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["2", "2", "2", "3"]);
}

#[test]
fn nested_multi_field_enum_guard_keeps_payload_until_fallback_arm() {
    let source = r#"
module tests.cli.ownership.nested_multi_field_enum_guard
import std.alloc.string as strings
import std.core.io as io
enum Inner { Data(strings.String, int), Empty }
enum Outer { Wrapped(Inner), End }
func classify(value: Outer): int {
    return match value {
        Outer.Wrapped(Inner.Data(payload, 9)) if false => 1
        Outer.Wrapped(Inner.Data(payload, _)) => {
            let length = payload.len()
            payload.destroy()
            length as int
        }
        Outer.Wrapped(_) => 3
        Outer.End => 4
    }
}
func main(): int {
    io.println(classify(Outer.Wrapped(Inner.Data(strings.from("retained"), 9))))
    io.println(classify(Outer.Wrapped(Inner.Data(strings.from("different"), 8))))
    io.println(classify(Outer.Wrapped(Inner.Empty)))
    return 0
}
"#;
    let stdout = run(source, "nested_multi_field_enum_guard");
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["8", "9", "3"]);
    let c_source = emit_c(source, "nested_multi_field_enum_guard_c");
    run_emitted_c_with_asan(&c_source, "nested_multi_field_enum_guard_asan");
}

#[test]
fn custom_enum_drop_is_expanded_by_amir_for_active_variant_only() {
    let source = r#"
module tests.cli.ownership.custom_enum_drop_glue
import std.alloc.string as strings
enum Resource { Open(strings.String, strings.String), Closed }
func consume(value: Resource): int { return 0 }
func main(): int {
    let open = Resource.Open(strings.from("first"), strings.from("second"))
    let closed = Resource.Closed
    consume(open)
    consume(closed)
    return 0
}
"#;
    run(source, "custom_enum_drop_glue");
    let c_source = emit_c(source, "custom_enum_drop_glue_c");
    run_emitted_c_with_asan(&c_source, "custom_enum_drop_glue_asan");
}

#[test]
fn result_guard_moves_only_selected_payload_and_drops_other_variant() {
    let source = r#"
module tests.cli.ownership.result_match_guard_transaction
import std.alloc.string as strings
import std.core.io as io
func classify(value: Result<strings.String, strings.String>): int {
    match value {
        Result.Ok(payload) if false => { return 0 }
        Result.Ok(payload) if payload.len() > 0 => { return 1 }
        Result.Err(_) => { return 2 }
    }
}
func main(): int {
    io.println(classify(Result.Ok(strings.from("ok"))))
    io.println(classify(Result.Err(strings.from("err"))))
    return 0
}
"#;
    let stdout = run(source, "result_match_guard_transaction");
    assert_eq!(stdout.lines().collect::<Vec<_>>(), ["1", "2"]);
    let c_source = emit_c(source, "result_match_guard_transaction");
    run_emitted_c_with_asan(&c_source, "result_match_guard_transaction");
}

#[test]
fn result_match_can_move_success_payload_and_inspect_noncopy_error_arm() {
    let source = r#"
module tests.cli.ownership.result_noncopy_error_arm
import std.fs as fs
import std.alloc.string as strings
import std.core.io as io
func main(): int {
    let mut code = 0
    match fs.readToString("missing") {
        Result.Ok(content) => {
            if content.len == 0 { code = 5 }
            content.destroy()
        }
        Result.Err(_) => { code = 6 }
    }
    match Result.Ok(strings.from("owned")) {
        Result.Ok(content) => { content.destroy() }
        Result.Err(_) => { code = 7 }
    }
    io.println(code)
    return 0
}
"#;
    check_passes(source, "result_noncopy_error_arm_check");
    assert_eq!(run(source, "result_noncopy_error_arm_run").trim(), "6");
    let c_source = emit_c(source, "result_noncopy_error_arm_c");
    run_emitted_c_with_asan(&c_source, "result_noncopy_error_arm_asan");
}

#[test]
fn loop_iteration_locals_drop_on_backedge_continue_and_break() {
    let c_source = emit_c(
        r#"
module tests.cli.ownership.loop_scope_drops
import std.alloc.string as strings
func main(): int {
    let mut index = 0
    while index < 4 {
        index = index + 1
        let owned = strings.from("iteration-owned")
        if index == 1 { continue }
        if index == 3 { break }
        owned.destroy()
    }
    return 0
}
"#,
        "loop_scope_drops",
    );
    run_emitted_c_with_asan(&c_source, "loop_scope_drops");
}

#[test]
fn c_drop_in_place_uses_opaque_struct_layout_not_member_access() {
    let c_source = emit_c(
        r#"
module tests.cli.ownership.c_drop_opaque
import std.alloc.string as strings
import std.alloc.vec as vec
struct FileItem { path: strings.String, size: usize }
func main(): int {
    let mut files = vec.new<FileItem>()
    vec.push<FileItem>(files, FileItem { path: strings.from("payload"), size: 7 })
    vec.destroy<FileItem>(files)
    let mut optional = vec.new<Option<strings.String>>()
    vec.push<Option<strings.String>>(optional, Option.Some(strings.from("optional")))
    vec.push<Option<strings.String>>(optional, Option.None)
    vec.destroy<Option<strings.String>>(optional)
    return 0
}
"#,
        "c_drop_opaque",
    );
    run_emitted_c_with_asan(&c_source, "c_drop_opaque");
}

#[test]
fn vec_get_borrows_owned_values_and_blocks_reallocation() {
    let stderr = check(
        r#"
module tests.cli.ownership.vec_get_noncopy
import std.alloc.string as strings
import std.alloc.vec as vec
import std.core.io as io
func main(): int {
    let mut values = vec.new<strings.String>()
    vec.push<strings.String>(values, strings.from("held"))
    let item = vec.get<strings.String>(ref values, 0)
    vec.push<strings.String>(values, strings.from("reallocate"))
    match item {
        Some(value) => { io.println(value.asStr()) }
        None => {}
    }
    return 0
}
"#,
        "vec_get_noncopy",
    );
    assert!(
        stderr.contains("O003"),
        "expected reallocation conflict with live element reference, got: {stderr}"
    );
}

#[test]
fn vec_put_cannot_invalidate_a_live_element_reference() {
    let stderr = check(
        r#"
module tests.cli.ownership.vec_put_live_reference
import std.alloc.string as strings
import std.alloc.vec as vec
import std.core.io as io
func main(): int {
    let mut values = vec.new<strings.String>()
    vec.push<strings.String>(values, strings.from("held"))
    let item = vec.get<strings.String>(ref values, 0)
    let _ = vec.put<strings.String>(values, 0, strings.from("replacement"))
    match item {
        Some(value) => { io.println(value.asStr()) }
        None => {}
    }
    return 0
}
"#,
        "vec_put_live_reference",
    );
    assert!(
        stderr.contains("O003"),
        "expected put to conflict with a live element reference, got: {stderr}"
    );
}

#[test]
fn vec_put_drops_replaced_owned_value_once() {
    let c_source = emit_c(
        r#"
module tests.cli.ownership.vec_put_drop
import std.alloc.string as strings
import std.alloc.vec as vec
func main(): int {
    let mut values = vec.new<strings.String>()
    vec.push<strings.String>(values, strings.from("old"))
    let _ = vec.put<strings.String>(values, 0, strings.from("new"))
    vec.destroy<strings.String>(values)
    return 0
}
"#,
        "vec_put_drop",
    );
    run_emitted_c_with_asan(&c_source, "vec_put_drop");
}

#[test]
fn vec_take_some_moves_noncopy_payload_and_leaves_empty_slot() {
    let source = r#"
module tests.cli.ownership.vec_take_some
import std.alloc.string as strings
import std.alloc.vec as vec
import std.core.io as io
func main(): int {
    let mut values = vec.new<Option<strings.String>>()
    vec.push<Option<strings.String>>(values, Option.Some(strings.from("moved")))
    let taken = vec.takeSome<strings.String>(values, 0)
    match taken {
        Some(value) => { io.println(value.asStr()) }
        None => { return 1 }
    }
    vec.destroy<Option<strings.String>>(values)
    return 0
}

"#;
    let stdout = run(source, "vec_take_some");
    assert_eq!(stdout.trim(), "moved");
    let c_source = emit_c(source, "vec_take_some_c");
    run_emitted_c_with_asan(&c_source, "vec_take_some_asan");
}

#[test]
fn smallvec_moves_owned_payloads_through_inline_spill_clear_and_destroy() {
    let source = r#"
module tests.cli.ownership.smallvec_owned_lifecycle
import std.alloc.smallvec as smallvec
import std.alloc.string as strings
func main(): int {
    let mut values = smallvec.new<strings.String>()
    values.push(strings.from("inline-0"))
    values.push(strings.from("inline-1"))
    values.push(strings.from("inline-2"))
    values.push(strings.from("inline-3"))
    values.push(strings.from("spill-4"))
    values.push(strings.from("spill-5"))
    values.clear()
    values.push(strings.from("again-0"))
    values.push(strings.from("again-1"))
    values.push(strings.from("again-2"))
    values.push(strings.from("again-3"))
    values.push(strings.from("again-4"))
    values.destroy()
    return 0
}

"#;
    run(source, "smallvec_owned_lifecycle");
    let c_source = emit_c(source, "smallvec_owned_lifecycle_c");
    run_emitted_c_with_asan(&c_source, "smallvec_owned_lifecycle_asan");
}

#[test]
fn vec_drops_owned_string_elements_via_drop_in_place() {
    let source = r#"
module tests.cli.ownership.vec_owned_string_drop
import std.alloc.string as strings
import std.alloc.vec as vec
func main(): int {
    let mut values = vec.new<Option<strings.String>>()
    vec.push<Option<strings.String>>(values, Option.Some(strings.from("owned-0")))
    vec.push<Option<strings.String>>(values, Option.Some(strings.from("owned-1")))
    vec.push<Option<strings.String>>(values, Option.None)
    vec.destroy<Option<strings.String>>(values)
    return 0
}
"#;
    run(source, "vec_owned_string_drop");
    let c_source = emit_c(source, "vec_owned_string_drop_c");
    run_emitted_c_with_asan(&c_source, "vec_owned_string_drop_asan");
}

#[test]
fn owned_option_array_elements_are_dropped_by_amir_places() {
    let source = r#"
module tests.cli.ownership.option_array_drops
import std.alloc.string as strings
func main(): int {
    let values = [
        Option.Some(strings.from("first")),
        Option.None,
        Option.Some(strings.from("third")),
        Option.None,
    ]
    return 0
}
"#;
    run(source, "option_array_drops");
    let c_source = emit_c(source, "option_array_drops_c");
    run_emitted_c_with_asan(&c_source, "option_array_drops_asan");
}

#[test]
fn assigning_owned_struct_field_drops_only_the_replaced_field() {
    let source = r#"
module tests.cli.ownership.field_reassignment_drop
import std.alloc.string as strings
struct Holder { value: strings.String, marker: int }
func main(): int {
    let mut holder = Holder { value: strings.from("old"), marker: 7 }
    holder.value = strings.from("new")
    return 0
}
"#;
    check_passes(source, "field_reassignment_drop_check");
    run(source, "field_reassignment_drop_run");
    let c_source = emit_c(source, "field_reassignment_drop_c");
    run_emitted_c_with_asan(&c_source, "field_reassignment_drop_asan");
}

#[test]
fn owned_array_elements_are_dropped_by_amir_element_places() {
    let source = r#"
module tests.cli.ownership.array_element_drops
import std.alloc.string as strings
func main(): int {
    let values = [strings.from("first"), strings.from("second")]
    return 0
}
"#;
    check_passes(source, "array_element_drops_check");
    run(source, "array_element_drops_run");
    let c_source = emit_c(source, "array_element_drops_c");
    run_emitted_c_with_asan(&c_source, "array_element_drops_asan");
}

#[test]
fn noncopy_value_parameter_can_be_moved_into_a_mutable_local() {
    check_passes(
        r#"
module tests.cli.ownership.rebind_noncopy_param
import std.alloc.string as strings
func rebind(value: strings.String): strings.String {
    let mut current = value
    return current
}
func main(): int {
    let result = rebind(strings.from("value"))
    result.destroy()
    return 0
}
"#,
        "rebind_noncopy_param",
    );
}

#[test]
fn smallvec_get_cannot_shallow_copy_owned_values() {
    let stderr = check(
        r#"
module tests.cli.ownership.smallvec_get_noncopy
import std.alloc.string as strings
import std.alloc.smallvec as smallvec
import std.core.io as io
func main(): int {
    let values = smallvec.new<strings.String>()
    let copied = smallvec.get<strings.String>(ref values, 0)
    return 0
}
"#,
        "smallvec_get_noncopy",
    );
    assert!(
        stderr.contains("T025"),
        "expected Copy-bound error, got: {stderr}"
    );
}

#[test]
fn smallvec_push_transfers_noncopy_option_payload_ownership() {
    let source = r#"
module tests.cli.ownership.smallvec_push_noncopy
import std.alloc.string as strings
import std.alloc.smallvec as smallvec
func main(): int {
    let mut values = smallvec.new<strings.String>()
    values.push(strings.from("one"))
    values.push(strings.from("two"))
    values.push(strings.from("three"))
    values.push(strings.from("four"))
    values.push(strings.from("spill"))
    values.destroy()
    return 0
}
"#;
    let c_source = emit_c(source, "smallvec_push_noncopy_c");
    run_emitted_c_with_asan(&c_source, "smallvec_push_noncopy_asan");
}

#[test]
fn hashmap_moves_noncopy_keys_and_values_through_growth_lookup_remove_and_clear() {
    let source = r#"
module tests.cli.ownership.hashmap_noncopy_value
import std.alloc.string as strings
import std.alloc.hash_map as maps
func main(): int {
    let mut map = maps.new<strings.String, strings.String>()
    let first = maps.insert(map, strings.from("first-key"), strings.from("first-value"))
    let second = maps.insert(map, strings.from("second-key"), strings.from("second-value"))
    let third = maps.insert(map, strings.from("third-key"), strings.from("third-value"))
    let fourth = maps.insert(map, strings.from("fourth-key"), strings.from("fourth-value"))
    let fifth = maps.insert(map, strings.from("fifth-key"), strings.from("fifth-value"))
    let query = strings.from("third-key")
    if !maps.contains(map, ref query) {
        return 1
    }
    match maps.remove(map, ref query) {
        Some(value) => { value.destroy() }
        None => { return 2 }
    }
    let replacement = maps.insert(map, strings.from("replacement-key"), strings.from("replacement-value"))
    maps.clear(map)
    map.destroy()
    query.destroy()
    return 0
}
"#;
    let stdout = run(source, "hashmap_noncopy_value");
    assert!(stdout.is_empty());
    let c_source = emit_c(source, "hashmap_noncopy_value_c");
    run_emitted_c_with_asan(&c_source, "hashmap_noncopy_value_asan");
}

#[test]
fn hashmap_get_ref_borrows_a_noncopy_value_without_cloning() {
    let source = r#"
module tests.cli.ownership.hashmap_get_ref
import std.alloc.string as strings
import std.alloc.hash_map as maps
func main(): int {
    let mut map = maps.new<strings.String, strings.String>()
    let _ = maps.insert(map, strings.from("key"), strings.from("value"))
    let key = strings.from("key")
    let expected = strings.from("value")
    match maps.getRef(map, ref key) {
        Some(value) => {
            if value.eq(ref expected) {
                map.destroy()
                key.destroy()
                expected.destroy()
                return 0
            }
            return 1
        }
        None => { return 2 }
    }
}
"#;
    let stdout = run(source, "hashmap_get_ref");
    assert!(stdout.is_empty());
    let c_source = emit_c(source, "hashmap_get_ref_c");
    run_emitted_c_with_asan(&c_source, "hashmap_get_ref_asan");
}

#[test]
fn hashmap_get_ref_blocks_clear_while_the_borrow_is_live() {
    let stderr = check(
        r#"
module tests.cli.ownership.hashmap_get_ref_live
import std.alloc.string as strings
import std.alloc.hash_map as maps
import std.core.io as io
func main(): int {
    let mut map = maps.new<strings.String, strings.String>()
    let _ = maps.insert(map, strings.from("key"), strings.from("value"))
    let key = strings.from("key")
    let found = maps.getRef(map, ref key)
    maps.clear(map)
    match found {
        Some(value) => { io.println(value.asStr()) }
        None => {}
    }
    return 0
}
"#,
        "hashmap_get_ref_live",
    );
    assert!(
        stderr.contains("O003"),
        "expected clear to conflict with a live value reference, got: {stderr}"
    );
}

#[test]
fn smallvec_get_ref_borrows_noncopy_values_inline_and_after_spill() {
    let source = r#"
module tests.cli.ownership.smallvec_get_ref
import std.alloc.smallvec as smallvec
import std.alloc.string as strings
func main(): int {
    let mut values = smallvec.new<strings.String>()
    values.push(strings.from("inline"))
    let expected_inline = strings.from("inline")
    match smallvec.getRef(values, 0 as usize) {
        Some(value) => { if !value.eq(ref expected_inline) { return 1 } }
        None => { return 2 }
    }
    values.push(strings.from("two"))
    values.push(strings.from("three"))
    values.push(strings.from("four"))
    values.push(strings.from("heap"))
    let expected_heap = strings.from("heap")
    match smallvec.getRef(values, 4 as usize) {
        Some(value) => { if !value.eq(ref expected_heap) { return 3 } }
        None => { return 4 }
    }
    values.destroy()
    expected_inline.destroy()
    expected_heap.destroy()
    return 0
}
"#;
    let stdout = run(source, "smallvec_get_ref");
    assert!(stdout.is_empty());
    let c_source = emit_c(source, "smallvec_get_ref_c");
    run_emitted_c_with_asan(&c_source, "smallvec_get_ref_asan");
}

#[test]
fn smallvec_get_ref_blocks_spill_while_the_borrow_is_live() {
    let stderr = check(
        r#"
module tests.cli.ownership.smallvec_get_ref_live
import std.alloc.smallvec as smallvec
import std.alloc.string as strings
import std.core.io as io
func main(): int {
    let mut values = smallvec.new<strings.String>()
    values.push(strings.from("one"))
    let held = smallvec.getRef(values, 0 as usize)
    values.push(strings.from("two"))
    values.push(strings.from("three"))
    values.push(strings.from("four"))
    values.push(strings.from("fifth"))
    match held {
        Some(value) => { io.println(value.asStr()) }
        None => {}
    }
    return 0
}
"#,
        "smallvec_get_ref_live",
    );
    assert!(
        stderr.contains("O003"),
        "expected spill to conflict with a live element reference, got: {stderr}"
    );
}

#[test]
fn hashmap_supports_structural_user_defined_hash_keys() {
    let stdout = run(
        r#"
module tests.cli.ownership.hashmap_user_hash
import std.alloc.hash_map as maps
import std.core.hash as hash
struct Key { value: int }
public func Key.eq(self: ref Key, other: ref Key): bool {
    return self.value == other.value
}

public func Key.hash<H: hash.Hasher>(self: ref Key, state: mut ref H): void {
    state.writeInt(self.value)
}
func main(): int {
    let mut values = maps.new<Key, int>()
    let key = Key { value: 7 }
    let _ = maps.insert(values, Key { value: 7 }, 42)
    match maps.get(values, ref key) {
        Some(value) => { if value == 42 { return 0 } }
        None => {}
    }
    return 1
}
"#,
        "hashmap_user_hash",
    );
    assert!(stdout.is_empty());
}

#[test]
fn owned_string_implements_structural_hash_without_copying() {
    let source = r#"
module tests.cli.ownership.hash_string
import std.alloc.string as strings
import std.core.hash as hash
func main(): int {
    let left = strings.from("same")
    let right = strings.from("same")
    let different = strings.from("different")
    let mut left_hash = hash.fnvNew()
    let mut right_hash = hash.fnvNew()
    left.hash(mut ref left_hash)
    right.hash(mut ref right_hash)
    if !left.eq(ref right) { return 1 }
    if left_hash.finish() != right_hash.finish() { return 2 }
    if left.eq(ref different) { return 3 }
    return 0
}
"#;
    let stdout = run(source, "hash_string");
    assert!(stdout.is_empty());
    let c_source = emit_c(source, "hash_string_c");
    run_emitted_c_with_asan(&c_source, "hash_string_asan");
}
