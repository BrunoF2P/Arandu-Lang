use arandu_backend_c::CEmitter;
use arandu_backend_cranelift::CraneliftBackend;
use arandu_middle::amir::AmirProgram;
use arandu_middle::layout::LayoutEngine;
use arandu_semantics::{
    CodegenBackend, CompileSession, TypeCheckResult, lower_to_amir, lower_to_hir, resolve,
    type_check_with_session,
};
use std::env;
use std::fs;
use std::process::Command;

fn compile_src(src: &str) -> (AmirProgram, TypeCheckResult) {
    let program = arandu_parser::parse(src).expect("parse failed");
    let mut session = CompileSession::new();
    let resolution = resolve(&program);
    let mut tc = type_check_with_session(resolution, &program, &mut session);
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
    let backend = CraneliftBackend::new();
    let compiled = CodegenBackend::compile(backend, amir, &tc.symbols, &tc.type_info)
        .expect("cranelift compile failed");

    unsafe {
        let main_fn =
            arandu_semantics::CompiledCode::get_fn::<unsafe fn() -> i32>(&compiled, "main")
                .expect("main not found");
        main_fn()
    }
}

fn test_execution_parity(name: &str, src: &str) {
    let (amir, tc) = compile_src(src);

    // 1. Run via Cranelift
    let expected = execute_cranelift(&amir, &tc);

    // 2. Generate C
    let layout_engine = LayoutEngine::new(8);
    let emitter = CEmitter::new(
        &amir,
        &tc.symbols,
        &layout_engine,
        &tc.type_info,
        &tc.type_info.type_interner,
    );
    let mut c_code = emitter.emit();

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

    let compile_status = Command::new(&cc)
        .arg(&c_file)
        .arg("-o")
        .arg(&exe_file)
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

    let stdout = String::from_utf8(output.stdout).unwrap();
    let actual_result: i32 = stdout
        .trim()
        .parse()
        .expect("failed to parse stdout as integer");

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
