//! CST-first pipeline: syntax_tree owns structure; parse lowers from CST.

use arandu_query::db::DatabaseImpl;
use arandu_query::passes::{parse, syntax_tree, type_check};
use salsa::Setter;
use std::sync::Arc;

#[test]
fn parse_depends_on_cst_not_independent_dual() {
    let mut db = DatabaseImpl::new();
    // One-line body: parser allows omitted `;` before `}`.
    let file = db.new_file("c.aru".into(), "func main(): int { return 1 }\n".into());
    let tree = syntax_tree(&db, file);
    assert!(!tree.text().is_empty());
    let prog = parse(&db, file);
    assert!(
        prog.is_ok(),
        "CST lower must produce AST: {:?}",
        prog.as_ref().err()
    );
    let tc = type_check(&db, file);
    assert!(tc.diagnostics.is_empty(), "{:?}", tc.diagnostics);
}

#[test]
fn typeck_still_works_after_edit_via_cst() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file(
        "e.aru".into(),
        "func alpha(): int {\n    return 1\n}\nfunc beta(): int {\n    return 2\n}\n".into(),
    );
    let _ = type_check(&db, file);
    file.set_text(&mut db).to(Arc::from(
        "func alpha(): int {\n    return 1\n}\nfunc beta(): int {\n    return 99\n}\n",
    ));
    let tree = syntax_tree(&db, file);
    let items = tree.item_texts();
    assert!(items.len() >= 2);
    assert!(items[0].contains("alpha"));
    let tc = type_check(&db, file);
    assert!(tc.diagnostics.is_empty(), "{:?}", tc.diagnostics);
}
