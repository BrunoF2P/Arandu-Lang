//! W0: `exported_symbols` must not leak private declarations across modules.
//!
//! Note: use non-empty structs in method fixtures — empty `lib.Widget {}` multi-file
//! struct-lit typing is a separate residual (receiver becomes Error).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use arandu_query::db::DatabaseImpl;
use arandu_query::passes::{exported_symbols, type_check};

#[test]
fn private_func_not_in_export_table() {
    let mut db = DatabaseImpl::default();
    let lib = db.new_file(
        "lib.aru".to_string(),
        r#"
            public func visible(): int { return 1 }
            func hidden(): int { return 2 }
        "#
        .to_string(),
    );
    let exports = exported_symbols(&db, lib);
    assert!(
        exports.symbols.contains_key("visible"),
        "public free func must export, got {:?}",
        exports.symbols.keys().collect::<Vec<_>>()
    );
    assert!(
        !exports.symbols.contains_key("hidden"),
        "private free func must NOT export, got {:?}",
        exports.symbols.keys().collect::<Vec<_>>()
    );
}

#[test]
fn private_method_not_callable_across_modules() {
    let mut db = DatabaseImpl::default();
    let _lib = db.new_file(
        "lib.aru".to_string(),
        r#"
            public struct Widget { x: int }
            public func Widget.ok(shared self): int { return self.x }
            func Widget.secret(shared self): int { return self.x }
        "#
        .to_string(),
    );
    let main = db.new_file(
        "main.aru".to_string(),
        r#"
            import lib
            func main(): int {
                let b = lib.Widget { x: 1 }
                return b.secret()
            }
        "#
        .to_string(),
    );
    let tc = type_check(&db, main);
    assert!(
        tc.diagnostics.iter().any(|d| d.message.contains("secret")
            || d.message.contains("no method")
            || d.message.contains("no field")),
        "calling private associated method across modules must fail, got: {:?}",
        tc.diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn public_method_callable_across_modules() {
    let mut db = DatabaseImpl::default();
    let _lib = db.new_file(
        "lib.aru".to_string(),
        r#"
            public struct Widget { x: int }
            public func Widget.ok(shared self): int { return self.x }
        "#
        .to_string(),
    );
    let main = db.new_file(
        "main.aru".to_string(),
        r#"
            import lib
            func main(): int {
                let b = lib.Widget { x: 42 }
                return b.ok()
            }
        "#
        .to_string(),
    );
    let tc = type_check(&db, main);
    assert!(
        tc.diagnostics.is_empty(),
        "public method should resolve across modules, got: {:?}",
        tc.diagnostics
    );
}

#[test]
fn named_import_of_private_is_error() {
    let mut db = DatabaseImpl::default();
    let _lib = db.new_file(
        "lib.aru".to_string(),
        r#"
            func hidden(): int { return 0 }
        "#
        .to_string(),
    );
    let main = db.new_file(
        "main.aru".to_string(),
        r#"
            import lib { hidden }
            func main(): int { return hidden() }
        "#
        .to_string(),
    );
    let tc = type_check(&db, main);
    assert!(
        tc.diagnostics
            .iter()
            .any(|d| d.message.contains("not found or not public") || d.message.contains("hidden")),
        "named import of private must diagnose, got: {:?}",
        tc.diagnostics
    );
}

#[test]
fn private_associated_method_not_exported() {
    let mut db = DatabaseImpl::default();
    let lib = db.new_file(
        "lib.aru".to_string(),
        r#"
            public struct Widget { x: int }
            public func Widget.ok(shared self): int { return self.x }
            func Widget.secret(shared self): int { return self.x }
        "#
        .to_string(),
    );
    let exports = exported_symbols(&db, lib);
    assert!(exports.symbols.contains_key("Widget"));
    assert!(exports.symbols.contains_key("Widget.ok"));
    assert!(
        !exports.symbols.contains_key("Widget.secret"),
        "private method must not appear in export table: {:?}",
        exports.symbols.keys().collect::<Vec<_>>()
    );
}
