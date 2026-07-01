use arandu_backend_cranelift::CraneliftBackend;
use arandu_semantics::literal_pool::AmirLiteralEntry;
use arandu_semantics::{DiagCode, lower_to_amir, lower_to_hir, resolve, type_check};

fn compile_src(
    src: &str,
) -> (
    arandu_semantics::amir::AmirProgram,
    arandu_semantics::SymbolTable,
    arandu_semantics::TypeInfo,
) {
    let program = arandu_parser::parse(src).expect("parse failed");
    let resolution = resolve(&program);
    let mut tc = type_check(resolution, &program);
    let hir = lower_to_hir(&mut tc, &program).expect("HIR lowering failed");
    let amir = lower_to_amir(&tc, &hir).expect("AMIR lowering failed");
    (amir, tc.symbols, tc.type_info)
}

#[test]
fn jit_constant_i32() {
    let src = "func main(): int { return 42; }";
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();

    let result: i32 = unsafe {
        let f: unsafe fn() -> i32 = module.get_fn("main").unwrap();
        f()
    };
    assert_eq!(result, 42);
}

#[test]
fn jit_add_i32() {
    let src = "func add(a: int, b: int): int { return a + b; }";
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();

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
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();

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
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();

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
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();

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
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();

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
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();

    let result: u32 = unsafe {
        let f: unsafe fn(u32) -> u32 = module.get_fn("shr").unwrap();
        f(4_294_967_295)
    };
    // Logical shift: 0xFFFF_FFFF >> 1 = 0x7FFF_FFFF
    assert_eq!(result, 2_147_483_647);
}

#[test]
fn jit_signed_div() {
    let src = r#"
    func div(a: int, b: int): int {
        return a / b;
    }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();

    let result: i32 = unsafe {
        let f: unsafe fn(i32, i32) -> i32 = module.get_fn("div").unwrap();
        f(-1, 2)
    };
    assert_eq!(result, 0);
}

#[test]
fn jit_signed_mod() {
    let src = r#"
    func rem(a: int, b: int): int {
        return a % b;
    }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();

    let result: i32 = unsafe {
        let f: unsafe fn(i32, i32) -> i32 = module.get_fn("rem").unwrap();
        f(-7, 3)
    };
    assert_eq!(result, -1);
}

#[test]
fn jit_signed_comparison() {
    let src = r#"
    func is_gt(a: int, b: int): bool {
        return a > b;
    }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();

    let result: bool = unsafe {
        let f: unsafe fn(i32, i32) -> bool = module.get_fn("is_gt").unwrap();
        f(-1, 0)
    };
    assert!(!result);
}

#[test]
fn jit_signed_shift_right() {
    let src = r#"
    func shr(a: int): int {
        return a >> 1;
    }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();

    let result: i32 = unsafe {
        let f: unsafe fn(i32) -> i32 = module.get_fn("shr").unwrap();
        f(-1)
    };
    // Arithmetic shift: -1 >> 1 = -1
    assert_eq!(result, -1);
}

#[test]
fn jit_float_add() {
    let src = "func add(a: float, b: float): float { return a + b; }";
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: f64 = unsafe {
        let f: unsafe fn(f64, f64) -> f64 = module.get_fn("add").unwrap();
        f(1.5, 2.5)
    };
    assert_eq!(result, 4.0);
}

#[test]
fn jit_float_mul() {
    let src = "func mul(a: float, b: float): float { return a * b; }";
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: f64 = unsafe {
        let f: unsafe fn(f64, f64) -> f64 = module.get_fn("mul").unwrap();
        f(3.0, 1.5)
    };
    assert_eq!(result, 4.5);
}

#[test]
fn jit_float_compare() {
    let src = "func is_gt(a: float, b: float): bool { return a > b; }";
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: bool = unsafe {
        let f: unsafe fn(f64, f64) -> bool = module.get_fn("is_gt").unwrap();
        f(3.0, 2.0)
    };
    assert!(result);
    let result: bool = unsafe {
        let f: unsafe fn(f64, f64) -> bool = module.get_fn("is_gt").unwrap();
        f(2.0, 3.0)
    };
    assert!(!result);
}

#[test]
fn jit_cross_function_call() {
    let src = r#"
    func helper(): int {
        return 42;
    }
    func main(): int {
        return helper();
    }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: i32 = unsafe {
        let f: unsafe fn() -> i32 = module.get_fn("main").unwrap();
        f()
    };
    assert_eq!(result, 42);
}

#[test]
fn jit_string_literal() {
    let src = r#"func hello(): str { return "hello jit"; }"#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: *const u8 = unsafe {
        let f: unsafe fn() -> *const u8 = module.get_fn("hello").unwrap();
        f()
    };
    assert!(!result.is_null());
}

#[test]
fn jit_struct_field_access() {
    let src = r#"
    struct Point {
        x: int
        y: int
    }
    func get_x(p: Point): int {
        return p.x
    }
    func get_y(p: Point): int {
        return p.y
    }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    #[repr(C)]
    struct Point {
        x: i64,
        y: i64,
    }
    let p = Point { x: 10, y: 20 };
    let result: i32 = unsafe {
        let f: unsafe fn(*const Point) -> i32 = module.get_fn("get_x").unwrap();
        f(&p as *const Point)
    };
    assert_eq!(result, 10);
    let result: i32 = unsafe {
        let f: unsafe fn(*const Point) -> i32 = module.get_fn("get_y").unwrap();
        f(&p as *const Point)
    };
    assert_eq!(result, 20);
}

#[test]
fn jit_sub_i32() {
    let src = "func sub(a: int, b: int): int { return a - b; }";
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: i32 = unsafe {
        let f: unsafe fn(i32, i32) -> i32 = module.get_fn("sub").unwrap();
        f(10, 3)
    };
    assert_eq!(result, 7);
}

#[test]
fn jit_neg_i32() {
    let src = "func neg(a: int): int { return -a; }";
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: i32 = unsafe {
        let f: unsafe fn(i32) -> i32 = module.get_fn("neg").unwrap();
        f(42)
    };
    assert_eq!(result, -42);
}

#[test]
fn jit_equality() {
    let src = r#"
    func eq(a: int, b: int): bool { return a == b; }
    func neq(a: int, b: int): bool { return a != b; }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: bool = unsafe {
        let f: unsafe fn(i32, i32) -> bool = module.get_fn("eq").unwrap();
        f(3, 3)
    };
    assert!(result);
    let result: bool = unsafe {
        let f: unsafe fn(i32, i32) -> bool = module.get_fn("eq").unwrap();
        f(3, 4)
    };
    assert!(!result);
    let result: bool = unsafe {
        let f: unsafe fn(i32, i32) -> bool = module.get_fn("neq").unwrap();
        f(3, 4)
    };
    assert!(result);
}

#[test]
fn jit_less_than() {
    let src = r#"
    func lt(a: int, b: int): bool { return a < b; }
    func lte(a: int, b: int): bool { return a <= b; }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: bool = unsafe {
        let f: unsafe fn(i32, i32) -> bool = module.get_fn("lt").unwrap();
        f(2, 3)
    };
    assert!(result);
    let result: bool = unsafe {
        let f: unsafe fn(i32, i32) -> bool = module.get_fn("lt").unwrap();
        f(3, 2)
    };
    assert!(!result);
    let result: bool = unsafe {
        let f: unsafe fn(i32, i32) -> bool = module.get_fn("lte").unwrap();
        f(3, 3)
    };
    assert!(result);
}

#[test]
fn jit_greater_equal() {
    let src = r#"
    func gte(a: int, b: int): bool { return a >= b; }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: bool = unsafe {
        let f: unsafe fn(i32, i32) -> bool = module.get_fn("gte").unwrap();
        f(5, 3)
    };
    assert!(result);
    let result: bool = unsafe {
        let f: unsafe fn(i32, i32) -> bool = module.get_fn("gte").unwrap();
        f(3, 5)
    };
    assert!(!result);
    let result: bool = unsafe {
        let f: unsafe fn(i32, i32) -> bool = module.get_fn("gte").unwrap();
        f(3, 3)
    };
    assert!(result);
}

#[test]
fn jit_logical_not() {
    let src = r#"
    func not(a: bool): bool { return !a; }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: bool = unsafe {
        let f: unsafe fn(bool) -> bool = module.get_fn("not").unwrap();
        f(true)
    };
    assert!(!result);
    let result: bool = unsafe {
        let f: unsafe fn(bool) -> bool = module.get_fn("not").unwrap();
        f(false)
    };
    assert!(result);
}

#[test]
fn jit_logical_or_and() {
    let src = r#"
    func or(a: bool, b: bool): bool { return a || b; }
    func and(a: bool, b: bool): bool { return a && b; }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: bool = unsafe {
        let f: unsafe fn(bool, bool) -> bool = module.get_fn("or").unwrap();
        f(true, false)
    };
    assert!(result);
    let result: bool = unsafe {
        let f: unsafe fn(bool, bool) -> bool = module.get_fn("or").unwrap();
        f(false, false)
    };
    assert!(!result);
    let result: bool = unsafe {
        let f: unsafe fn(bool, bool) -> bool = module.get_fn("and").unwrap();
        f(true, true)
    };
    assert!(result);
    let result: bool = unsafe {
        let f: unsafe fn(bool, bool) -> bool = module.get_fn("and").unwrap();
        f(true, false)
    };
    assert!(!result);
}

#[test]
fn jit_bitwise_and_or_xor() {
    let src = r#"
    func band(a: int, b: int): int { return a & b; }
    func bor(a: int, b: int): int { return a | b; }
    func bxor(a: int, b: int): int { return a ^ b; }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: i32 = unsafe {
        let f: unsafe fn(i32, i32) -> i32 = module.get_fn("band").unwrap();
        f(0xFF, 0x0F)
    };
    assert_eq!(result, 0x0F);
    let result: i32 = unsafe {
        let f: unsafe fn(i32, i32) -> i32 = module.get_fn("bor").unwrap();
        f(0xF0, 0x0F)
    };
    assert_eq!(result, 0xFF);
    let result: i32 = unsafe {
        let f: unsafe fn(i32, i32) -> i32 = module.get_fn("bxor").unwrap();
        f(0xFF, 0x0F)
    };
    assert_eq!(result, 0xF0);
}

#[test]
fn jit_bitwise_not() {
    let src = "func bnot(a: int): int { return ~a; }";
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: i32 = unsafe {
        let f: unsafe fn(i32) -> i32 = module.get_fn("bnot").unwrap();
        f(0x0F)
    };
    assert_eq!(result, !0x0F);
}

#[test]
fn jit_int_match() {
    let src = r#"
    func classify(x: int): int {
        return match x {
            1 => 10
            2 => 20
            _ => 30
        }
    }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: i32 = unsafe {
        let f: unsafe fn(i32) -> i32 = module.get_fn("classify").unwrap();
        f(1)
    };
    assert_eq!(result, 10);
    let result: i32 = unsafe {
        let f: unsafe fn(i32) -> i32 = module.get_fn("classify").unwrap();
        f(2)
    };
    assert_eq!(result, 20);
    let result: i32 = unsafe {
        let f: unsafe fn(i32) -> i32 = module.get_fn("classify").unwrap();
        f(99)
    };
    assert_eq!(result, 30);
}

// FIXME: implement AmirRvalue::Discriminant + EnumPayload in crates/arandu_backend_cranelift/src/translator/expr.rs
// #[test]
// fn jit_enum_match() {
//     let src = r#"
//     enum Color {
//         Red,
//         Green,
//         Blue,
//     }
//     func pick(c: Color): int {
//         return match c {
//             Color.Red => 1
//             Color.Green => 2
//             Color.Blue => 3
//         }
//     }
//     "#;
//     let (amir, symbols, type_info) = compile_src(src);
//     let backend = CraneliftBackend::new();
//     let module = backend.compile(&amir, &symbols, &type_info).unwrap();
//     let result: i32 = unsafe {
//         let f: unsafe fn(i64) -> i32 = module.get_fn("pick").unwrap();
//         f(0)
//     };
//     assert_eq!(result, 1);
// }

// FIXME: implement AmirRvalue::IndexAccess in crates/arandu_backend_cranelift/src/translator/expr.rs
// #[test]
// fn jit_index_access() {
//     let src = r#"
//     func get(a: []int, i: int): int {
//         return a[i];
//     }
//     "#;
// }

// FIXME: implement AmirRvalue::Array in crates/arandu_backend_cranelift/src/translator/expr.rs
// #[test]
// fn jit_array_literal() {
//     let src = r#"
//     func first(): int {
//         let a = [1, 2, 3];
//         return a[0];
//     }
//     "#;
// }

// FIXME: implement AmirRvalue::Tuple in crates/arandu_backend_cranelift/src/translator/expr.rs
// #[test]
// fn jit_tuple() {
//     let src = r#"
//     func pair(): (int, bool) {
//         return (42, true);
//     }
//     "#;
// }

// FIXME: implement AmirRvalue::Borrow/BorrowMut in crates/arandu_backend_cranelift/src/translator/expr.rs
// #[test]
// fn jit_borrow() {
//     let src = r#"
//     func inc(p: ptr[int]): void {
//         *p += 1;
//     }
//     "#;
// }

// FIXME: implement BinaryOp::Mod for floats in crates/arandu_backend_cranelift/src/translator/compare.rs
// #[test]
// fn jit_float_mod() {
//     let src = "func rem(a: float, b: float): float { return a % b; }";
// }

// FIXME: implement UnaryOp::Await in crates/arandu_backend_cranelift/src/translator/expr.rs
// #[test]
// fn jit_await() {}

// FIXME: AmirRvalue::StructLiteral needs malloc declared as extern in the program;
//        backend also has ptr_type vs uint mismatch for the size parameter
// #[test]
// fn jit_struct_literal() {
//     let src = r#"
//     struct Point { x: int, y: int }
//     func make(): Point { return Point { x: 1, y: 2 }; }
//     "#;
// }

#[test]
fn jit_returns_ice_on_invalid_literal_pool() {
    let (mut amir, symbols, type_info) = compile_src("func main(): int { return 42; }");
    for entry in &mut amir.literal_pool.entries {
        if let AmirLiteralEntry::Int(value) = entry {
            *value = "not_an_int".to_string();
            break;
        }
    }

    let backend = CraneliftBackend::new();
    let err = match backend.compile(&amir, &symbols, &type_info) {
        Err(err) => err,
        Ok(_) => panic!("expected codegen ICE for invalid literal pool"),
    };
    assert_eq!(err.code, DiagCode::ICEGEN001);
    assert!(
        err.message.contains("invalid integer literal"),
        "unexpected ICE message: {}",
        err.message
    );
}
