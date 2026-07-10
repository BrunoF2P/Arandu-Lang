#![cfg(target_pointer_width = "64")]

use arandu_backend_cranelift::CraneliftBackend;
use arandu_middle::amir::AmirProgram;
use arandu_middle::layout::DataLayout;
use arandu_semantics::{
    CodegenBackend, TypeCheckResult, lower_to_amir, lower_to_hir, resolve_for_test, type_check,
};
use std::env;
use std::fs;
use std::process::Command;

fn compile_src(src: &str) -> (AmirProgram, TypeCheckResult) {
    let program = arandu_parser::parse(src).expect("parse failed");
    let resolution = resolve_for_test(0, &program);
    let mut tc = type_check(resolution, &program);
    assert!(
        tc.diagnostics.is_empty(),
        "type check failed: {:?}",
        tc.diagnostics
    );

    let hir = lower_to_hir(&mut tc, &program).expect("HIR lowering failed");
    let amir = lower_to_amir(&tc, &hir).expect("AMIR lowering failed");
    (amir, tc)
}

fn execute_cranelift(amir: &AmirProgram, tc: &TypeCheckResult) -> i32 {
    let backend = CraneliftBackend::try_new().unwrap();
    let compiled =
        CodegenBackend::compile(backend, amir, tc.symbols.as_ref(), tc.type_info.as_ref())
            .expect("cranelift compile failed");

    unsafe {
        let main_fn =
            arandu_semantics::CompiledCode::get_fn::<unsafe fn() -> i32>(&compiled, "main")
                .expect("main not found");
        main_fn()
    }
}

fn emit_c(amir: &AmirProgram, tc: &TypeCheckResult) -> String {
    // Host parity only; Cranelift is host-only — see solidification matrix.
    arandu_backend_c::emit_c(
        amir,
        tc.symbols.as_ref(),
        tc.type_info.as_ref(),
        &tc.type_info.type_interner,
        arandu_middle::layout::DataLayout::host(),
    )
}

fn test_execution_parity(name: &str, src: &str) {
    let (amir, tc) = compile_src(src);

    // 1. Generate C (no debug dumps — keep tests pure / CI-friendly).
    let mut c_code = emit_c(&amir, &tc);

    // CEmitter emits `int32_t main(void)`. We rename it to `arandu_main` via a preprocessor
    // macro so we can wrap it in a standard C `main` that captures and prints the return
    // value for parity comparison with the Cranelift result.
    c_code = format!("#define main arandu_main\n{}\n#undef main\n", c_code);
    c_code.push_str(
        r#"
#include <stdio.h>
int main() {
    int32_t res = arandu_main();
    printf("%d\n", res);
    return 0;
}
"#,
    );

    let out_dir = env::temp_dir().join("arandu_c_tests");
    fs::create_dir_all(&out_dir).unwrap();
    let c_file = out_dir.join(format!("{}.c", name));
    let exe_file = out_dir.join(format!("{}.exe", name)); // .exe works on windows

    fs::write(&c_file, c_code).unwrap();

    // Compiler selection: use $CC env var if set, otherwise fallback to gcc.
    let cc = env::var("CC").unwrap_or_else(|_| "gcc".to_string());

    // `-lm` for ToStr float helpers (`isnan`/`isinf` via math.h).
    let compile_status = Command::new(&cc)
        .arg(&c_file)
        .arg("-o")
        .arg(&exe_file)
        .arg("-lm")
        .status()
        .unwrap_or_else(|_| {
            panic!(
                "failed to invoke C compiler '{}'. Parity tests require a C compiler in PATH.",
                cc
            )
        });

    assert!(
        compile_status.success(),
        "C compilation failed for {}",
        name
    );

    let output = Command::new(&exe_file)
        .output()
        .expect("failed to run compiled executable");

    assert!(output.status.success(), "C program crashed for {}", name);

    // Last line is the harness exit code (`printf("%d\n", res)`). Earlier lines
    // may be `io.println` output (ToStr product path).
    let stdout = String::from_utf8(output.stdout).unwrap();
    let last_line = stdout
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    let actual_result: i32 = last_line
        .parse()
        .unwrap_or_else(|_| panic!("failed to parse C exit line as integer: {stdout:?}"));

    // 2. Run via Cranelift
    let expected = execute_cranelift(&amir, &tc);

    assert_eq!(
        expected, actual_result,
        "Execution mismatch for {}! Cranelift={}, C={}",
        name, expected, actual_result
    );
}

#[test]
fn parity_fibonacci() {
    let src = r#"
    func fib(n: int): int {
        if n <= 1 {
            return n
        }
        return fib(n - 1) + fib(n - 2)
    }
    
    func main(): int {
        return fib(10)
    }
    "#;
    test_execution_parity("fibonacci", src);
}

#[test]
fn parity_struct_layout() {
    let src = r#"
    struct Point {
        x: int
        y: byte
        z: int
    }
    
    func main(): int {
        let p = Point { x: 10, y: 5 as byte, z: 20 }
        return p.z
    }
    "#;
    test_execution_parity("struct_layout", src);
}

#[test]
fn parity_str_literal() {
    let src = r#"
    func get_len(s: str): int {
        return 42 // fixed value; this test verifies str structs can be passed without crashing
    }
    func main(): int {
        return get_len("hello")
    }
    "#;
    test_execution_parity("str_literal", src);
}

#[test]
fn parity_string_interpolation() {
    // Builds an interpolated string and only checks that the program runs
    // end-to-end on both backends (C + Cranelift) without crash.
    let src = r#"
    func main(): int {
        let name = "Bruno"
        let msg = "Oi, ${name}"
        return 0
    }
    "#;
    test_execution_parity("string_interpolation", src);
}

#[test]
fn parity_enum_layout() {
    let src = r#"
    enum Status {
        Ok(int)
        Err(byte)
    }
    
    func main(): int {
        let r: Status = Status.Ok(42)
        let mut out: int = 0
        match r {
            Status.Ok(v) => { out = v; }
            Status.Err(_) => { out = -1; }
        }
        return out
    }
    "#;
    test_execution_parity("enum_layout", src);
}

#[test]
fn parity_ssa_pattern_bind() {
    let src = r#"
    enum Wrapper {
        Val(int)
    }
    
    func main(): int {
        let w: Wrapper = Wrapper.Val(123)
        let mut res: int = 0
        if w is Wrapper.Val(x) {
            res = x
        }
        return res
    }
    "#;
    test_execution_parity("ssa_pattern_bind", src);
}

#[test]
fn parity_ssa_pattern_bind_multi_arms() {
    let src = r#"
    enum Wrapper {
        Val(int)
        Other(int)
    }
    
    func main(): int {
        let w: Wrapper = Wrapper.Other(42)
        let mut res: int = 0
        match w {
            Wrapper.Val(x) => {
                res = x
            }
            Wrapper.Other(y) => {
                res = y
            }
        }
        return res
    }
    "#;
    test_execution_parity("ssa_pattern_bind_multi_arms", src);
}

#[test]
fn parity_array_index_access() {
    let src = r#"
    func dummy(xs: [3]int) {}

    func main(): int {
        let mut xs = [10, 20, 30]
        let idx = 1
        xs[idx] = 42
        dummy(xs)
        return 42
    }
    "#;
    test_execution_parity("array_index_access", src);
}

#[test]
fn parity_enum_multi_variant_switch() {
    let src = r#"
    enum Color {
        Red
        Green
        Blue
        Yellow(int)
    }
    
    func main(): int {
        let c: Color = Color.Yellow(100)
        let mut out: int = 0
        match c {
            Color.Red => { out = 1; }
            Color.Green => { out = 2; }
            Color.Blue => { out = 3; }
            Color.Yellow(v) => { out = v; }
        }
        return out
    }
    "#;
    test_execution_parity("enum_multi_variant_switch", src);
}

#[test]
fn parity_array_reassignment() {
    let src = r#"
    func main(): int {
        let mut arr = [10, 20, 30]
        arr = [99, 98, 97]
        return arr[1]
    }
    "#;
    test_execution_parity("array_reassignment", src);
}

#[test]
fn parity_control_flow_diamond() {
    let src = r#"
    func main(): int {
        let x = 10
        let mut out = 0
        if x > 5 {
            out = 1
        } else {
            out = 2
        }
        return out
    }
    "#;
    test_execution_parity("control_flow_diamond", src);
}

#[test]
fn parity_to_str_int_interp() {
    // ToStr v0.1: int formatted into string interp; both backends exit 0.
    let src = r#"
    func main(): int {
        let n: int = 42
        let s = "n=${n}"
        let t = "b=${true}"
        return 0
    }
    "#;
    test_execution_parity("to_str_int_interp", src);
}

#[test]
fn parity_io_println_to_str() {
    // println stub + ToStr; exit code only (stdout not compared).
    let src = r#"
    import io
    func main(): int {
        io.println(42)
        io.println("n=${7}")
        return 0
    }
    "#;
    test_execution_parity("io_println_to_str", src);
}

#[test]
fn parity_to_str_method_and_float() {
    let src = r#"
    import io
    func main(): int {
        let n: int = 10
        let f: float = 2.0
        io.println(n.to_str())
        io.println(f.to_str())
        return 0
    }
    "#;
    test_execution_parity("to_str_method_float", src);
}

#[test]
fn c_emit_to_str_helpers_present() {
    let src = r#"
    func main(): int {
        let n: int = 7
        let s = "x=${n}"
        return 0
    }
    "#;
    let (amir, tc) = compile_src(src);
    let c = emit_c(&amir, &tc);
    assert!(
        c.contains("ar_i64_to_str"),
        "expected ToStr helper in emit, got:\n{c}"
    );
    assert!(
        c.contains("to_str") || c.contains("ar_i64_to_str("),
        "expected ToStr call site"
    );
}

#[test]
fn c_emit_arstr_is_fat_pointer() {
    // S-C-AUDIT: ArStr matches LayoutEngine fat pointer (host 64 → int64_t len).
    let src = r#"
    func main(): int {
        let s = "hi"
        return 0
    }
    "#;
    let (amir, tc) = compile_src(src);
    let c = emit_c(&amir, &tc);
    assert!(
        c.contains("typedef struct { const uint8_t *ptr; int64_t len; } ArStr;"),
        "expected ArStr fat-pointer typedef, got headers:\n{}",
        c.lines().take(40).collect::<Vec<_>>().join("\n")
    );
    assert!(c.contains("AR_STR_"), "expected named string constants");
}

#[test]
fn c_emit_arstr_layout_32bit() {
    // S-C-32BIT: emit-only with W=4 (no Cranelift). ArStr.len is int32_t.
    let src = r#"
    func main(): int {
        let s = "hi"
        return 0
    }
    "#;
    let (amir, tc) = compile_src(src);
    let c = arandu_backend_c::emit_c(
        &amir,
        tc.symbols.as_ref(),
        tc.type_info.as_ref(),
        &tc.type_info.type_interner,
        DataLayout::ptr_width(4),
    );
    assert!(
        c.contains("typedef struct { const uint8_t *ptr; int32_t len; } ArStr;"),
        "expected 32-bit ArStr, headers:\n{}",
        c.lines().take(40).collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn c_emit_arstr_i686_sysv() {
    // DataLayout::i686_sysv: pointer 4; i64/f64 abi_align 4 — ArStr still {ptr, int32_t len}.
    let src = r#"
    func main(): int {
        let s = "hi"
        return 0
    }
    "#;
    let (amir, tc) = compile_src(src);
    let c = arandu_backend_c::emit_c(
        &amir,
        tc.symbols.as_ref(),
        tc.type_info.as_ref(),
        &tc.type_info.type_interner,
        DataLayout::i686_sysv(),
    );
    assert!(
        c.contains("typedef struct { const uint8_t *ptr; int32_t len; } ArStr;"),
        "i686 ArStr: {}",
        c.lines().take(30).collect::<Vec<_>>().join("\n")
    );
}
