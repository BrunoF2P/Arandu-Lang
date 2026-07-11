//! W5: indirect calls must be rejected at typeck (T033), not only in JIT.
#![allow(clippy::unwrap_used)]
use arandu_query::db::DatabaseImpl;
use arandu_query::passes::type_check;

#[test]
fn function_value_call_is_t033() {
    let mut db = DatabaseImpl::default();
    let f = db.new_file(
        "ind.aru".to_string(),
        r#"
            func add(a: int, b: int): int { return a + b }
            func main(): int {
                let f = add
                return f(1, 2)
            }
        "#
        .to_string(),
    );
    let tc = type_check(&db, f);
    assert!(
        tc.diagnostics
            .iter()
            .any(|d| d.message.contains("indirect") || format!("{:?}", d.code).contains("T033")),
        "expected T033 indirect call, got {:?}",
        tc.diagnostics
    );
}
