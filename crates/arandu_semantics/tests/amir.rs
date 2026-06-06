use arandu_semantics::amir::{AmirProjection, AmirStmt};
use arandu_semantics::{
    DiagCode, SymbolKind, lower_to_amir, lower_to_hir, resolve, type_check, validate_amir_program,
};

#[test]
fn test_amir_golden_files() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root_dir = std::path::Path::new(&manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let fixtures_dir = root_dir.join("tests").join("amir");

    if !fixtures_dir.exists() {
        // No fixtures directory = nothing to test
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
            panic!("failed to parse {name}: {err:?}");
        });
        let resolution = resolve(&program);
        let tc = type_check(resolution, &program);
        let errors: Vec<_> = tc
            .diagnostics
            .iter()
            .filter(|d| d.severity == arandu_semantics::Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "type check failed for {name}: {errors:?}"
        );
        let hir = lower_to_hir(&tc, &program).expect("HIR lowering failed");
        hir.validate_invariants(&hir.pool, &tc.symbols)
            .expect("HIR invariant validation failed");
        let amir = lower_to_amir(&tc, &hir).expect("AMIR lowering failed");
        let amir_issues = validate_amir_program(&amir, &tc.symbols);
        assert!(
            amir_issues.is_empty(),
            "AMIR validation failed for {name}: {amir_issues:?}"
        );
        let pretty = amir.pretty_print(&tc.symbols);

        let golden_path = fixtures_dir.join(format!("{name}.amir"));
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
                "AMIR mismatch for {name}. Run with UPDATE_GOLDEN=1 to update."
            );
        }
    }
}

#[test]
fn field_projection_uses_field_symbol_id() {
    let src = r#"
struct Point {
    x int
    y int
}

func main() {
    p Point = Point { x: 1, y: 2 }
    set p.x = 3
}
"#;
    let program = arandu_parser::parse(src).expect("parse failed");
    let resolution = resolve(&program);
    let tc = type_check(resolution, &program);
    let x_symbol = tc
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Field && symbol.name == "x")
        .map(|symbol| symbol.id)
        .expect("missing field symbol");
    let hir = lower_to_hir(&tc, &program).expect("HIR lowering failed");
    let amir = lower_to_amir(&tc, &hir).expect("AMIR lowering failed");

    let has_symbol_projection = amir.funcs[0].blocks.iter().any(|block| {
        block.statements.iter().any(|stmt| match stmt {
            AmirStmt::Store { lhs, .. } => lhs
                .projections
                .iter()
                .any(|projection| matches!(projection, AmirProjection::Field(symbol) if *symbol == x_symbol)),
            _ => false,
        })
    });
    assert!(
        has_symbol_projection,
        "expected p.x store to use field SymbolId"
    );
}

#[test]
fn non_copy_local_use_after_move_fails_during_amir_analysis() {
    let src = r#"
struct Boxed {
    value int
}

func main() {
    a Boxed = Boxed { value: 1 }
    b Boxed = a
    c Boxed = a
}
"#;
    let program = arandu_parser::parse(src).expect("parse failed");
    let resolution = resolve(&program);
    let tc = type_check(resolution, &program);
    let hir = lower_to_hir(&tc, &program).expect("HIR lowering failed");
    let diagnostics = lower_to_amir(&tc, &hir).expect_err("expected use after move diagnostic");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagCode::O001UseAfterMove),
        "expected O001 use-after-move diagnostic, got {diagnostics:?}"
    );
}

#[test]
fn copy_local_can_be_reused_during_amir_lowering() {
    let src = r#"
func main() {
    a int = 1
    b int = a
    c int = a
}
"#;
    let program = arandu_parser::parse(src).expect("parse failed");
    let resolution = resolve(&program);
    let tc = type_check(resolution, &program);
    let hir = lower_to_hir(&tc, &program).expect("HIR lowering failed");
    let amir = lower_to_amir(&tc, &hir).expect("AMIR lowering failed");
    let pretty = amir.pretty_print(&tc.symbols);
    assert!(
        !pretty.contains("move _"),
        "copy types must not emit move operands"
    );
}

#[test]
fn branch_move_mismatch_reports_o007() {
    let src = r#"
struct Boxed {
    value int
}

func main(cond bool) {
    a Boxed = Boxed { value: 1 }
    if cond {
        b Boxed = a
    }
    c Boxed = a
}
"#;
    let program = arandu_parser::parse(src).expect("parse failed");
    let resolution = resolve(&program);
    let tc = type_check(resolution, &program);
    let hir = lower_to_hir(&tc, &program).expect("HIR lowering failed");
    let diagnostics = lower_to_amir(&tc, &hir).expect_err("expected branch move diagnostic");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagCode::O007InconsistentMoveBetweenBranches),
        "expected O007 inconsistent move diagnostic, got {diagnostics:?}"
    );
}
