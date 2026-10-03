#![allow(clippy::expect_used, clippy::panic)]

use arandu_middle::{amir::AmirStmt, ctfe::ConstValue, Severity};
use arandu_query::{
    passes::{lower_amir, type_check},
    DatabaseImpl,
};
use salsa::Setter;

fn checked(db: &DatabaseImpl, file: arandu_query::SourceFile) -> Vec<ConstValue> {
    let typed = type_check(db, file);
    let errors: Vec<_> = typed
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let result = lower_amir(db, file);
    let errors: Vec<_> = result
        .type_check
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let main = result
        .amir
        .funcs
        .iter()
        .find(|f| result.type_check.symbols.get(f.symbol).name == "main")
        .expect("main");
    assert!(main
        .stmts
        .iter_ids()
        .all(|id| !matches!(main.stmt(id), AmirStmt::Call { .. })));
    let mut values: Vec<_> = typed
        .type_info
        .ctfe_values
        .iter()
        .map(|(span, (value, _))| (span.start, value.clone()))
        .collect();
    values.sort_by_key(|(start, _)| *start);
    values.into_iter().map(|(_, value)| value).collect()
}

#[test]
fn arrays_nested_products_and_closed_structs_materialize_in_destination_pools() {
    for source in [
        "func table(): [2]int { return [20, 22] }\nfunc main(): int { let a = comptime table(); return a[0] + a[1] }",
        "func pair(): (int, bool) { return 42, true }\nfunc main(): int { let a, enabled = comptime pair(); if enabled { return a }; return 0 }",
        "struct Point { x: int y: int }\nfunc make(): Point { return Point { y: 22, x: 20 } }\nfunc main(): int { let point = comptime make(); return point.x + point.y }",
        "func grid(): [2][2]int { return [[1, 2], [20, 22]] }\nfunc main(): int { let a = comptime grid(); return a[1][0] + a[1][1] }",
        "func main(): int { let a = comptime { let mut a: [2]int = [2, 3]; set a[1] = 40; return a }; return a[0] + a[1] }",
        "func main(): int { let a = comptime [20, 22]; return a[0] + a[1] }",
        "func main(): int { let a = comptime { let mut a = [2, 3]; set a[1] = 40; return a }; return a[0] + a[1] }",
        "func empty(): [0]int { return [] }\nfunc main(): int { let a = comptime empty(); return 42 }",
    ] {
        let mut db = DatabaseImpl::new();
        let file = db.new_file("products.aru".into(), source.into());
        let values = checked(&db, file);
        assert_eq!(values.len(), 1, "{source}");
        assert!(matches!(values[0], ConstValue::Aggregate(_)), "{values:?}");
    }
}

#[test]
fn frozen_products_follow_target_width_and_are_revalidated_after_target_edits() {
    use arandu_middle::layout::DataLayout;

    let source = "struct Pair { small: usize wide: u64 }\nfunc make(): Pair { return Pair { small: 20, wide: 22 } }\nfunc main(): int { let pair = comptime make(); return 42 }";
    let mut db = DatabaseImpl::new();
    let file = db.new_file("target-products.aru".into(), source.into());
    let mut values = Vec::new();
    for width in [4, 8] {
        db.set_target_config(DataLayout::ptr_width(width));
        let result = checked(&db, file);
        let ConstValue::Aggregate(aggregate) = &result[0] else {
            panic!("expected frozen struct");
        };
        let ConstValue::Integer(small) = aggregate.values()[0] else {
            panic!("expected target-sized field");
        };
        let ConstValue::Integer(wide) = aggregate.values()[1] else {
            panic!("expected fixed-width field");
        };
        assert_eq!(u64::from(small.ty().bit_width()), width * 8);
        assert_eq!(wide.ty().bit_width(), 64);
        let mut clean = DatabaseImpl::new();
        clean.set_target_config(DataLayout::ptr_width(width));
        let clean_file = clean.new_file("target-products.aru".into(), source.into());
        assert_eq!(result, checked(&clean, clean_file));
        values.push(result);
    }
    assert_ne!(
        values[0], values[1],
        "target width is part of frozen identity"
    );
}

#[test]
fn frozen_array_helper_edits_match_clean_and_keep_equal_values_equal() {
    let source = "func table(): [2]int { return [20, 22] }\nfunc main(): int { let a = comptime table(); return a[0] + a[1] }";
    let mut db = DatabaseImpl::new();
    let file = db.new_file("products.aru".into(), source.into());
    let first = checked(&db, file);
    let equal = source.replace("[20, 22]", "[20, 11 + 11]");
    file.set_text(&mut db).to(equal.into());
    assert_eq!(checked(&db, file), first);
    let changed = source.replace("[20, 22]", "[20, 23]");
    file.set_text(&mut db).to(changed.clone().into());
    let incremental = checked(&db, file);
    assert_ne!(incremental, first);
    let mut clean = DatabaseImpl::new();
    let file = clean.new_file("products.aru".into(), changed);
    assert_eq!(incremental, checked(&clean, file));
}

#[test]
fn resource_structs_and_out_of_bounds_products_fail_without_ice() {
    for source in [
        "struct Resource { x: int }\n@Destructor\nfunc Resource.destroy(self: Resource): void {}\nfunc make(): Resource { return Resource { x: 42 } }\nfunc main(): void { let value = comptime make() }",
        "func main(): int { return comptime { let a = [20, 22]; a[4] } }",
    ] {
        let mut db = DatabaseImpl::new();
        let file = db.new_file("invalid.aru".into(), source.into());
        let typed = type_check(&db, file);
        let errors: Vec<_> = typed
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(!errors.is_empty(), "{source}");
        assert!(
            errors.iter().all(|d| !d.code.as_str().starts_with("ICE")),
            "{errors:?}"
        );
        assert!(typed.type_info.ctfe_values.is_empty());
    }
}

#[test]
fn equal_field_types_reordered_in_nominal_metadata_do_not_alias_cached_units() {
    let source = "struct Point { x: int y: int }\nfunc make(): Point { return Point { x: 20, y: 22 } }\nfunc main(): int { let point = comptime make(); return point.x + point.y }";
    let mut db = DatabaseImpl::new();
    let file = db.new_file("fields.aru".into(), source.into());
    let before = checked(&db, file);
    let edited = source.replace("x: int y: int", "y: int x: int");
    file.set_text(&mut db).to(edited.clone().into());
    let incremental = checked(&db, file);
    assert_ne!(
        before, incremental,
        "frozen fields follow declaration order"
    );
    let mut clean = DatabaseImpl::new();
    let file = clean.new_file("fields.aru".into(), edited);
    assert_eq!(incremental, checked(&clean, file));
}
