use arandu_query::db::{DatabaseImpl, SourceFile};
use std::sync::atomic::Ordering;

// Reusable macro to prove that a specific query correctly uses Salsa's Accumulator
// and that diagnostics survive a cache hit without re-executing the query's hot path.
macro_rules! assert_query_accumulator_survives_cache_hit {
    ($db:expr, $file:expr, $query_fn:path, $accum_fn:path, $counter:expr) => {{
        // Reset the counter to 0
        $counter.store(0, Ordering::SeqCst);

        // First execution: query runs, diagnostic is emitted via accumulate()
        let _ = $query_fn($db, $file);

        let diags1 = $accum_fn($db, $file);
        assert!(!diags1.is_empty(), "Expected diagnostics on first run of query");

        // Prova nº 1: A query de fato rodou.
        assert_eq!($counter.load(Ordering::SeqCst), 1, "Query should have executed exactly once");

        // Second execution, WITHOUT changing the input: it must be a cache hit.
        let _ = $query_fn($db, $file);
        let diags2 = $accum_fn($db, $file);
        assert!(!diags2.is_empty(), "Expected diagnostics to be re-emitted on cache hit");

        // Prova nº 2: A query bateu no cache e NÃO re-executou o código interno.
        assert_eq!($counter.load(Ordering::SeqCst), 1, "Query should NOT have executed again on a cache hit");

        // Prova nº 3: O exato mesmo diagnóstico que estava guardado no cache foi re-emitido nativamente.
        assert_eq!(diags1.len(), diags2.len());
        assert_eq!(diags1[0].0.code, diags2[0].0.code);
    }};
}

#[test]
fn accumulator_diagnostic_survives_cache_hit() {
    let db = DatabaseImpl::default();

    // Create a source file with an intentional resolution error (duplicate field)
    let code = std::sync::Arc::from("struct Foo { a: i32; a: i32; }");
    let file = SourceFile::new(
        &db,
        1,
        code,
        std::sync::Arc::new(std::path::PathBuf::from("test.ar")),
    );

    assert_query_accumulator_survives_cache_hit!(
        &db,
        file,
        arandu_query::passes::type_check,
        arandu_query::passes::type_check::accumulated::<arandu_middle::db::DiagnosticsAccumulator>,
        arandu_query::passes::TYPE_CHECK_EXEC_COUNT
    );
}
