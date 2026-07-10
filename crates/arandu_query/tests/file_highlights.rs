//! F2 — type-aware semantic highlights.

use arandu_query::db::DatabaseImpl;
use arandu_query::highlight::{file_highlights, HlKind};
use arandu_query::passes::type_check;

#[test]
fn function_name_is_function_not_variable() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file(
        "hl.aru".into(),
        "func main(): int {\n    let x = 1\n    return x\n}\n".into(),
    );
    let _ = type_check(&db, file);
    let hls = file_highlights(&db, file);

    let text = file.text(&db);
    let main_start = text.find("main").expect("main") as u32;
    let main_end = main_start + 4;
    // `x` in `let x`
    let x_start = (text.find("let x").expect("let x") + 4) as u32;

    let main_hl = hls
        .iter()
        .find(|t| t.start == main_start && t.end == main_end);
    assert!(
        main_hl.is_some_and(|t| t.kind == HlKind::Function),
        "main should be Function, got {main_hl:?}"
    );

    let x_hl = hls
        .iter()
        .find(|t| t.start == x_start && t.end == x_start + 1);
    assert!(
        x_hl.is_some_and(|t| t.kind == HlKind::Variable),
        "local x should be Variable, got {x_hl:?}"
    );
    assert!(
        hls.iter().any(|t| t.kind == HlKind::Keyword),
        "expected keywords in highlights"
    );
    assert!(
        hls.iter().any(|t| t.kind == HlKind::Number),
        "expected number literal"
    );
}

#[test]
fn type_ident_classified() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file(
        "ty.aru".into(),
        "func f(a: int): int {\n    return a\n}\n".into(),
    );
    let _ = type_check(&db, file);
    let hls = file_highlights(&db, file);
    assert!(
        hls.iter()
            .any(|t| matches!(t.kind, HlKind::Type | HlKind::Struct)),
        "expected type-like highlight"
    );
    // param `a` in signature
    let a_start = text_offset(&file.text(&db), "a:") as u32;
    let a_hl = hls
        .iter()
        .find(|t| t.start == a_start && t.end == a_start + 1);
    assert!(
        a_hl.is_some_and(|t| t.kind == HlKind::Parameter || t.kind == HlKind::Variable),
        "param a should be Parameter/Variable, got {a_hl:?}"
    );
}

fn text_offset(text: &str, needle: &str) -> usize {
    text.find(needle).expect(needle)
}
