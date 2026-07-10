use arandu_middle::types::ArType;
use arandu_semantics::{lower_to_hir, monomorphize_program, resolve_for_test, type_check};

#[test]
fn generic_method_typechecks_and_monos() {
    let src = r#"
struct Holder {
    v: int
}

func Holder.id_val<T>(shared self, x: T): T {
    return x
}

func main(): int {
    let b = Holder { v: 10 }
    let n = b.id_val<int>(32)
    return n + b.v
}
"#;
    let program = arandu_parser::parse(src).expect("parse");
    let resolution = resolve_for_test(0, &program);
    let mut tc = type_check(resolution, &program);
    assert!(
        tc.diagnostics.is_empty(),
        "typeck diags: {:?}",
        tc.diagnostics
    );
    let mut hir = lower_to_hir(&mut tc, &program).expect("hir");
    hir.validate_invariants(&hir.pool, &tc.symbols)
        .expect("HIR invariants before mono");
    let n = monomorphize_program(&mut tc, &mut hir).expect("mono");
    assert!(n >= 1, "expected at least one specialization, got {n}");
    hir.validate_invariants(&hir.pool, &tc.symbols)
        .expect("HIR invariants after mono");
}

#[test]
fn generic_method_dual_int_str_specializations() {
    let src = r#"
struct Holder {
    v: int
}

func Holder.id_val<T>(shared self, x: T): T {
    return x
}

func main(): int {
    let b = Holder { v: 1 }
    let n = b.id_val<int>(41)
    let s = b.id_val<str>("hi")
    return n + 1
}
"#;
    let program = arandu_parser::parse(src).expect("parse");
    let resolution = resolve_for_test(0, &program);
    let mut tc = type_check(resolution, &program);
    assert!(
        tc.diagnostics.is_empty(),
        "typeck diags: {:?}",
        tc.diagnostics
    );
    let mut hir = lower_to_hir(&mut tc, &program).expect("hir");
    hir.validate_invariants(&hir.pool, &tc.symbols)
        .expect("HIR invariants before mono");
    let n = monomorphize_program(&mut tc, &mut hir).expect("mono");
    assert!(n >= 2, "expected int+str specializations, got {n}");
    hir.validate_invariants(&hir.pool, &tc.symbols)
        .expect("HIR invariants after mono");

    // Specialized methods appear as mangled free funcs (receiver already in params).
    let mut mangled = 0usize;
    for &did in &hir.decls {
        if let arandu_middle::hir::HirDecl::Func(f) = hir.pool.decl(did) {
            let name = tc.symbols.get(f.symbol).name.as_str();
            if name.starts_with("_A$") {
                mangled += 1;
                assert!(
                    !tc.type_info.generic_params.contains_key(&f.symbol),
                    "specialized `{name}` must not keep generic_params"
                );
                let ret = tc.type_info.type_interner.resolve(f.return_type);
                assert!(
                    matches!(
                        ret,
                        ArType::Primitive(_) | ArType::IntLiteral | ArType::FloatLiteral
                    ),
                    "expected concrete return on `{name}`, got {ret:?}"
                );
            }
        }
    }
    assert!(mangled >= 2, "expected >=2 mangled methods, got {mangled}");
}

#[test]
fn generic_method_lowers_to_amir_and_reuses_shared_receiver() {
    let src = r#"
struct Holder {
    v: int
}

func Holder.id_val<T>(shared self, x: T): T {
    return x
}

func main(): int {
    let b = Holder { v: 10 }
    let n = b.id_val<int>(32)
    return n + b.v
}
"#;
    let program = arandu_parser::parse(src).expect("parse");
    let resolution = resolve_for_test(0, &program);
    let mut tc = type_check(resolution, &program);
    assert!(tc.diagnostics.is_empty(), "{:?}", tc.diagnostics);
    let mut hir = lower_to_hir(&mut tc, &program).expect("hir");
    monomorphize_program(&mut tc, &mut hir).expect("mono");
    match arandu_semantics::lower_to_amir(&tc, &hir) {
        Ok(amir) => {
            assert!(!amir.funcs.is_empty(), "expected AMIR funcs");
            assert!(
                amir.funcs
                    .iter()
                    .any(|f| tc.symbols.get(f.symbol).name == "main"),
                "need main"
            );
            assert!(
                amir.funcs.iter().any(|f| {
                    let name = tc.symbols.get(f.symbol).name.as_str();
                    name.starts_with("_A$") && name.contains("id_val")
                }),
                "need mangled method specialization in AMIR"
            );
        }
        Err(diags) => panic!("lower_to_amir failed: {diags:?}"),
    }
}
