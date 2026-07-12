//! T2.2: implicit enum / Result / Option variant sugar (`.Ok`, `.None`, …).
#![allow(clippy::unwrap_used)]
use arandu_query::db::DatabaseImpl;
use arandu_query::passes::type_check;

#[test]
fn sugar_option_none_return() {
    let mut db = DatabaseImpl::default();
    let f = db.new_file(
        "t.aru".to_string(),
        r#"
            func f(): Option<int> {
                return .None
            }
        "#
        .to_string(),
    );
    let tc = type_check(&db, f);
    assert!(
        tc.diagnostics.is_empty(),
        "expected .None with return type Option, got {:?}",
        tc.diagnostics
    );
}

#[test]
fn sugar_option_some_let() {
    let mut db = DatabaseImpl::default();
    let f = db.new_file(
        "t.aru".to_string(),
        r#"
            func f(): int {
                let x: Option<int> = .Some(42)
                return 0
            }
        "#
        .to_string(),
    );
    let tc = type_check(&db, f);
    assert!(
        tc.diagnostics.is_empty(),
        "expected .Some with annotation, got {:?}",
        tc.diagnostics
    );
}

#[test]
fn sugar_result_ok_return() {
    let mut db = DatabaseImpl::default();
    let f = db.new_file(
        "t.aru".to_string(),
        r#"
            func f(): Result<int, Err> {
                return .Ok(1)
            }
        "#
        .to_string(),
    );
    let tc = type_check(&db, f);
    assert!(
        tc.diagnostics.is_empty(),
        "expected .Ok with Result return, got {:?}",
        tc.diagnostics
    );
}

#[test]
fn sugar_without_expected_is_error() {
    let mut db = DatabaseImpl::default();
    let f = db.new_file(
        "t.aru".to_string(),
        r#"
            func f(): int {
                let x = .None
                return 0
            }
        "#
        .to_string(),
    );
    let tc = type_check(&db, f);
    assert!(
        tc.diagnostics
            .iter()
            .any(|d| d.message.contains("expected type") || d.message.contains("variant sugar")),
        "bare .None without expected type must fail, got {:?}",
        tc.diagnostics
    );
}

#[test]
fn sugar_wrong_variant_for_option() {
    let mut db = DatabaseImpl::default();
    let f = db.new_file(
        "t.aru".to_string(),
        r#"
            func f(): Option<int> {
                return .Ok(1)
            }
        "#
        .to_string(),
    );
    let tc = type_check(&db, f);
    assert!(!tc.diagnostics.is_empty(), ".Ok is not valid for Option");
}
