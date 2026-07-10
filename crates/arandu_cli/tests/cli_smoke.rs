use std::fs;
use std::process::Command;

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_arandu_cli"))
        .args(args)
        .output()
        .expect("cli should run")
}

#[test]
fn invalid_usage_exits_with_code_2() {
    let output = run_cli(&[]);

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage: arandu_cli"));
}

#[test]
fn missing_file_exits_with_code_1() {
    let output = run_cli(&["lex", "missing-file.aru"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to read"));
}

#[test]
fn lex_parse_and_check_valid_files_exit_successfully() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_smoke.aru");
    fs::write(
        &file,
        r"module tests.cli

func main() {
    let value = 1
}
",
    )
    .expect("fixture should be writable");

    let path = file.to_string_lossy();
    let lex = run_cli(&["lex", &path]);
    let parse = run_cli(&["parse", &path]);
    let check = run_cli(&["check", &path]);

    assert!(lex.status.success());
    assert!(String::from_utf8_lossy(&lex.stdout).contains("KW_MODULE"));
    assert!(parse.status.success());
    let parse_stdout = String::from_utf8_lossy(&parse.stdout);
    assert!(parse_stdout.contains("Func @"));
    assert!(parse_stdout.contains("main() -> void"));
    assert!(check.status.success());
    assert!(String::from_utf8_lossy(&check.stdout).contains("ok"));
}

/// Builtin prelude must resolve on the CLI/Salsa path without on-disk modules.
#[test]
fn check_import_io_prelude_succeeds() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_prelude_io.aru");
    fs::write(
        &file,
        r#"module tests.cli.prelude

import io
import err

func boom(): Result<int, Err> {
    return Result.Err(err.new("x"))
}

func main() {
    io.println("ok")
}
"#,
    )
    .expect("fixture should be writable");

    let path = file.to_string_lossy();
    let output = run_cli(&["check", &path]);
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("ok"));
}

/// Official stable examples must type-check end-to-end on the CLI.
#[test]
fn check_stable_examples_succeed() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest_dir)
        .join("../..")
        .canonicalize()
        .expect("workspace root");
    let stable_root = workspace_root.join("examples/stable");

    let mut files = Vec::new();
    fn collect_aru(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in fs::read_dir(dir).expect("read examples/stable") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                collect_aru(&path, out);
            } else if path.extension().and_then(|s| s.to_str()) == Some("aru") {
                out.push(path);
            }
        }
    }
    collect_aru(&stable_root, &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "expected stable examples under {}",
        stable_root.display()
    );

    for file in files {
        let path = file.to_string_lossy();
        let output = run_cli(&["check", &path]);
        assert!(
            output.status.success(),
            "check failed for {}:\n{}",
            path,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn check_invalid_file_reports_name_error_and_exits_1() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_check_invalid.aru");
    fs::write(
        &file,
        r"module tests.cli

func main() {
    value = missing_name
}
",
    )
    .expect("fixture should be writable");

    let path = file.to_string_lossy();
    let output = run_cli(&["check", &path]);

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("N001"));
}

#[test]
fn check_missing_set_target_reports_specific_name_error() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_check_set_missing.aru");
    fs::write(
        &file,
        r"module tests.cli

func main() {
    missing = 1
}
",
    )
    .expect("fixture should be writable");

    let path = file.to_string_lossy();
    let output = run_cli(&["check", &path]);

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("N007"));
}

#[test]
fn amir_opt_flag_folds_constants_without_changing_default_command() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_amir_opt.aru");
    fs::write(
        &file,
        r"func main(): int {
    let value: int = 1 + 2
    return value
}
",
    )
    .expect("fixture should be writable");

    let path = file.to_string_lossy();
    let plain = run_cli(&["amir", &path]);
    let optimized = run_cli(&["amir", &path, "--opt"]);

    assert!(plain.status.success());
    assert!(optimized.status.success());

    let plain_stdout = String::from_utf8_lossy(&plain.stdout);
    let optimized_stdout = String::from_utf8_lossy(&optimized.stdout);
    assert!(plain_stdout.contains("add 1, 2"));
    assert!(!optimized_stdout.contains("add 1, 2"));
    assert!(optimized_stdout.contains("= 3"));
}

#[test]
#[cfg(target_pointer_width = "64")]
fn run_returns_main_exit_code() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_run_return.aru");
    fs::write(
        &file,
        r"func main(): int {
    return 42
}
",
    )
    .expect("fixture should be writable");

    let path = file.to_string_lossy();
    let output = run_cli(&["run", &path]);

    assert_eq!(output.status.code(), Some(42));
}

#[test]
#[cfg(target_pointer_width = "64")]
fn run_signed_integer_arithmetic_exits_successfully() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_run_signed.aru");
    fs::write(
        &file,
        r"func main(): int {
    let div = -1 / 2
    let rem = -7 % 3
    if div != 0 {
        return 1
    }
    if rem != -1 {
        return 2
    }
    return 0
}
",
    )
    .expect("fixture should be writable");

    let path = file.to_string_lossy();
    let output = run_cli(&["run", &path]);

    assert_eq!(output.status.code(), Some(0));
}

#[test]
#[cfg(target_pointer_width = "64")]
fn run_control_flow_returns_expected_code() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_run_control_flow.aru");
    fs::write(
        &file,
        r"func main(): int {
    let mut res = 0
    let a: int = 10
    let b: int = 20
    if a > b {
        res = a
    } else {
        res = b
    }
    return res
}
",
    )
    .expect("fixture should be writable");

    let path = file.to_string_lossy();
    let output = run_cli(&["run", &path]);

    assert_eq!(output.status.code(), Some(20));
}

#[test]
#[cfg(target_pointer_width = "64")]
fn run_with_ztime_passes_emits_perf_timings() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_run_perf.aru");
    fs::write(
        &file,
        r"func main(): int {
    return 0
}
",
    )
    .expect("fixture should be writable");

    let path = file.to_string_lossy();
    let output = run_cli(&["-Ztime-passes", "run", &path]);

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("parse+check") || stderr.contains("[perf]"),
        "expected perf output in stderr, got:\n{stderr}"
    );
}

/// ToStr v0.1: CLI check accepts println/interp of primitives.
#[test]
fn check_to_str_primitives_succeeds() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_to_str_check.aru");
    fs::write(
        &file,
        r#"module tests.cli.tostr

import io

func main(): int {
    let n: int = 7
    io.println(n)
    io.println("n=${n}")
    return 0
}
"#,
    )
    .expect("fixture");
    let path = file.to_string_lossy();
    let output = run_cli(&["check", &path]);
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// ToStr v0.1: `run` executes println with int (prelude host stub).
#[test]
fn run_to_str_println_int_exits_0() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_to_str_run.aru");
    fs::write(
        &file,
        r#"module tests.cli.tostr_run

import io

func main(): int {
    let n: int = 42
    io.println(n)
    io.println("answer=${n}")
    return 0
}
"#,
    )
    .expect("fixture");
    let path = file.to_string_lossy();
    let output = run_cli(&["run", &path]);
    assert!(
        output.status.success(),
        "stderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("42") && stdout.contains("answer=42"),
        "expected formatted output, got:\n{stdout}"
    );
}

/// ToStr v0.1: method `.to_str()` + fixed-width integers + float specials path.
#[test]
fn run_to_str_method_and_fixed_widths() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_to_str_method.aru");
    fs::write(
        &file,
        r#"module tests.cli.tostr_method

import io

func main(): int {
    let n: int = 42
    let i8v: i8 = -5
    let u64v: u64 = 99
    let f: float = 2.0
    io.println(n.to_str())
    io.println(i8v)
    io.println(u64v)
    io.println(f.to_str())
    io.println(true.to_str())
    return 0
}
"#,
    )
    .expect("fixture");
    let path = file.to_string_lossy();
    let output = run_cli(&["run", &path]);
    assert!(
        output.status.success(),
        "stderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in ["42", "-5", "99", "2", "true"] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?} in:\n{stdout}"
        );
    }
}

/// ToStr v0.1: emit-c includes ToStr helpers and io_println stub.
#[test]
fn emit_c_to_str_includes_helpers_and_println() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_to_str_emit.aru");
    fs::write(
        &file,
        r#"module tests.cli.tostr_emit

import io

func main(): int {
    io.println(1)
    return 0
}
"#,
    )
    .expect("fixture");
    let path = file.to_string_lossy();
    let output = run_cli(&["emit-c", &path, "--layout=host"]);
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let c = String::from_utf8_lossy(&output.stdout);
    assert!(c.contains("ar_i64_to_str"), "missing ToStr helper:\n{c}");
    assert!(c.contains("io_println"), "missing io_println stub:\n{c}");
    assert!(c.contains("int main("), "missing main:\n{c}");
}

/// S5 gate: emit-c produces host C with int main and DataLayout-driven types.
#[test]
fn emit_c_host_fib_main_contains_int_main() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let fib = std::path::Path::new(manifest_dir)
        .join("../../examples/stable/syntax/fib_main.aru")
        .canonicalize()
        .expect("fib_main.aru");
    let path = fib.to_string_lossy();
    let output = run_cli(&["emit-c", &path, "--layout=host"]);
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("int main("),
        "expected int main in C, got:\n{stdout}"
    );
    assert!(stdout.contains("fib("), "expected fib in C output");
    // No str runtime for pure-int program
    assert!(
        !stdout.contains("ar_str_concat_n"),
        "unexpected str runtime for fib_main"
    );
}

#[test]
fn emit_c_i686_uses_int32_for_platform_int() {
    let dir = std::env::temp_dir();
    let file = dir.join("arandu_cli_emit_c_i686.aru");
    fs::write(
        &file,
        r"func main(): int {
    return 7
}
",
    )
    .expect("fixture");
    let path = file.to_string_lossy();
    let output = run_cli(&["emit-c", &path, "--layout=i686"]);
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // main body still int; platform int locals/params use 32-bit when present
    assert!(stdout.contains("int main("));
    // return value temp is platform int width
    assert!(
        stdout.contains("int32_t") || stdout.contains("return (int)"),
        "expected 32-bit layout types:\n{stdout}"
    );
}
