use crate::common;
use std::fs;

#[test]
fn static_loops_execute_with_fresh_locals_and_structured_exits() {
    let root = common::temp_dir("arandu-cli-static-for").expect("reserve fixture");
    let source = root.join("main.aru");
    for (index, program) in [
        "func main(): int { let mut sum = 0\ncomptime for i in 0..4 { let item = i + 1\nsum += item }\nreturn sum - 10 }",
        "func main(): int { let mut sum = 0\ncomptime for i in 0..4 { if i == 1 { continue } if i == 3 { break } sum += i }\nreturn sum - 2 }",
        "func main(): int { let mut sum = 0\ncomptime for i in 0..3 { comptime for j in 0..2 { sum += i + j } }\nreturn sum - 9 }",
        "func first(): int { comptime for i in 0..4 { return i + 7 } return 99 }\nfunc main(): int { return first() - 7 }",
        "func main(): int { return comptime { let mut sum = 0\ncomptime for i in 0..4 { sum += i }\nsum } - 6 }",
    ].iter().enumerate() {
        fs::write(&source, program).expect("write static loop source");
        for command in ["check", "run", "emit-c"] {
            let output = common::cli_command().arg(command).arg(&source).output()
                .expect("execute static loop source");
            assert!(output.status.success(), "case {index}, {command}: {output:?}");
            if command == "emit-c" {
                assert!(!String::from_utf8_lossy(&output.stdout).contains("comptime for"));
            }
        }
    }
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn public_layout_expressions_execute_and_emit_c_without_runtime_intrinsics() {
    let root = common::temp_dir("arandu-cli-layout").expect("reserve fixture");
    let source = root.join("main.aru");
    fs::write(&source, "func size<T>(): usize { return @sizeOf(T) }\nfunc main(): int { let n = comptime (@sizeOf([3]u16)); if n == 6 && @alignOf(i64) == 8 && size<int>() == 4 { return 0 } return 1 }")
        .expect("write layout source");
    for command in ["check", "run", "emit-c"] {
        let output = common::cli_command()
            .arg(command)
            .arg(&source)
            .output()
            .expect("run layout expressions");
        assert!(output.status.success(), "{command}: {output:?}");
        if command == "emit-c" {
            let emitted = String::from_utf8(output.stdout).expect("C output");
            assert!(!emitted.contains("@sizeOf"));
            assert!(!emitted.contains("@alignOf"));
        }
    }
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn public_comptime_runs_from_a_standalone_source() {
    let root = common::temp_dir("arandu-cli-comptime").expect("reserve fixture");
    let source = root.join("main.aru");
    fs::write(
        &source,
        "func add(a: int, b: int): int { return a + b }\n\
         func main(): int {\n\
         let answer = comptime { return add(20, 22) }\n\
         comptime if true { if answer == 42 { return 0 } } else { unavailable() }\nreturn 1\n}\n",
    )
    .expect("write source");
    for command in ["check", "run"] {
        let output = common::cli_command()
            .arg(command)
            .arg(&source)
            .output()
            .expect("execute public syntax");
        assert!(output.status.success(), "{command}: {output:?}");
    }
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn public_comptime_capture_reports_context_instead_of_running() {
    let root = common::temp_dir("arandu-cli-comptime-capture").expect("reserve fixture");
    let source = root.join("main.aru");
    fs::write(
        &source,
        "func main(): int { let outside = 42\nreturn comptime outside }\n",
    )
    .expect("write source");
    let output = common::cli_command()
        .args(["check", "--color=never"])
        .arg(&source)
        .output()
        .expect("check forbidden capture");
    assert!(!output.status.success(), "{output:?}");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("T043"), "{error}");
    assert!(error.contains("runtime value 'outside'"), "{error}");
    assert!(error.contains("runtime value declared here"), "{error}");
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn public_comptime_formats_multiple_sources_without_changing_their_results() {
    let root = common::temp_dir("arandu-cli-comptime-fmt").expect("reserve fixture");
    let first = root.join("first.aru");
    let second = root.join("second.aru");
    fs::write(
        &first,
        "func main():int{let x=comptime{return 42}\nreturn x-42}",
    )
    .expect("write block");
    fs::write(&second, "func main():int{return comptime(20+22)-42}").expect("write expression");
    let third = root.join("third.aru");
    fs::write(&third, "func count<comptime N:uint>():uint{return N}\nfunc main():int{return(count<comptime(20+22)>() as int)-42}")
        .expect("write computed argument");
    for check in [false, true] {
        let mut command = common::cli_command();
        command.arg("fmt");
        if check {
            command.arg("--check");
        }
        let output = command
            .args([&first, &second, &third])
            .output()
            .expect("format sources");
        assert!(output.status.success(), "{output:?}");
    }
    for source in [&first, &second, &third] {
        let output = common::cli_command()
            .arg("run")
            .arg(source)
            .output()
            .expect("run formatted source");
        assert!(output.status.success(), "{source:?}: {output:?}");
    }
    fs::remove_dir_all(root).expect("remove fixtures");
}
