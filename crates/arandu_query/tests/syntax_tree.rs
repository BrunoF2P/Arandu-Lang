//! P5 — Salsa `syntax_tree` dual + item stability across sibling edits.

use arandu_query::db::DatabaseImpl;
use arandu_query::passes::syntax_tree;
use salsa::Setter;
use std::sync::Arc;

fn two_funcs(beta: i32) -> String {
    format!(
        "func alpha(): int {{\n    return 1\n}}\n\nfunc beta(): int {{\n    return {beta}\n}}\n"
    )
}

#[test]
fn syntax_tree_has_items() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file("cst.aru".into(), two_funcs(2));
    let tree = syntax_tree(&db, file);
    assert!(
        tree.items().len() >= 2,
        "expected ITEM nodes from dual parse, got {}",
        tree.items().len()
    );
}

#[test]
fn syntax_item_text_stable_when_sibling_edited() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file("cst2.aru".into(), two_funcs(2));
    let t1 = syntax_tree(&db, file);
    let items1 = t1.item_texts();
    assert!(items1.len() >= 2);

    file.set_text(&mut db).to(Arc::from(two_funcs(99)));
    let t2 = syntax_tree(&db, file);
    let items2 = t2.item_texts();
    assert!(items2.len() >= 2);
    assert_eq!(
        items1[0], items2[0],
        "alpha CST item text must be unchanged after beta edit"
    );
    assert_ne!(items1[1], items2[1]);
}
