use arandu_query::any_execute;
use arandu_query::db::DatabaseImpl;
use arandu_query::passes::type_check;
use salsa::Setter;
use std::sync::Arc;

#[test]
fn cold_type_check_executes() {
    let (mut db, log) = DatabaseImpl::with_rebuild_log();
    let file = db.new_file(
        "explain.aru".into(),
        "func main(): int { return 1 }\n".into(),
    );
    log.clear();
    let _ = type_check(&db, file);
    assert!(
        any_execute(&log),
        "cold type_check must execute:\n{}",
        log.format_chain(true)
    );
}

#[test]
fn text_change_triggers_execute() {
    let (mut db, log) = DatabaseImpl::with_rebuild_log();
    let file = db.new_file(
        "explain2.aru".into(),
        "func main(): int { return 1 }\n".into(),
    );
    let _ = type_check(&db, file);
    log.clear();
    file.set_text(&mut db)
        .to(Arc::from("func main(): int { return 2 }\n"));
    let _ = type_check(&db, file);
    assert!(
        any_execute(&log),
        "dirty text must re-execute:\n{}",
        log.format_chain(true)
    );
}
