use crate::common;
use std::fs;

#[test]
fn array_repetition_runs_once_and_supports_static_lengths_and_ctfe() {
    let root = common::temp_dir("arandu-cli-array-repeat").expect("reserve fixture");
    let source = root.join("main.aru");
    fs::write(
        &source,
        r#"
func flags<comptime N: uint>(): [N]bool { return [false; N] }
func repeated<T, comptime N: uint>(value: T): [N]T { return [value; N] }
enum Packet { Data([64]u8), Empty }
func packet(): Packet { return .Data([42; 64]) }
struct Counter { value: int }
func next(counter: mut ref Counter): int { counter.value += 1; return counter.value }
func main(): int {
    let mut calls = Counter { value: 0 }
    let numbers = [next(calls); 3]
    let empty = [next(calls); 0]
    if calls.value != 2 || numbers[0] != 1 || numbers[2] != 1 { return 1 }
    let flags = flags<256>()
    let generic = repeated<int, 3>(42)
    let computed = [7; comptime (2 + 2)]
    let frozen = comptime { let mut table = [false; 256]; table[32] = true; table }
    let nested = comptime [[7; 2]; 3]
    if flags[255] || !frozen[32] || frozen[33] { return 2 }
    if generic[2] != 42 || computed[3] != 7 || nested[2][1] != 7 { return 3 }
    if @sizeOf([0]int) != 0 { return 4 }
    match packet() {
        Packet.Data(bytes) => { if bytes[63] != 42 { return 5 } }
        Packet.Empty => { return 6 }
    }
    return 0
}
"#,
    )
    .expect("write repeat consumer");
    for command in ["check", "run", "emit-c"] {
        let output = common::cli_command()
            .arg(command)
            .arg(&source)
            .output()
            .expect("run repeat consumer");
        assert!(output.status.success(), "{command}: {output:?}");
        if command == "emit-c"
            && std::process::Command::new("clang")
                .arg("--version")
                .output()
                .is_ok_and(|version| version.status.success())
        {
            let c_file = root.join("repeat.c");
            let binary = root.join(if cfg!(windows) {
                "repeat.exe"
            } else {
                "repeat"
            });
            fs::write(&c_file, &output.stdout).expect("write repeat C");
            let compiled = std::process::Command::new("clang")
                .arg("-O2")
                .arg(c_file)
                .arg("-o")
                .arg(&binary)
                .output()
                .expect("compile repeat C");
            assert!(compiled.status.success(), "{compiled:?}");
            let executed = std::process::Command::new(binary)
                .output()
                .expect("execute repeat C");
            assert!(executed.status.success(), "{executed:?}");
        }
    }
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn array_repetition_rejects_ownership_duplication_and_excessive_lengths() {
    let root = common::temp_dir("arandu-cli-array-repeat-invalid").expect("reserve fixture");
    let source = root.join("main.aru");
    for program in [
        "func main(): void { let mut x = 0; let refs = [mut ref x; 2] }",
        "func main(): void { let x = [false; 65537] }",
        "func main(): void { let x = [false; 18446744073709551615] }",
        "func main(): void { let x = [false; bool] }",
        "func main(): void { let x = [[false; 65536]; 65536] }",
        "func repeat<T, comptime N: uint>(x: T): [N]T { return [x; N] }\nfunc main(): void { let mut x = 0; let refs = repeat<mut ref int, 2>(mut ref x) }",
    ] {
        fs::write(&source, program).expect("write invalid repetition");
        let output = common::cli_command()
            .arg("check")
            .arg(&source)
            .output()
            .expect("check invalid repetition");
        assert!(!output.status.success(), "{program}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("T048"),
            "{program}: {output:?}"
        );
    }
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn zero_and_one_repetitions_drop_owners_exactly_once() {
    let root = common::temp_dir("arandu-cli-array-repeat-drop").expect("reserve fixture");
    let source = root.join("main.aru");
    fs::write(
        &source,
        r#"
import std.io as io
struct Owner { id: int }
@Destructor
func Owner.destroy(own self: Owner): void { io.println("drop ${self.id}") }
func make(id: int): Owner { return Owner { id: id } }
func main(): int {
    let empty = [make(10); 0]
    io.println("after zero")
    let single = [make(20); 1]
    if single[0].id != 20 { return 1 }
    return 0
}
"#,
    )
    .expect("write owning repetition");
    let stdlib = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib");
    let output = common::cli_command()
        .arg("run")
        .arg(&source)
        .arg("--stdlib-path")
        .arg(stdlib)
        .output()
        .expect("run owning repetition");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "drop 10\nafter zero\ndrop 20\n"
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn stdlib_byte_sets_freeze_imported_aggregate_helpers_and_match_all_bytes() {
    let root = common::temp_dir("arandu-cli-byte-set").expect("reserve fixture");
    let source = root.join("main.aru");
    fs::write(&source, r#"
import std.core.ascii as ascii
func main(): int {
    let horizontal = comptime ascii.byteSet(" \t\r")
    let identifier = comptime ascii.byteSet("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_")
    let empty = comptime ascii.byteSet("")
    let horizontalTable = comptime ascii.byteTable(" \t\r")
    let edgeTable = comptime ascii.byteTable("\0?@Àÿ??\0")
    let edge = comptime ascii.byteSet("\0?@Àÿ??\0")
    let runtime = ascii.byteSet(" \t\r")
    if @sizeOf(ascii.ByteSet) != 32 { return 1 }
    let mut value: uint = 0
    while value < 256 {
        let b = value as u8
        let expectedSpace = b == 32 || b == 9 || b == 13
        let expectedWord = (b >= 97 && b <= 122) || (b >= 65 && b <= 90)
            || (b >= 48 && b <= 57) || b == 95
        let expectedEdge = b == 0 || b == 63 || b == 64 || b == 195 || b == 128 || b == 191
        if horizontal.contains(b) != expectedSpace { return 2 }
        if identifier.contains(b) != expectedWord { return 3 }
        if empty.contains(b) { return 4 }
        if edge.contains(b) != expectedEdge { return 5 }
        if runtime.contains(b) != horizontal.contains(b) { return 6 }
        if horizontalTable[value as usize] != expectedSpace { return 7 }
        if edgeTable[value as usize] != expectedEdge { return 8 }
        value = value + 1
    }
    return 0
}
"#).expect("write byte-set consumer");
    let stdlib = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib");
    for command in ["check", "run", "amir", "emit-c"] {
        let output = common::cli_command()
            .arg(command)
            .arg(&source)
            .arg("--stdlib-path")
            .arg(&stdlib)
            .output()
            .expect("execute byte-set consumer");
        assert!(output.status.success(), "{command}: {output:?}");
        if command == "amir" {
            let amir = std::str::from_utf8(&output.stdout).expect("AMIR output");
            assert_eq!(
                amir.matches("call fn@byteSet(").count(),
                1,
                "only the explicitly runtime builder may remain:\n{amir}"
            );
        }
        if command == "emit-c"
            && std::process::Command::new("clang")
                .arg("--version")
                .output()
                .is_ok_and(|version| version.status.success())
        {
            let c_file = root.join("byte_set.c");
            let binary = root.join(if cfg!(windows) {
                "byte_set.exe"
            } else {
                "byte_set"
            });
            fs::write(&c_file, output.stdout).expect("write emitted C");
            let compile = std::process::Command::new("clang")
                .arg("-O2")
                .arg(&c_file)
                .arg("-o")
                .arg(&binary)
                .output()
                .expect("compile byte-set consumer");
            assert!(compile.status.success(), "{compile:?}");
            let executed = std::process::Command::new(&binary)
                .output()
                .expect("run byte-set C consumer");
            assert!(executed.status.success(), "{executed:?}");
        }
    }
    fs::remove_dir_all(root).expect("remove fixture");
}

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
