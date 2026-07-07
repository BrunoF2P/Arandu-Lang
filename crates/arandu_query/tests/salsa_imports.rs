use arandu_query::DatabaseImpl;
use salsa::Setter;

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
