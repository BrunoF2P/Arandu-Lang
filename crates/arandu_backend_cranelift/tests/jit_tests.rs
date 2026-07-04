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

#[test]
fn jit_enum_match() {
    let src = r#"
    enum Color {
        Red,
        Green,
        Blue,
    }
    func pick(c: Color): int {
        return match c {
            Color.Red => 1
            Color.Green => 2
            Color.Blue => 3
        }
    }
    func test_red(): int {
        return pick(Color.Red);
    }
    func test_green(): int {
        return pick(Color.Green);
    }
    func test_blue(): int {
        return pick(Color.Blue);
    }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result_red: i32 = unsafe {
        let f: unsafe fn() -> i32 = module.get_fn("test_red").unwrap();
        f()
    };
    assert_eq!(result_red, 1);
    let result_green: i32 = unsafe {
        let f: unsafe fn() -> i32 = module.get_fn("test_green").unwrap();
        f()
    };
    assert_eq!(result_green, 2);
    let result_blue: i32 = unsafe {
        let f: unsafe fn() -> i32 = module.get_fn("test_blue").unwrap();
        f()
    };
    assert_eq!(result_blue, 3);
}

#[test]
fn jit_enum_none_payload_never_read() {
    let src = r#"
    enum MaybeInt {
        None,
        Some(int),
    }
    func get_value(m: MaybeInt): int {
        return match m {
            MaybeInt.None => 0
            MaybeInt.Some(val) => val
        }
    }
    func run_loop(n: int): int {
        let mut i = 0;
        let mut sum = 0;
        while i < n {
            let m = MaybeInt.None;
            sum = sum + get_value(m);
            i = i + 1;
        }
        return sum;
    }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let result: i32 = unsafe {
        let f: unsafe fn(i32) -> i32 = module.get_fn("run_loop").unwrap();
        f(1000)
    };
    assert_eq!(result, 0);
}

#[test]
fn jit_tuple() {
    let src = r#"
    func pair(): (int, bool) {
        return 42, true;
    }
    func get_first(): int {
        let x, y = pair();
        return x;
    }
    func get_second(): bool {
        let x, y = pair();
        return y;
    }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let first: i32 = unsafe {
        let f: unsafe fn() -> i32 = module.get_fn("get_first").unwrap();
        f()
    };
    assert_eq!(first, 42);
    let second: bool = unsafe {
        let f: unsafe fn() -> bool = module.get_fn("get_second").unwrap();
        f()
    };
    assert!(second);
}

#[test]
fn jit_struct_literal() {
    let src = r#"
    struct Point {
        x: int
        y: int
    }
    func get_sum(): int {
        let p = Point { x: 10, y: 20 };
        return p.x + p.y;
    }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();
    let sum: i32 = unsafe {
        let f: unsafe fn() -> i32 = module.get_fn("get_sum").unwrap();
        f()
    };
    assert_eq!(sum, 30);
}

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

/// Regression test: two enums sharing a variant name must not collide
/// on their discriminant tags.
///
/// ## What this guards against
///
/// The variant resolution fallback used to scan `enum_variant_tags` globally
/// by name — so `Color.Red` and `Status.Red` could silently resolve to
/// whichever `SymbolId` the hashmap returned first (non-deterministic with
/// standard HashMap; consistent-but-wrong with FxHashMap since it does not
/// randomize its seed per process, so a collision would mask itself in CI).
///
/// The fix registers both the definition-site SymbolId ("Red") *and* the
/// associated-member SymbolId ("Color.Red") in `enum_variant_tags` during
/// `collect_type_shapes`, so the direct `.get(symbol)` hit never falls
/// through to the name-based global scan.
///
/// ## Why this test is deterministic
///
/// Rather than relying on iteration order to expose the bug, the test
/// encodes the expected discriminant as the JIT return value and asserts
/// it numerically.  A regression produces tag 1 (the wrong variant) instead
/// of 0, failing the assert regardless of hashmap ordering.
#[test]
fn jit_enum_cross_variant_name_no_collision() {
    // Two enums with identically-named variants.
    // color_tag() and status_tag() each return the discriminant of their
    // respective ".Red" variant encoded as an integer via match.
    //
    // Before the fix, the global name-based fallback scan would silently
    // assign the wrong discriminant when FxHashMap happened to return the
    // other enum's "Red" first.  The test is deterministic: a regression
    // produces 1 instead of 0, failing the assert regardless of hash order.
    let src = r#"
        enum Color  { Red, Green }
        enum Status { Yellow, Red }

        func color_tag() : int {
            return match Color.Red {
                Color.Red   => 0
                Color.Green => 1
            }
        }

        func status_tag() : int {
            return match Status.Red {
                Status.Yellow => 0
                Status.Red    => 1
            }
        }
    "#;
    let (amir, symbols, type_info) = compile_src(src);
    let backend = CraneliftBackend::new();
    let module = backend.compile(&amir, &symbols, &type_info).unwrap();

    let color_tag: i32 = unsafe {
        let f: unsafe fn() -> i32 = module.get_fn("color_tag").unwrap();
        f()
    };
    let status_tag: i32 = unsafe {
        let f: unsafe fn() -> i32 = module.get_fn("status_tag").unwrap();
        f()
    };

    // Color.Red is declared first in Color → match arm 0.
    // Status.Red is declared SECOND in Status (Yellow first) → match arm 1.
    // The asymmetry is intentional: if the bug regresses and Color.Red is
    // resolved using Status.Red's discriminant (1) or vice-versa (0),
    // the wrong arm fires and the assert catches it — regardless of which
    // direction the cross-enum lookup goes and regardless of hashmap ordering.
    assert_eq!(color_tag, 0, "Color.Red must match arm 0 (tag 0 in Color)");
    assert_eq!(status_tag, 1, "Status.Red must match arm 1 (tag 1 in Status, Yellow is 0)");
}
