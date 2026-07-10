#![allow(clippy::collapsible_if)]
use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
};

fn print_diagnostics_and_exit(diagnostics: &[arandu_middle::Diagnostic], filepath: &str) -> ! {
    let source = if !filepath.is_empty() {
        fs::read_to_string(filepath).unwrap_or_default()
    } else {
        String::new()
    };

    let named_source = miette::NamedSource::new(filepath, source);

    for diagnostic in diagnostics {
        let report = miette::Report::new(diagnostic.clone()).with_source_code(named_source.clone());
        eprintln!("{:?}", report);
    }

    process::exit(1);
}

fn print_parse_error_and_exit(err: &arandu_parser::ParseError, filepath: &str) -> ! {
    let diag = arandu_middle::Diagnostic::from(err.clone());
    print_diagnostics_and_exit(&[diag], filepath);
}

fn validate_hir_and_analyze(
    hir: &arandu_semantics::hir::HirProgram,
    type_check: &arandu_semantics::TypeCheckResult,
    filepath: &str,
) {
    if let Err(err) = hir.validate_invariants(&hir.pool, &type_check.symbols) {
        eprintln!("HIR invariant violation: {err}");
        process::exit(1);
    }

    if let Err(diags) =
        arandu_semantics::passes::monomorphize::analyze_instantiations(type_check, hir)
    {
        print_diagnostics_and_exit(&diags, filepath);
    }
}

struct CheckedProgram {
    program: arandu_parser::Program,
    type_check: arandu_semantics::TypeCheckResult,
}

fn parse_and_check(
    db: &dyn arandu_query::db::ArandCompilerDb,
    file: arandu_query::db::SourceFile,
    filepath: &str,
) -> CheckedProgram {
    let program_res = arandu_query::passes::parse(db, file);
    let program = match &*program_res {
        Ok(program) => program.as_ref().clone(),
        Err(err) => print_parse_error_and_exit(err, filepath),
    };

    let type_check = arandu_query::passes::type_check(db, file);

    let diagnostics = arandu_query::passes::type_check::accumulated::<
        arandu_middle::db::DiagnosticsAccumulator,
    >(db, file);
    let diags: Vec<_> = diagnostics.into_iter().map(|d| d.0.clone()).collect();

    // Always print user-facing diagnostics (errors and warnings).
    // Exit only on Error / ICE — warnings must not fail `check`.
    let has_fatal = diags
        .iter()
        .any(|d| matches!(d.severity, arandu_middle::Severity::Error));
    if !diags.is_empty() {
        let source = std::fs::read_to_string(filepath).unwrap_or_default();
        let named_source = miette::NamedSource::new(filepath, source);
        for diagnostic in &diags {
            let report =
                miette::Report::new(diagnostic.clone()).with_source_code(named_source.clone());
            eprintln!("{:?}", report);
        }
        if has_fatal {
            std::process::exit(1);
        }
    }

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
    eprintln!("       -Z flags: -Ztime-passes  -Zprofile-queries  -Zprint-alloc-stats  -Zdump-mir");
    eprintln!(
        "                : -Zdebug-parser -Zdebug-typeck -Zdebug-ossa -Zdebug-layout -Zdebug-backend -Zdebug-all"
    );
    eprintln!("                : -Zself-profile=<path>  -Zexplain-rebuild");

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
            s if s.starts_with("-Z") => z_flags.push(arg.clone()),
            s if s.starts_with("--layout=") => layout_flags.push(arg.clone()),
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
                    process::exit(1);
                }
            },

            "parse" => match arandu_parser::parse_to_string(&source) {
                Ok(output) => println!("{output}"),
                Err(err) => {
                    eprintln!("{err}");
                    process::exit(1);
                }
            },

            "check" => {
                let mut checked = {
                    arandu_base::time_pass!("parse+check");
                    parse_and_check(&db, source_file, &filepath)
                };
                tracing::info!("Syntax analysis and type-check completed for {}", filepath);

                let hir = {
                    arandu_base::time_pass!("lower-hir");
                    match arandu_semantics::lower_to_hir(&mut checked.type_check, &checked.program)
                    {
                        Ok(hir) => hir,
                        Err(diags) => print_diagnostics_and_exit(&diags, &filepath),
                    }
                };
                tracing::info!("HIR lowering completed for {}", filepath);

                validate_hir_and_analyze(&hir, &checked.type_check, &filepath);
                {
                    arandu_base::time_pass!("lower-amir");
                    if let Err(diags) = arandu_semantics::lower_to_amir(&checked.type_check, &hir) {
                        print_diagnostics_and_exit(&diags, &filepath);
                    }
                }
                tracing::info!(
                    "Compilation verified successfully — no errors found for {}",
                    filepath
                );
                println!("ok {}", filepath);
            }

            "hir" => {
                let mut checked = {
                    arandu_base::time_pass!("parse+check");
                    parse_and_check(&db, source_file, &filepath)
                };
                let hir = {
                    arandu_base::time_pass!("lower-hir");
                    match arandu_semantics::lower_to_hir(&mut checked.type_check, &checked.program)
                    {
                        Ok(hir) => hir,
                        Err(diags) => print_diagnostics_and_exit(&diags, &filepath),
                    }
                };
                validate_hir_and_analyze(&hir, &checked.type_check, &filepath);

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
                let mut checked = {
                    arandu_base::time_pass!("parse+check");
                    parse_and_check(&db, source_file, &filepath)
                };
                let hir = {
                    arandu_base::time_pass!("lower-hir");
                    match arandu_semantics::lower_to_hir(&mut checked.type_check, &checked.program)
                    {
                        Ok(hir) => hir,
                        Err(diags) => print_diagnostics_and_exit(&diags, &filepath),
                    }
                };
                validate_hir_and_analyze(&hir, &checked.type_check, &filepath);

                let mut amir = {
                    arandu_base::time_pass!("lower-amir");
                    let amir_program = arandu_query::passes::lower_amir(&db, source_file);
                    (*amir_program).clone()
                };

                if opt {
                    arandu_base::time_pass!("optimize-amir");
                    arandu_semantics::optimize_amir(&mut amir);
                }

                if debug {
                    println!("{amir:#?}");
                } else {
                    println!("--- AMIR for {} ---", filepath);
                    print!(
                        "{}",
                        amir.pretty_print(
                            &checked.type_check.symbols,
                            &checked.type_check.type_info.type_interner
                        )
                    );
                }
            }

            "run" => {
                let mut checked = {
                    arandu_base::time_pass!("parse+check");
                    parse_and_check(&db, source_file, &filepath)
                };
                tracing::info!("Syntax analysis and type-check completed");

                let hir = {
                    arandu_base::time_pass!("lower-hir");
                    match arandu_semantics::lower_to_hir(&mut checked.type_check, &checked.program)
                    {
                        Ok(hir) => hir,
                        Err(diags) => print_diagnostics_and_exit(&diags, &filepath),
                    }
                };
                tracing::info!("HIR lowering completed");
                validate_hir_and_analyze(&hir, &checked.type_check, &filepath);

                let mut amir = {
                    arandu_base::time_pass!("lower-amir");
                    let amir_program = arandu_query::passes::lower_amir(&db, source_file);
                    (*amir_program).clone()
                };
                tracing::info!("AMIR lowering completed");

                if opt {
                    arandu_base::time_pass!("optimize-amir");
                    arandu_semantics::optimize_amir(&mut amir);
                    tracing::info!("Optimisation passes applied");
                }

                use arandu_semantics::{CodegenBackend, CompiledCode};
                let output = {
                    arandu_base::time_pass!("codegen");
                    let backend = match arandu_backend_cranelift::CraneliftBackend::try_new() {
                        Ok(backend) => backend,
                        Err(diag) => print_diagnostics_and_exit(&[diag], &filepath),
                    };
                    match CodegenBackend::compile(
                        backend,
                        &amir,
                        checked.type_check.symbols.as_ref(),
                        checked.type_check.type_info.as_ref(),
                    ) {
                        Ok(out) => out,
                        Err(diag) => print_diagnostics_and_exit(&[diag], &filepath),
                    }
                };
                tracing::info!("Machine code generated (Cranelift JIT backend)");

                // Resolve `main` return kind from AMIR (void → exit 0; int → process exit code).
                let main_is_void = amir.funcs.iter().any(|f| {
                    let name = checked.type_check.symbols.get(f.symbol).name.as_str();
                    if name != "main" {
                        return false;
                    }
                    matches!(
                        checked
                            .type_check
                            .type_info
                            .type_interner
                            .resolve(f.return_type),
                        arandu_semantics::types::ArType::Void
                    )
                });
                let has_main = amir.funcs.iter().any(|f| {
                    checked.type_check.symbols.get(f.symbol).name.as_str() == "main"
                });
                if !has_main {
                    eprintln!("Error: 'main' function not found in compiled program");
                    process::exit(1);
                }

                unsafe {
                    if main_is_void {
                        if let Some(main_fn) = CompiledCode::get_fn::<unsafe fn()>(&output, "main") {
                            main_fn();
                            process::exit(0);
                        }
                    } else if let Some(main_fn) =
                        CompiledCode::get_fn::<unsafe fn() -> i32>(&output, "main")
                    {
                        let code = main_fn();
                        process::exit(code);
                    }
                    eprintln!("Error: 'main' function not found in compiled program");
                    process::exit(1);
                }
            }
            "emit-c" => {
                let mut checked = {
                    arandu_base::time_pass!("parse+check");
                    parse_and_check(&db, source_file, &filepath)
                };
                let hir = {
                    arandu_base::time_pass!("lower-hir");
                    match arandu_semantics::lower_to_hir(&mut checked.type_check, &checked.program)
                    {
                        Ok(hir) => hir,
                        Err(diags) => print_diagnostics_and_exit(&diags, &filepath),
                    }
                };
                validate_hir_and_analyze(&hir, &checked.type_check, &filepath);

                let mut amir = {
                    arandu_base::time_pass!("lower-amir");
                    let amir_program = arandu_query::passes::lower_amir(&db, source_file);
                    (*amir_program).clone()
                };
                if opt {
                    arandu_base::time_pass!("optimize-amir");
                    arandu_semantics::optimize_amir(&mut amir);
                }

                arandu_base::time_pass!("emit-c");
                let c_src = arandu_backend_c::emit_c(
                    &amir,
                    checked.type_check.symbols.as_ref(),
                    checked.type_check.type_info.as_ref(),
                    &checked.type_check.type_info.type_interner,
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
                    let (source, target) = dep_graph.edge_endpoints(edge).unwrap();
                    dot_graph.add_edge(node_map[&source], node_map[&target], ());
                }
                println!("{:?}", petgraph::dot::Dot::with_config(&dot_graph, &[]));
            }

            _ => unreachable!(),
        }
    };

    if use_parallel {
        let db_mutex = std::sync::Mutex::new(db);
        source_files
            .into_par_iter()
            .for_each(|(source_file, filepath, source)| {
                let thread_db = db_mutex.lock().unwrap().clone();
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
