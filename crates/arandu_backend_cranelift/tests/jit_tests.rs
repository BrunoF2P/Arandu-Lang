use arandu_backend_cranelift::CraneliftBackend;
use arandu_semantics::{resolve, type_check, lower_to_hir, lower_to_amir};

fn compile_src(src: &str) -> (arandu_semantics::amir::AmirProgram, arandu_semantics::SymbolTable) {
    let program = arandu_parser::parse(src).expect("parse failed");
    let resolution = resolve(&program);
    let tc = type_check(resolution, &program);
    let hir = lower_to_hir(&tc, &program).expect("HIR lowering failed");
    let amir = lower_to_amir(&tc, &hir).expect("AMIR lowering failed");
    (amir, tc.symbols)
}

#[test]
fn jit_constant_i32() {
    let src = "func main() int { return 42; }";
    let (amir, symbols) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols).unwrap();
    
    let result: i32 = unsafe {
        let f: unsafe fn() -> i32 = module.get_fn("main").unwrap();
        f()
    };
    assert_eq!(result, 42);
}

#[test]
fn jit_add_i32() {
    let src = "func add(a int, b int) int { return a + b; }";
    let (amir, symbols) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols).unwrap();
    
    let result: i32 = unsafe {
        let f: unsafe fn(i32, i32) -> i32 = module.get_fn("add").unwrap();
        f(10, 32)
    };
    assert_eq!(result, 42);
}

#[test]
fn jit_control_flow() {
    let src = r#"
    func max(a int, b int) int {
        mut res = 0
        if a > b {
            set res = a
        } else {
            set res = b
        }
        return res
    }
    "#;
    let (amir, symbols) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols).unwrap();
    
    let result: i32 = unsafe {
        let f: unsafe fn(i32, i32) -> i32 = module.get_fn("max").unwrap();
        f(10, 20)
    };
    assert_eq!(result, 20);

    let result2: i32 = unsafe {
        let f: unsafe fn(i32, i32) -> i32 = module.get_fn("max").unwrap();
        f(42, 7)
    };
    assert_eq!(result2, 42);
}
