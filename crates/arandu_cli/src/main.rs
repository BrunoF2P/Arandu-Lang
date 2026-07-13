#![allow(clippy::collapsible_if)]
use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
};

fn clean_exit(code: i32) -> ! {
    arandu_base::print_perf_summary();
    arandu_base::finalize_self_profile();
    process::exit(code);
}

fn print_diagnostics_and_exit(
    diagnostics: impl IntoIterator<Item = arandu_middle::Diagnostic>,
    filepath: &str,
) -> ! {
    let source = if !filepath.is_empty() {
        fs::read_to_string(filepath).unwrap_or_default()
    } else {
        String::new()
    };

    let named_source = miette::NamedSource::new(filepath, source);

    for diagnostic in diagnostics {
        let report = miette::Report::new(diagnostic).with_source_code(named_source.clone());
        eprintln!("{:?}", report);
    }

    clean_exit(1);
}

fn print_parse_error_and_exit(err: &arandu_parser::ParseError, filepath: &str) -> ! {
    let diag = arandu_middle::Diagnostic::from(err.clone());
    print_diagnostics_and_exit(std::iter::once(diag), filepath);
}

fn validate_hir_and_monomorphize(
    hir: &mut arandu_semantics::hir::HirProgram,
    type_check: &mut arandu_semantics::TypeCheckResult,
    filepath: &str,
) {
    if let Err(err) = hir.validate_invariants(&hir.pool, &type_check.symbols) {
        eprintln!("HIR invariant violation: {err}");
        clean_exit(1);
    }

    if let Err(diags) =
        arandu_semantics::passes::monomorphize::monomorphize_program(type_check, hir)
    {
        print_diagnostics_and_exit(diags, filepath);
    }
}

struct CheckedProgram {
    /// Shared with Salsa memo — never deep-clone the AST.
    program: std::sync::Arc<arandu_parser::Program>,
    type_check: arandu_semantics::TypeCheckResult,
}

/// Print Salsa-accumulated diagnostics once. Returns whether any Error was seen.
fn print_accumulated_diags(
    diags: &[impl std::ops::Deref<Target = arandu_middle::db::DiagnosticsAccumulator>],
    filepath: &str,
) -> bool {
    if diags.is_empty() {
        return false;
    }
    let source = std::fs::read_to_string(filepath).unwrap_or_default();
    let named_source = miette::NamedSource::new(filepath, source);
    let mut has_fatal = false;
    for d in diags {
        if matches!(d.0.severity, arandu_middle::Severity::Error) {
            has_fatal = true;
        }
        let report = miette::Report::new(d.0.clone()).with_source_code(named_source.clone());
        eprintln!("{:?}", report);
    }
    has_fatal
}

/// Single pipeline entry for check / run / amir / emit-c:
/// parse → type_check (diags once) → lower_amir (lower diags once).
/// Salsa memos each step; no second HIR/mono outside the query.
fn pipeline_lower(
    db: &dyn arandu_query::db::ArandCompilerDb,
    file: arandu_query::db::SourceFile,
    filepath: &str,
) -> std::sync::Arc<arandu_query::LowerAmirArtifacts> {
    {
        arandu_base::time_pass!("parse");
        let program_res = arandu_query::passes::parse(db, file);
        if let Err(err) = &*program_res {
            print_parse_error_and_exit(err, filepath);
        }
    }

    {
        arandu_base::time_pass!("type_check");
        let _ = arandu_query::passes::type_check(db, file);
    }
    let type_diags = arandu_query::passes::type_check::accumulated::<
        arandu_middle::db::DiagnosticsAccumulator,
    >(db, file);
    if print_accumulated_diags(&type_diags, filepath) {
        process::exit(1);
    }

    let artifacts = {
        arandu_base::time_pass!("lower-amir");
        arandu_query::passes::lower_amir(db, file)
    };
    let lower_diags = arandu_query::passes::lower_amir::accumulated::<
        arandu_middle::db::DiagnosticsAccumulator,
    >(db, file);
    if print_accumulated_diags(&lower_diags, filepath) {
        process::exit(1);
    }

    std::sync::Arc::clone(&artifacts.value)
}

/// Parse + type-check for paths that still need a local TypeCheckResult (e.g. `hir`).
fn parse_and_check(
    db: &dyn arandu_query::db::ArandCompilerDb,
    file: arandu_query::db::SourceFile,
    filepath: &str,
) -> CheckedProgram {
    let program_res = {
        arandu_base::time_pass!("parse");
        arandu_query::passes::parse(db, file)
    };
    let program = match &*program_res {
        Ok(program) => std::sync::Arc::clone(program),
        Err(err) => print_parse_error_and_exit(err, filepath),
    };

    let type_check = {
        arandu_base::time_pass!("type_check");
        arandu_query::passes::type_check(db, file)
    };

    let diagnostics = arandu_query::passes::type_check::accumulated::<
        arandu_middle::db::DiagnosticsAccumulator,
    >(db, file);
    if print_accumulated_diags(&diagnostics, filepath) {
        process::exit(1);
    }

    // TypeCheckResult is Arc-heavy (symbols/resolved/type_info) — clone is O(1) for IR.
    CheckedProgram {
        program,
        type_check: (*type_check).clone(),
    }
}

fn usage_and_exit() -> ! {
    eprintln!(
        "usage: arandu_cli <lex|parse|check|hir|amir|run|emit-c|graph|fmt> <path> [--debug] [--opt] [--parallel]"
    );
    eprintln!("       emit-c options: --layout=host|ptr4|i686  (default: host)");
    eprintln!("       G2/F2.3: --no-generational-fallback  (promote O004 notes to errors)");
    eprintln!("       -Z flags: -Ztime-passes  -Zprofile-queries  -Zprint-alloc-stats  -Zdump-mir");
    eprintln!(
        "                : -Zdebug-parser -Zdebug-typeck -Zdebug-ossa -Zdebug-layout -Zdebug-backend -Zdebug-all"
    );
    eprintln!(
        "                : -Zself-profile=<path>  -Zexplain-rebuild  -Zno-generational-fallback"
    );

    process::exit(2);
}

fn parse_data_layout(flags: &[String]) -> arandu_middle::layout::DataLayout {
    use arandu_middle::layout::DataLayout;
    for f in flags {
        if let Some(rest) = f.strip_prefix("--layout=") {
            return match rest {
                "host" => DataLayout::host(),
                "ptr4" | "32" => DataLayout::ptr_width(4),
                "i686" | "i686-sysv" => DataLayout::i686_sysv(),
                "ptr8" | "64" => DataLayout::ptr_width(8),
                other => {
                    eprintln!("unknown --layout={other} (use host|ptr4|ptr8|i686)");
                    process::exit(2);
                }
            };
        }
    }
    DataLayout::host()
}

fn find_aru_files(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                find_aru_files(&path, files)?;
            } else if path.extension().and_then(|s| s.to_str()) == Some("aru") {
                files.push(path);
            }
        }
    }
    Ok(())
}

fn main() {
    let mut debug = false;
    let mut opt = false;
    let mut parallel = false;
    let mut args = Vec::new();
    let mut z_flags: Vec<String> = Vec::new();
    let mut layout_flags: Vec<String> = Vec::new();

    for arg in env::args() {
        match arg.as_str() {
            "--debug" => debug = true,
            "--opt" => opt = true,
            "--parallel" => parallel = true,
            // G2: long form of -Zno-generational-fallback (same atomic).
            "--no-generational-fallback" => {
                z_flags.push("-Zno-generational-fallback".into());
            }
            s if s.starts_with("-Z") => z_flags.push(arg),
            s if s.starts_with("--layout=") => layout_flags.push(arg),
            _ => args.push(arg),
        }
    }
    let data_layout = parse_data_layout(&layout_flags);

    // Initialise global perf flags (written once, read-only afterwards).
    arandu_base::init_z_flags(&z_flags);

    // Initialise the tracing subscriber from -Zdebug-* / -Zself-profile flags.
    let tracing_cfg = arandu_base::build_tracing_config();
    arandu_base::tracing_bridge::init_tracing(tracing_cfg);

    if args.len() != 3 {
        usage_and_exit();
    }

    let command = &args[1];

    if !matches!(
        command.as_str(),
        "lex" | "parse" | "check" | "hir" | "amir" | "run" | "emit-c" | "graph" | "fmt"
    ) {
        usage_and_exit();
    }

    let path = Path::new(&args[2]);

    let mut paths = Vec::new();
    if path.is_dir() {
        if let Err(err) = find_aru_files(path, &mut paths) {
            eprintln!("failed to list directory {}: {err}", path.display());
            process::exit(1);
        }
        paths.sort();
    } else {
        paths.push(path.to_path_buf());
    }

    if paths.is_empty() {
        eprintln!("no .aru source files found at {}", path.display());
        process::exit(1);
    }

    if command == "fmt" {
        let mut changed = 0usize;
        for p in &paths {
            let src = match fs::read_to_string(p) {
                Ok(s) => s,
                Err(err) => {
                    eprintln!("failed to read {}: {err}", p.display());
                    process::exit(1);
                }
            };
            let formatted = arandu_fmt::format_source(&src);
            if formatted != src {
                if let Err(err) = fs::write(p, &formatted) {
                    eprintln!("failed to write {}: {err}", p.display());
                    process::exit(1);
                }
                changed += 1;
                eprintln!("formatted {}", p.display());
            }
        }
        if changed == 0 {
            eprintln!("already formatted ({} file(s))", paths.len());
        }
        return;
    }

    let use_parallel = parallel || paths.len() > 1;

    if use_parallel {
        if matches!(command.as_str(), "lex" | "parse" | "run" | "emit-c") {
            eprintln!(
                "parallel/multi-file mode is not supported for command '{}'",
                command
            );
            process::exit(1);
        }
    }

    let explain = arandu_base::EXPLAIN_REBUILD.load(std::sync::atomic::Ordering::Relaxed);
    let (db, rebuild_log) = if explain {
        let (db, log) = arandu_query::db::DatabaseImpl::with_rebuild_log();
        (db, Some(log))
    } else {
        (arandu_query::db::DatabaseImpl::new(), None)
    };
    let mut registry = arandu_base::SourceRegistry::default();

    let mut source_files = Vec::new();
    for p in &paths {
        match fs::read_to_string(p) {
            Ok(source) => {
                let filepath = p.to_string_lossy().into_owned();
                let file_id = registry.register(&filepath, &source);
                let code = std::sync::Arc::from(source.clone());
                let source_file = arandu_query::db::SourceFile::new(
                    &db,
                    file_id,
                    code,
                    std::sync::Arc::new(p.clone()),
                );
                db.register_source_file(filepath.clone(), source_file);
                source_files.push((source_file, filepath, source));
            }
            Err(err) => {
                eprintln!("failed to read {}: {err}", p.display());
                process::exit(1);
            }
        }
    }

    use rayon::prelude::*;
    let process_file = |source_file: arandu_query::db::SourceFile,
                        filepath: String,
                        source: String,
                        db: arandu_query::db::DatabaseImpl| {
        match command.as_str() {
            "lex" => match arandu_lexer::lex_to_string(&source) {
                Ok(output) => println!("{output}"),
                Err(err) => {
                    eprintln!("{err}");
                    clean_exit(1);
                }
            },

            "parse" => match arandu_parser::parse_to_string(&source) {
                Ok(output) => println!("{output}"),
                Err(err) => {
                    eprintln!("{err}");
                    clean_exit(1);
                }
            },

            "check" => {
                // One pipeline: parse → typeck → lower_amir (Salsa memos; diags once each).
                let _ = pipeline_lower(&db, source_file, &filepath);
                tracing::info!(
                    "Compilation verified successfully — no errors found for {}",
                    filepath
                );
                println!("ok {}", filepath);
            }

            "hir" => {
                let mut checked = parse_and_check(&db, source_file, &filepath);
                let mut hir = {
                    arandu_base::time_pass!("lower-hir");
                    match arandu_semantics::lower_to_hir(&mut checked.type_check, &checked.program)
                    {
                        Ok(hir) => hir,
                        Err(diags) => print_diagnostics_and_exit(diags, &filepath),
                    }
                };
                validate_hir_and_monomorphize(&mut hir, &mut checked.type_check, &filepath);

                if debug {
                    println!("{hir:#?}");
                } else {
                    let ctx = arandu_semantics::hir::HirPrettyCtx {
                        pool: &hir.pool,
                        symbols: &checked.type_check.symbols,
                        show_spans: false,
                        type_interner: Some(&checked.type_check.type_info.type_interner),
                    };
                    println!("--- HIR for {} ---", filepath);
                    print!("{}", hir.pretty_print(&ctx));
                }
            }

            "amir" => {
                let artifacts = pipeline_lower(&db, source_file, &filepath);
                let symbols = artifacts.type_check.symbols.as_ref();
                let interner = &artifacts.type_check.type_info.type_interner;
                let mut amir_owned = if opt {
                    Some(artifacts.amir.clone())
                } else {
                    None
                };
                if let Some(ref mut amir) = amir_owned {
                    arandu_base::time_pass!("optimize-amir");
                    arandu_semantics::optimize_amir(amir);
                }
                let amir = match &amir_owned {
                    Some(a) => a,
                    None => &artifacts.amir,
                };

                if debug {
                    println!("{amir:#?}");
                } else {
                    println!("--- AMIR for {} ---", filepath);
                    print!("{}", amir.pretty_print(symbols, interner));
                }
            }

            "run" => {
                let artifacts = pipeline_lower(&db, source_file, &filepath);
                tracing::info!("AMIR lowering completed (Salsa: single pipeline)");

                let type_check = &artifacts.type_check;
                let mut amir_owned = if opt {
                    Some(artifacts.amir.clone())
                } else {
                    None
                };
                if let Some(ref mut amir) = amir_owned {
                    arandu_base::time_pass!("optimize-amir");
                    arandu_semantics::optimize_amir(amir);
                    tracing::info!("Optimisation passes applied");
                }
                let amir = match &amir_owned {
                    Some(a) => a,
                    None => &artifacts.amir,
                };

                use arandu_semantics::{CodegenBackend, CompiledCode};
                let output = {
                    let backend = {
                        arandu_base::time_pass!("codegen-init");
                        match arandu_backend_cranelift::CraneliftBackend::try_new() {
                            Ok(backend) => backend,
                            Err(diag) => {
                                print_diagnostics_and_exit(std::iter::once(diag), &filepath)
                            }
                        }
                    };
                    arandu_base::time_pass!("codegen-translate");
                    match CodegenBackend::compile(
                        backend,
                        amir,
                        type_check.symbols.as_ref(),
                        type_check.type_info.as_ref(),
                    ) {
                        Ok(out) => out,
                        Err(diag) => print_diagnostics_and_exit(std::iter::once(diag), &filepath),
                    }
                };
                tracing::info!("Machine code generated (Cranelift JIT backend)");

                let main_is_void = amir.funcs.iter().any(|f| {
                    let name = type_check.symbols.get(f.symbol).name.as_str();
                    if name != "main" {
                        return false;
                    }
                    matches!(
                        type_check.type_info.type_interner.resolve(f.return_type),
                        arandu_semantics::types::ArType::Void
                    )
                });
                let has_main = amir
                    .funcs
                    .iter()
                    .any(|f| type_check.symbols.get(f.symbol).name.as_str() == "main");
                if !has_main {
                    eprintln!("Error: 'main' function not found in compiled program");
                    clean_exit(1);
                }

                unsafe {
                    if main_is_void {
                        if let Some(main_fn) = CompiledCode::get_fn::<unsafe fn()>(&output, "main")
                        {
                            main_fn();
                            clean_exit(0);
                        }
                    } else if let Some(main_fn) =
                        CompiledCode::get_fn::<unsafe fn() -> i32>(&output, "main")
                    {
                        let code = main_fn();
                        clean_exit(code);
                    }
                    eprintln!("Error: 'main' function not found in compiled program");
                    clean_exit(1);
                }
            }
            "emit-c" => {
                let artifacts = pipeline_lower(&db, source_file, &filepath);
                let type_check = &artifacts.type_check;
                let mut amir_owned = if opt {
                    Some(artifacts.amir.clone())
                } else {
                    None
                };
                if let Some(ref mut amir) = amir_owned {
                    arandu_base::time_pass!("optimize-amir");
                    arandu_semantics::optimize_amir(amir);
                }
                let amir = match &amir_owned {
                    Some(a) => a,
                    None => &artifacts.amir,
                };

                arandu_base::time_pass!("emit-c");
                let c_src = arandu_backend_c::emit_c(
                    amir,
                    type_check.symbols.as_ref(),
                    type_check.type_info.as_ref(),
                    &type_check.type_info.type_interner,
                    data_layout,
                );
                print!("{c_src}");
            }

            "graph" => {
                use arandu_query::db::ArandCompilerDb;
                let dep_graph = arandu_query::passes::module_dependency_graph(&db, source_file);
                let mut dot_graph = petgraph::Graph::<String, ()>::new();
                let mut node_map = std::collections::HashMap::new();
                for node in dep_graph.node_indices() {
                    let file_id = dep_graph[node];
                    let path = db.file_path(file_id);
                    let path_str = path.to_string_lossy().into_owned();
                    let new_node = dot_graph.add_node(path_str);
                    node_map.insert(node, new_node);
                }
                for edge in dep_graph.edge_indices() {
                    let Some((source, target)) = dep_graph.edge_endpoints(edge) else {
                        continue;
                    };
                    if let (Some(&s), Some(&t)) = (node_map.get(&source), node_map.get(&target)) {
                        dot_graph.add_edge(s, t, ());
                    }
                }
                println!("{:?}", petgraph::dot::Dot::with_config(&dot_graph, &[]));
            }

            _ => {
                eprintln!("Error: unknown command");
                process::exit(2);
            }
        }
    };

    if use_parallel {
        let db_mutex = std::sync::Mutex::new(db);
        source_files
            .into_par_iter()
            .for_each(|(source_file, filepath, source)| {
                let thread_db = match db_mutex.lock() {
                    Ok(guard) => guard.clone(),
                    Err(poisoned) => poisoned.into_inner().clone(),
                };
                process_file(source_file, filepath, source, thread_db);
            });
    } else {
        for (source_file, filepath, source) in source_files {
            process_file(source_file, filepath, source, db.clone());
        }
    }

    if let Some(log) = rebuild_log {
        eprint!("{}", log.format_chain(true));
    }

    arandu_base::print_perf_summary();
    arandu_base::finalize_self_profile();
}
