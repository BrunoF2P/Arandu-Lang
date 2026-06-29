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
        r"func main() {
    let value: int = 1 + 2
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
    assert!(optimized_stdout.contains("_1 = 3"));
}

#[test]
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
