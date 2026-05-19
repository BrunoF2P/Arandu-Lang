use arandu_semantics::hir::*;
use arandu_semantics::passes::type_checker::types::ArType;
use arandu_semantics::{SymbolTable, lower_to_hir, resolve, type_check};

fn lower(src: &str) -> (HirProgram, SymbolTable) {
    let program = arandu_parser::parse(src).expect("parse failed");
    let resolution = resolve(&program);
    let tc = type_check(resolution, &program);
    assert!(
        tc.diagnostics.is_empty(),
        "type check errors: {:?}",
        tc.diagnostics
    );
    let hir = lower_to_hir(&tc, &program).expect("HIR lowering failed");
    hir.validate_invariants(&tc.symbols)
        .expect("HIR invariant validation failed");
    (hir, tc.symbols)
}

// ── Struct lowering ─────────────────────────────────────────────────

#[test]
fn lowers_struct_fields_with_types() {
    let (hir, symbols) = lower(
        "struct Point {
            x int
            y int
        }",
    );
    assert_eq!(hir.decls.len(), 1);
    match &hir.decls[0] {
        HirDecl::Struct(s) => {
            assert_eq!(symbol_name(&symbols, s.symbol), "Point");
            assert_eq!(s.fields.len(), 2);
            assert_eq!(symbol_name(&symbols, s.fields[0].symbol), "x");
            assert!(matches!(s.fields[0].ty, ArType::Primitive(_)));
            assert_eq!(symbol_name(&symbols, s.fields[1].symbol), "y");
        }
        other => panic!("expected Struct, got {other:?}"),
    }
}

// ── Function lowering ───────────────────────────────────────────────

#[test]
fn lowers_function_with_params_and_return_type() {
    let (hir, symbols) = lower(
        "func add(x int, y int) int {
            return x + y
        }",
    );
    assert_eq!(hir.decls.len(), 1);
    match &hir.decls[0] {
        HirDecl::Func(f) => {
            assert_eq!(symbol_name(&symbols, f.symbol), "add");
            assert_eq!(f.params.len(), 2);
            assert_eq!(symbol_name(&symbols, f.params[0].symbol), "x");
            assert_eq!(symbol_name(&symbols, f.params[1].symbol), "y");
            // return_type is extracted from ArType::Func(params, ret)
            assert!(
                matches!(f.return_type, ArType::Primitive(_)),
                "expected Primitive return type, got {:?}",
                f.return_type
            );
            assert!(f.body.is_some());
            let body = f.body.as_ref().unwrap();
            assert!(!body.statements.is_empty());
            match &body.statements[0].kind {
                HirStmtKind::Return { values } => {
                    assert_eq!(values.len(), 1);
                    assert!(matches!(values[0].kind, HirExprKind::Binary { .. }));
                }
                other => panic!("expected Return, got {other:?}"),
            }
        }
        other => panic!("expected Func, got {other:?}"),
    }
}

// ── Variable declarations ───────────────────────────────────────────

#[test]
fn lowers_var_decl_with_type_annotation() {
    let (hir, symbols) = lower(
        "func main() {
            x int = 42
        }",
    );
    match &hir.decls[0] {
        HirDecl::Func(f) => {
            let body = f.body.as_ref().unwrap();
            match &body.statements[0].kind {
                HirStmtKind::VarDecl { bindings, value } => {
                    assert_eq!(bindings.len(), 1);
                    assert_eq!(symbol_name(&symbols, bindings[0].symbol), "x");
                    assert!(
                        !matches!(bindings[0].ty, ArType::Error),
                        "expected resolved type, got Error"
                    );
                    match &value.kind {
                        HirExprKind::Int(v) => assert_eq!(v, "42"),
                        other => panic!("expected Int, got {other:?}"),
                    }
                }
                other => panic!("expected VarDecl, got {other:?}"),
            }
        }
        other => panic!("expected Func, got {other:?}"),
    }
}

// ── Expressions carry types ─────────────────────────────────────────

#[test]
fn expr_nodes_carry_resolved_types() {
    let (hir, _symbols) = lower(
        "func main() {
            x int = 10
            y int = x + 5
        }",
    );
    match &hir.decls[0] {
        HirDecl::Func(f) => {
            let body = f.body.as_ref().unwrap();
            match &body.statements[1].kind {
                HirStmtKind::VarDecl { value, .. } => {
                    assert!(
                        !matches!(value.ty, ArType::Error),
                        "binary expr should have a resolved type"
                    );
                }
                other => panic!("expected VarDecl, got {other:?}"),
            }
        }
        other => panic!("expected Func, got {other:?}"),
    }
}

// ── Path expressions resolve to symbols ─────────────────────────────

#[test]
fn path_expr_resolves_symbol() {
    let (hir, _symbols) = lower(
        "func main() {
            x int = 10
            y int = x
        }",
    );
    match &hir.decls[0] {
        HirDecl::Func(f) => {
            let body = f.body.as_ref().unwrap();
            match &body.statements[1].kind {
                HirStmtKind::VarDecl { value, .. } => match &value.kind {
                    HirExprKind::Path { symbol } => {
                        assert!(symbol.0 != 0, "symbol should be resolved");
                    }
                    other => panic!("expected Path, got {other:?}"),
                },
                other => panic!("expected VarDecl, got {other:?}"),
            }
        }
        other => panic!("expected Func, got {other:?}"),
    }
}

// ── Call expressions ────────────────────────────────────────────────

#[test]
fn lowers_call_expression() {
    let (hir, _symbols) = lower(
        "func add(x int, y int) int {
            return x + y
        }
        func main() {
            result int = add(1, 2)
        }",
    );
    match &hir.decls[1] {
        HirDecl::Func(f) => {
            let body = f.body.as_ref().unwrap();
            match &body.statements[0].kind {
                HirStmtKind::VarDecl { value, .. } => match &value.kind {
                    HirExprKind::Call { callee, args, .. } => {
                        assert!(matches!(callee.kind, HirExprKind::Path { .. }));
                        assert_eq!(args.len(), 2);
                    }
                    other => panic!("expected Call, got {other:?}"),
                },
                other => panic!("expected VarDecl, got {other:?}"),
            }
        }
        other => panic!("expected Func, got {other:?}"),
    }
}

// ── Enum lowering ───────────────────────────────────────────────────

#[test]
fn lowers_enum_with_variants() {
    let (hir, symbols) = lower(
        "enum Color {
            Red
            Green
            Blue
        }",
    );
    match &hir.decls[0] {
        HirDecl::Enum(e) => {
            assert_eq!(symbol_name(&symbols, e.symbol), "Color");
            assert_eq!(e.variants.len(), 3);
            assert_eq!(symbol_name(&symbols, e.variants[0].symbol), "Red");
            assert_eq!(symbol_name(&symbols, e.variants[1].symbol), "Green");
            assert_eq!(symbol_name(&symbols, e.variants[2].symbol), "Blue");
            assert!(e.variants[0].payload.is_none());
        }
        other => panic!("expected Enum, got {other:?}"),
    }
}

// ── If statement lowering ───────────────────────────────────────────

#[test]
fn lowers_if_stmt() {
    let (hir, _symbols) = lower(
        "func main() {
            x int = 10
            if x > 5 {
                y int = 1
            } else {
                y int = 2
            }
        }",
    );
    match &hir.decls[0] {
        HirDecl::Func(f) => {
            let body = f.body.as_ref().unwrap();
            match &body.statements[1].kind {
                HirStmtKind::If {
                    condition,
                    then_block,
                    else_block,
                } => {
                    assert!(matches!(condition, HirCondition::Expr(_)));
                    assert!(!then_block.statements.is_empty());
                    assert!(else_block.is_some());
                }
                other => panic!("expected If, got {other:?}"),
            }
        }
        other => panic!("expected Func, got {other:?}"),
    }
}

// ── Group expressions are transparent ───────────────────────────────

#[test]
fn group_expressions_unwrap() {
    let (hir, _symbols) = lower(
        "func main() {
            x int = (42)
        }",
    );
    match &hir.decls[0] {
        HirDecl::Func(f) => {
            let body = f.body.as_ref().unwrap();
            match &body.statements[0].kind {
                HirStmtKind::VarDecl { value, .. } => {
                    assert!(
                        matches!(value.kind, HirExprKind::Int(_)),
                        "expected Int after group unwrap, got {:?}",
                        value.kind
                    );
                }
                other => panic!("expected VarDecl, got {other:?}"),
            }
        }
        other => panic!("expected Func, got {other:?}"),
    }
}

// ── Const lowering ──────────────────────────────────────────────────

#[test]
fn lowers_const_declaration() {
    let (hir, symbols) = lower("const MAX int = 100");
    match &hir.decls[0] {
        HirDecl::Const(c) => {
            assert_eq!(symbol_name(&symbols, c.symbol), "MAX");
            // const type comes from synth_expr, which gives IntLiteral for unadorned int
            assert!(
                !matches!(c.ty, ArType::Error),
                "const type should be resolved, got {:?}",
                c.ty
            );
            assert!(matches!(c.value.kind, HirExprKind::Int(_)));
        }
        other => panic!("expected Const, got {other:?}"),
    }
}

// ── Struct literal lowering ─────────────────────────────────────────

#[test]
fn lowers_struct_literal() {
    let (hir, _symbols) = lower(
        "struct Point {
            x int
            y int
        }
        func main() {
            p Point = Point { x: 1, y: 2 }
        }",
    );
    match &hir.decls[1] {
        HirDecl::Func(f) => {
            let body = f.body.as_ref().unwrap();
            match &body.statements[0].kind {
                HirStmtKind::VarDecl { value, .. } => match &value.kind {
                    HirExprKind::StructLiteral {
                        struct_symbol,
                        fields,
                    } => {
                        assert!(struct_symbol.0 != 0, "struct symbol should be resolved");
                        assert_eq!(fields.len(), 2);
                        assert_eq!(fields[0].name, "x");
                        assert_eq!(fields[1].name, "y");
                    }
                    other => panic!("expected StructLiteral, got {other:?}"),
                },
                other => panic!("expected VarDecl, got {other:?}"),
            }
        }
        other => panic!("expected Func, got {other:?}"),
    }
}

// ── Symbol consistency ──────────────────────────────────────────────

#[test]
fn func_param_symbols_match_usage() {
    let (hir, _symbols) = lower(
        "func identity(x int) int {
            return x
        }",
    );
    match &hir.decls[0] {
        HirDecl::Func(f) => {
            let param_symbol = f.params[0].symbol;
            let body = f.body.as_ref().unwrap();
            match &body.statements[0].kind {
                HirStmtKind::Return { values } => match &values[0].kind {
                    HirExprKind::Path { symbol } => {
                        assert_eq!(
                            *symbol, param_symbol,
                            "usage symbol should match param definition"
                        );
                    }
                    other => panic!("expected Path, got {other:?}"),
                },
                other => panic!("expected Return, got {other:?}"),
            }
        }
        other => panic!("expected Func, got {other:?}"),
    }
}

// ── Golden Tests ───────────────────────────────────────────────────

#[test]
fn test_hir_golden_files() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root_dir = std::path::Path::new(&manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let fixtures_dir = root_dir.join("tests").join("hir");

    if !fixtures_dir.exists() {
        return;
    }

    let update_golden = std::env::var("UPDATE_GOLDEN").is_ok();

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&fixtures_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "aru") {
            entries.push(path);
        }
    }

    entries.sort();

    for path in entries {
        let name = path.file_stem().unwrap().to_str().unwrap();
        let src = std::fs::read_to_string(&path).unwrap();

        let program = arandu_parser::parse(&src).unwrap_or_else(|err| {
            panic!("failed to parse {}: {:?}", name, err);
        });
        let resolution = resolve(&program);
        let tc = type_check(resolution, &program);
        if !tc.diagnostics.is_empty() {
            panic!("type check failed for {}: {:?}", name, tc.diagnostics);
        }
        let hir = lower_to_hir(&tc, &program).expect("HIR lowering failed");
        hir.validate_invariants(&tc.symbols)
            .expect("HIR invariant validation failed");
        let ctx = HirPrettyCtx {
            symbols: &tc.symbols,
            show_spans: false,
        };
        let pretty = hir.pretty_print(&ctx);

        let golden_path = fixtures_dir.join(format!("{}.hir", name));
        if update_golden {
            std::fs::write(&golden_path, &pretty).unwrap();
        } else {
            assert!(
                golden_path.exists(),
                "Golden file missing for {name}. Run with UPDATE_GOLDEN=1 to create it."
            );
            let expected = std::fs::read_to_string(&golden_path).unwrap();
            let expected_normalized = expected.replace("\r\n", "\n");
            let pretty_normalized = pretty.replace("\r\n", "\n");
            assert_eq!(
                pretty_normalized, expected_normalized,
                "HIR mismatch for {}. Run with UPDATE_GOLDEN=1 to update.",
                name
            );
        }
    }
}
