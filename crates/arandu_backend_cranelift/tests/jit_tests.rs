use arandu_backend_cranelift::CraneliftBackend;
use arandu_semantics::{lower_to_amir, lower_to_hir, resolve, type_check};

fn compile_src(
    src: &str,
) -> (
    arandu_semantics::amir::AmirProgram,
    arandu_semantics::SymbolTable,
) {
    let program = arandu_parser::parse(src).expect("parse failed");
    let resolution = resolve(&program);
    let tc = type_check(resolution, &program);
    let hir = lower_to_hir(&tc, &program).expect("HIR lowering failed");
    let amir = lower_to_amir(&tc, &hir).expect("AMIR lowering failed");
    (amir, tc.symbols)
}

#[test]
fn jit_constant_i32() {
    let src = "func main(): int { return 42; }";
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
    let src = "func add(a: int, b: int): int { return a + b; }";
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
    func max(a: int, b: int): int {
        let mut res = 0
        if a > b {
            res = a
        } else {
            res = b
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

#[test]
fn jit_unsigned_comparison() {
    let src = r#"
    func is_gt(a: u32, b: u32): bool {
        return a > b;
    }
    "#;
    let (amir, symbols) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols).unwrap();

    let result: bool = unsafe {
        let f: unsafe fn(u32, u32) -> bool = module.get_fn("is_gt").unwrap();
        f(4294967295, 0)
    };
    assert!(result);
}

#[test]
fn jit_unsigned_div() {
    let src = r#"
    func half(a: u32): u32 {
        return a / 2;
    }
    "#;
    let (amir, symbols) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols).unwrap();

    let result: u32 = unsafe {
        let f: unsafe fn(u32) -> u32 = module.get_fn("half").unwrap();
        f(4_294_967_295)
    };
    // u32::MAX / 2 = 2_147_483_647; signed interpretation (-1 / 2) would be 0.
    assert_eq!(result, 2_147_483_647);
}

#[test]
fn jit_unsigned_mod() {
    let src = r#"
    func rem(a: u32, b: u32): u32 {
        return a % b;
    }
    "#;
    let (amir, symbols) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols).unwrap();

    let result: u32 = unsafe {
        let f: unsafe fn(u32, u32) -> u32 = module.get_fn("rem").unwrap();
        f(4_294_967_295, 4_294_967_294)
    };
    assert_eq!(result, 1);
}

#[test]
fn jit_unsigned_shift_right() {
    let src = r#"
    func shr(a: u32): u32 {
        return a >> 1;
    }
    "#;
    let (amir, symbols) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols).unwrap();

    let result: u32 = unsafe {
        let f: unsafe fn(u32) -> u32 = module.get_fn("shr").unwrap();
        f(4_294_967_295)
    };
    // Logical shift: 0xFFFF_FFFF >> 1 = 0x7FFF_FFFF
    assert_eq!(result, 2_147_483_647);
}
