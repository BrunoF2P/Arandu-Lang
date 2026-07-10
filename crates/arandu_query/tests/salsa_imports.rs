use arandu_query::DatabaseImpl;
use salsa::Setter;

/// Regression: builtin prelude (`import io` / `import err`) must resolve on the
/// Salsa/CLI path without requiring on-disk `io.aru` / `err.aru` files.
#[test]
fn test_prelude_import_io_err_without_files() {
    let mut db = DatabaseImpl::default();
    let src = r#"
        module tests.prelude_import

        import io
        import err

        func main() {
            let msg = err.new("x")
            io.println("ok")
        }
    "#;
    let file = db.new_file("tests_prelude_import.aru".to_string(), src.to_string());

    let resolved = arandu_query::passes::resolve(&db, file);
    let has_m001 = resolved
        .diagnostics
        .iter()
        .any(|d| matches!(d.code, arandu_middle::DiagCode::M001UnresolvedImport));
    assert!(
        !has_m001,
        "prelude import must not emit M001, got: {:?}",
        resolved.diagnostics
    );

    let tc = arandu_query::passes::type_check(&db, file);
    assert!(
        tc.diagnostics.is_empty(),
        "type check with import io/err should succeed, got: {:?}",
        tc.diagnostics
    );
}

#[test]
fn test_prelude_import_io_as_alias() {
    let mut db = DatabaseImpl::default();
    let src = r#"
        module tests.prelude_alias

        import io as out

        func main() {
            out.println("hi")
        }
    "#;
    let file = db.new_file("tests_prelude_alias.aru".to_string(), src.to_string());
    let tc = arandu_query::passes::type_check(&db, file);
    assert!(
        tc.diagnostics.is_empty(),
        "import io as out should resolve prelude members, got: {:?}",
        tc.diagnostics
    );
}

#[test]
fn test_cross_file_type_check() {
    let mut db = DatabaseImpl::default();
    let mod_b_text = r#"
        func add(a: int, b: int): int {
            return a + b
        }
    "#;
    let _mod_b = db.new_file("mod_b.aru".to_string(), mod_b_text.to_string());

    let mod_a_text = r#"
        import mod_b
        func main(): int {
            return mod_b.add(10, 20)
        }
    "#;
    let mod_a = db.new_file("mod_a.aru".to_string(), mod_a_text.to_string());

    let tc_a = arandu_query::passes::type_check(&db, mod_a);
    assert!(
        tc_a.diagnostics.is_empty(),
        "Expected no diagnostics, got: {:?}",
        tc_a.diagnostics
    );
}

#[test]
fn test_early_cutoff_on_function_body_change() {
    let mut db = DatabaseImpl::default();

    // We create a base file mod_b
    let mod_b_text = "func add(a: int, b: int): int {\n return a + b\n }";
    let mod_b = db.new_file("mod_b.aru".to_string(), mod_b_text.to_string());

    // And mod_a depends on mod_b
    let mod_a_text = "import mod_b\nfunc main(): int {\n return mod_b.add(1, 2)\n }";
    let mod_a = db.new_file("mod_a.aru".to_string(), mod_a_text.to_string());

    // 1. Evaluate mod_a typecheck.
    let tc1 = arandu_query::passes::type_check(&db, mod_a);
    assert!(tc1.diagnostics.is_empty());

    // 2. Change the body of mod_b but keep the signature the same
    let mod_b_text_new = "func add(a: int, b: int): int {\n let c = a\n return c + b\n }";
    mod_b
        .set_text(&mut db)
        .to(std::sync::Arc::from(mod_b_text_new));

    // 3. Re-evaluate mod_a typecheck.
    let tc2 = arandu_query::passes::type_check(&db, mod_a);
    assert!(tc2.diagnostics.is_empty());
}

#[test]
fn test_cross_file_collision_during_circular_import_is_still_deterministic() {
    let mut db = DatabaseImpl::default();
    // circular dependency: A imports B, B imports A
    let mod_a_text = r#"
        import mod_b
        func foo() {
            mod_b.bar()
        }
    "#;
    let mod_b_text = r#"
        import mod_a
        func bar() {
            mod_a.foo()
        }
    "#;
    let mod_a = db.new_file("mod_a.aru".to_string(), mod_a_text.to_string());
    let _mod_b = db.new_file("mod_b.aru".to_string(), mod_b_text.to_string());

    let tc_a = arandu_query::passes::type_check(&db, mod_a);
    let has_cycle_error = tc_a
        .diagnostics
        .iter()
        .any(|d| d.message.contains("cyclic"));
    if !has_cycle_error {
        panic!("Expected a cycle error or unresolved type, got no diagnostics");
    }
}
