//! Regression: empty qualified struct lit `lib.Type {}` must typecheck (not trailing-block Call).
#![allow(clippy::unwrap_used)]
use arandu_query::db::DatabaseImpl;
use arandu_query::passes::{resolve, type_check};

#[test]
fn empty_struct_cross_module_method() {
    let mut db = DatabaseImpl::default();
    let _lib = db.new_file(
        "lib.aru".to_string(),
        r#"
            public struct Widget {}
            public func Widget.ok(shared self): int { return 42 }
        "#
        .to_string(),
    );
    let main = db.new_file(
        "main.aru".to_string(),
        r#"
            import lib
            func main(): int {
                let b = lib.Widget {}
                return b.ok()
            }
        "#
        .to_string(),
    );
    let r = resolve(&db, main);
    assert!(
        !r.resolved.type_refs.is_empty(),
        "struct lit type must be resolved, type_refs empty; diags={:?}",
        r.diagnostics
    );
    let tc = type_check(&db, main);
    assert!(
        tc.diagnostics.is_empty(),
        "empty multi-file struct lit + method must typecheck, got: {:?}",
        tc.diagnostics
    );
    // No Error-typed exprs
    for t in tc.type_info.expr_types.iter().flatten() {
        let ty = tc.type_info.type_interner.resolve(*t);
        assert!(
            !matches!(ty, arandu_middle::types::ArType::Error),
            "unexpected Error expr type"
        );
    }
}
