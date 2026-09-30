#![allow(clippy::expect_used)]

use arandu_middle::ctfe::{ConstInt, ConstValue, IntegerType};
use arandu_middle::types::Primitive;
use arandu_middle::{DataLayout, SymbolId};
use arandu_mir::ctfe::{Budget, CtfeFunction, EvalError, EvalErrorKind, FunctionProvider};
use arandu_query::ctfe::{ctfe_eval, ctfe_func_amir, BuildFailure, CtfeRequest};
use arandu_query::{ArandCompilerDb, DatabaseImpl, SourceFile};
use salsa::Setter;
use std::sync::Arc;

fn budget() -> Budget {
    Budget {
        fuel: 100_000,
        frames: 64,
        values: 10_000,
    }
}

fn symbol(db: &DatabaseImpl, file: SourceFile, name: &str) -> SymbolId {
    arandu_query::passes::module_signatures(db, file)
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .expect("function symbol")
        .id
}

fn run(
    db: &DatabaseImpl,
    file: SourceFile,
    symbol: SymbolId,
    arguments: Vec<ConstValue>,
    budget: Budget,
) -> Result<ConstValue, EvalError> {
    *ctfe_eval(db, CtfeRequest::new(db, file, symbol, arguments, budget))
}

fn number(value: ConstValue) -> i128 {
    let ConstValue::Integer(value) = value else {
        panic!("expected integer")
    };
    value.value()
}

fn source(helper: &str, unused: &str) -> String {
    format!("func foo(): int {{ return add(20, 22) }}\nfunc add(a: int, b: int): int {{ {helper} }}\nfunc unused(): int {{ {unused} }}\n")
}

#[test]
fn evaluates_a_real_call_without_whole_program_lowering_or_sibling_typeck() {
    let (mut db, log) = DatabaseImpl::with_rebuild_log();
    let file = db.new_file("ctfe.aru".into(), source("return a + b", "return missing"));
    let foo = symbol(&db, file, "foo");
    log.clear();
    assert_eq!(
        number(run(&db, file, foo, vec![], budget()).expect("CTFE call")),
        42
    );
    assert_eq!(log.count_executions_matching("lower_amir"), 0);
    assert_eq!(log.count_executions_matching("ctfe_func_amir"), 2);
    assert_eq!(log.count_executions_matching("item_body_typeck"), 2);
    let lowered = ctfe_func_amir(&db, file, foo);
    assert_eq!(
        lowered.result.as_ref().expect("unit").function().symbol,
        foo
    );
}

#[test]
fn sibling_body_edit_does_not_relower_or_reevaluate_the_entry() {
    let (mut db, log) = DatabaseImpl::with_rebuild_log();
    let file = db.new_file("ctfe.aru".into(), source("return a + b", "return 1"));
    let foo = symbol(&db, file, "foo");
    assert_eq!(
        number(run(&db, file, foo, vec![], budget()).expect("initial")),
        42
    );
    log.clear();
    file.set_text(&mut db)
        .to(Arc::from(source("return a + b", "return 999")));
    assert_eq!(
        number(run(&db, file, foo, vec![], budget()).expect("after edit")),
        42
    );
    assert_eq!(log.count_executions_matching("ctfe_func_amir"), 0);
    assert_eq!(log.count_executions_matching("ctfe_eval"), 0);
    assert_eq!(log.count_executions_matching("lower_amir"), 0);
}

#[salsa::tracked]
fn consumer<'db>(
    db: &'db dyn ArandCompilerDb,
    request: CtfeRequest<'db>,
) -> Result<ConstValue, EvalError> {
    *ctfe_eval(db, request)
}

fn consume(db: &DatabaseImpl, file: SourceFile, symbol: SymbolId) -> Result<ConstValue, EvalError> {
    *consumer(db, CtfeRequest::new(db, file, symbol, vec![], budget()))
}

#[test]
fn helper_body_changes_recompute_only_that_unit_and_cut_off_equal_values() {
    let (mut db, log) = DatabaseImpl::with_rebuild_log();
    let file = db.new_file("ctfe.aru".into(), source("return a + b", "return 1"));
    let foo = symbol(&db, file, "foo");
    assert_eq!(number(consume(&db, file, foo).expect("initial")), 42);
    log.clear();
    file.set_text(&mut db)
        .to(Arc::from(source("return a + b + 0", "return 1")));
    assert_eq!(number(consume(&db, file, foo).expect("equal result")), 42);
    assert_eq!(log.count_executions_matching("ctfe_func_amir"), 1);
    assert_eq!(log.count_executions_matching("ctfe_eval"), 1);
    assert_eq!(log.count_executions_matching("consumer"), 0);
    log.clear();
    file.set_text(&mut db)
        .to(Arc::from(source("return a + b + 1", "return 1")));
    assert_eq!(number(consume(&db, file, foo).expect("changed result")), 43);
    assert_eq!(log.count_executions_matching("consumer"), 1);
    let mut clean = DatabaseImpl::new();
    let clean_file = clean.new_file("ctfe.aru".into(), source("return a + b + 1", "return 1"));
    assert_eq!(
        consume(&db, file, foo),
        run(
            &clean,
            clean_file,
            symbol(&clean, clean_file, "foo"),
            vec![],
            budget()
        )
    );
}

#[test]
fn loops_and_recursive_calls_run_on_bounded_frames() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file("ctfe.aru".into(), "func sum(): int { let mut i = 0\n let mut total = 0\n while i < 10 { total = total + i\n i = i + 1 }\n return total }\nfunc recurse(n: int): int { if n == 0 { return 7 }\n return recurse(n - 1) }".into());
    assert_eq!(
        number(run(&db, file, symbol(&db, file, "sum"), vec![], budget()).expect("loop")),
        45
    );
    let int = IntegerType::new(Primitive::Int, DataLayout::ptr_width(8)).expect("int");
    let argument = ConstValue::Integer(ConstInt::new(int, 20).expect("argument"));
    let recurse = symbol(&db, file, "recurse");
    assert_eq!(
        number(run(&db, file, recurse, vec![argument], budget()).expect("recursive call")),
        7
    );
    let mut limited = budget();
    limited.frames = 4;
    assert_eq!(
        run(&db, file, recurse, vec![argument], limited)
            .expect_err("frame bound")
            .kind,
        EvalErrorKind::FrameLimit
    );
}

#[test]
fn fuel_and_values_are_limits_not_fallback_results_and_policy_is_in_the_key() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file(
        "ctfe.aru".into(),
        "func foo(): int { return 42 }\nfunc endless(): int { while true {}\n return 1 }".into(),
    );
    let foo = symbol(&db, file, "foo");
    let mut limited = budget();
    limited.fuel = 0;
    assert_eq!(
        run(&db, file, foo, vec![], limited).expect_err("fuel").kind,
        EvalErrorKind::FuelExhausted
    );
    assert_eq!(
        number(run(&db, file, foo, vec![], budget()).expect("not poisoned")),
        42
    );
    limited = budget();
    limited.values = 0;
    assert_eq!(
        run(&db, file, foo, vec![], limited)
            .expect_err("values")
            .kind,
        EvalErrorKind::ValueLimit
    );
    limited = budget();
    limited.fuel = 200;
    assert_eq!(
        run(&db, file, symbol(&db, file, "endless"), vec![], limited)
            .expect_err("loop bound")
            .kind,
        EvalErrorKind::FuelExhausted
    );
}

struct One(Arc<CtfeFunction>);
impl FunctionProvider for One {
    fn function(&self, symbol: SymbolId) -> Result<Arc<CtfeFunction>, EvalErrorKind> {
        if self.0.function().symbol != symbol {
            return Err(EvalErrorKind::MissingFunction(symbol));
        }
        Ok(Arc::clone(&self.0))
    }
}

#[test]
fn cancellation_interrupts_an_infinite_loop_and_does_not_poison_a_later_run() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file(
        "ctfe.aru".into(),
        "func foo(): int { while true {}\n return 1 }".into(),
    );
    let foo = symbol(&db, file, "foo");
    let unit = ctfe_func_amir(&db, file, foo)
        .result
        .as_ref()
        .expect("unit")
        .clone();
    let mut polls = 0;
    let result = arandu_mir::ctfe::evaluate(&One(unit), foo, &[], budget(), || {
        polls += 1;
        polls >= 50
    });
    assert_eq!(
        result.expect_err("cancelled").kind,
        EvalErrorKind::Cancelled
    );
    assert_eq!(polls, 50);
    let mut limited = budget();
    limited.fuel = 100;
    assert_eq!(
        run(&db, file, foo, vec![], limited)
            .expect_err("ordinary fuel error")
            .kind,
        EvalErrorKind::FuelExhausted
    );
}

#[test]
fn full_u64_and_target_sized_values_do_not_use_the_host_width() {
    let mut db = DatabaseImpl::new();
    db.set_target_config(DataLayout::ptr_width(8));
    let file = db.new_file("ctfe.aru".into(), "func wide(): u64 { return 18446744073709551615 }\nfunc sized(): usize { return 4294967296 }".into());
    assert_eq!(
        number(run(&db, file, symbol(&db, file, "wide"), vec![], budget()).expect("u64")),
        i128::from(u64::MAX)
    );
    let sized = symbol(&db, file, "sized");
    assert_eq!(
        number(run(&db, file, sized, vec![], budget()).expect("usize64")),
        1_i128 << 32
    );
    db.set_target_config(DataLayout::ptr_width(4));
    assert!(run(&db, file, sized, vec![], budget()).is_err());
    assert_eq!(
        number(run(&db, file, symbol(&db, file, "wide"), vec![], budget()).expect("u64 on ptr4")),
        i128::from(u64::MAX)
    );
}

#[test]
fn unsupported_generics_and_external_calls_never_get_a_dummy_body() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file("ctfe.aru".into(), "func identity<T>(value: T): T { return value }\nextern \"C\" { func external(): int }\nfunc foo(): int { return external() }".into());
    let identity = symbol(&db, file, "identity");
    assert!(matches!(
        ctfe_func_amir(&db, file, identity).result,
        Err(BuildFailure::GenericFunction)
    ));
    assert!(matches!(
        run(&db, file, symbol(&db, file, "foo"), vec![], budget())
            .expect_err("external")
            .kind,
        EvalErrorKind::UnavailableFunction(_) | EvalErrorKind::MissingFunction(_)
    ));
}

#[test]
fn literal_pool_changes_invalidate_even_when_the_amir_ids_are_identical() {
    use arandu_query::StableHash;
    let mut db = DatabaseImpl::new();
    let file = db.new_file("literal.aru".into(), "func foo(): int { return 42 }".into());
    let foo = symbol(&db, file, "foo");
    let before = ctfe_func_amir(&db, file, foo)
        .result
        .as_ref()
        .expect("unit")
        .clone();
    assert_eq!(
        number(run(&db, file, foo, vec![], budget()).expect("before")),
        42
    );
    file.set_text(&mut db)
        .to(Arc::from("func foo(): int { return 43 }"));
    let after = ctfe_func_amir(&db, file, foo)
        .result
        .as_ref()
        .expect("unit")
        .clone();
    assert_eq!(
        before.function().stable_hash(),
        after.function().stable_hash()
    );
    assert_eq!(
        number(run(&db, file, foo, vec![], budget()).expect("after")),
        43
    );
}

#[test]
fn imports_are_explicitly_pending_without_a_backdoor_into_global_lowering() {
    let (mut db, log) = DatabaseImpl::with_rebuild_log();
    let file = db.new_file(
        "imports.aru".into(),
        "import missing\nfunc foo(): int { return 42 }".into(),
    );
    // Obtain the local identity without querying imported module signatures.
    let locals = arandu_query::passes::local_symbols(&db, file);
    let foo = locals
        .symbols
        .iter()
        .find(|symbol| symbol.name == "foo")
        .expect("local function")
        .id;
    log.clear();
    assert!(matches!(
        ctfe_func_amir(&db, file, foo).result,
        Err(BuildFailure::ImportsNotStaged)
    ));
    assert_eq!(log.count_executions_matching("lower_amir"), 0);
    assert_eq!(log.count_executions_matching("item_body_typeck"), 0);
}

#[test]
fn cancelled_salsa_reader_unwinds_and_does_not_memoize_a_ctfe_failure() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file("cancel.aru".into(), "func foo(): int { return 42 }".into());
    let foo = symbol(&db, file, "foo");
    let reader = db.clone();
    reader.query_cancellation_token().cancel();
    assert!(
        arandu_query::catch_query_cancellation(|| run(&reader, file, foo, vec![], budget()))
            .is_err()
    );
    drop(reader);
    assert_eq!(
        number(run(&db, file, foo, vec![], budget()).expect("uncancelled reader")),
        42
    );
}

#[test]
fn relevant_resolution_errors_are_preserved_and_disappear_after_an_edit() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file(
        "error.aru".into(),
        "func foo(): int { return missing }".into(),
    );
    let foo = symbol(&db, file, "foo");
    assert!(
        matches!(&ctfe_func_amir(&db, file, foo).result, Err(BuildFailure::Diagnostics(diagnostics))
        if diagnostics.iter().any(|diagnostic| diagnostic.code == arandu_middle::DiagCode::N001UndefinedValue))
    );
    file.set_text(&mut db)
        .to(Arc::from("func foo(): int { return 42 }"));
    assert_eq!(
        number(run(&db, file, foo, vec![], budget()).expect("fixed")),
        42
    );
}

#[test]
fn typed_use_casts_are_checked_instead_of_wrapping() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file(
        "cast.aru".into(),
        "func narrow(x: i64): u8 { return x as u8 }".into(),
    );
    let narrow = symbol(&db, file, "narrow");
    let ty = IntegerType::new(Primitive::I64, DataLayout::ptr_width(8)).expect("i64");
    let argument = |value| ConstValue::Integer(ConstInt::new(ty, value).expect("argument"));
    assert_eq!(
        number(run(&db, file, narrow, vec![argument(42)], budget()).expect("cast in range")),
        42
    );
    assert!(matches!(
        run(&db, file, narrow, vec![argument(256)], budget())
            .expect_err("out of range")
            .kind,
        EvalErrorKind::Value(arandu_middle::ctfe::ConstValueError::OutOfRange { .. })
    ));
}
