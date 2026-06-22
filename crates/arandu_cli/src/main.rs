#![allow(clippy::collapsible_if)]
use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
};

fn print_diagnostics_and_exit(diagnostics: &[arandu_semantics::Diagnostic], filepath: &str) -> ! {
    let mut registry = arandu_base::SourceRegistry::default();
    if !filepath.is_empty() {
        if let Ok(source) = fs::read_to_string(filepath) {
            registry.register(filepath, &source);
        }
    }
    for diagnostic in diagnostics {
        eprintln!("{}", diagnostic.format_for_cli(&registry));
    }

    process::exit(1);
}

fn print_parse_error_and_exit(err: &arandu_parser::ParseError, filepath: &str) -> ! {
    let mut registry = arandu_base::SourceRegistry::default();
    if !filepath.is_empty() {
        if let Ok(source) = fs::read_to_string(filepath) {
            registry.register(filepath, &source);
        }
    }
    eprintln!("{}", err.format_for_cli(&registry));
    process::exit(1);
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

fn parse_and_check(source: &str, filepath: &str) -> CheckedProgram {
    let mut registry = arandu_base::SourceRegistry::default();
    let file_id = registry.register(filepath, source);

    let program = match arandu_parser::parse_with_file_id(source, file_id) {
        Ok(program) => program,
        Err(err) => print_parse_error_and_exit(&err, filepath),
    };

    let resolution = arandu_semantics::resolve(&program);

    let type_check = arandu_semantics::type_check(resolution, &program);

    if !type_check.diagnostics.is_empty() {
        print_diagnostics_and_exit(&type_check.diagnostics, filepath);
    }

    CheckedProgram {
        program,
        type_check,
    }
}

fn usage_and_exit() -> ! {
    eprintln!(
        "usage: arandu_cli <lex|parse|check|hir|amir|run> <path> [--debug] [--opt] [--parallel]"
    );
    eprintln!("       -Z flags: -Ztime-passes  -Zprofile-queries  -Zprint-alloc-stats  -Zdump-mir");

    process::exit(2);
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

    for arg in env::args() {
        match arg.as_str() {
            "--debug" => debug = true,
            "--opt" => opt = true,
            "--parallel" => parallel = true,
            s if s.starts_with("-Z") => z_flags.push(arg.clone()),
            _ => args.push(arg),
        }
    }

    // Initialise global perf flags (written once, read-only afterwards).
    arandu_base::init_z_flags(&z_flags);

    if args.len() != 3 {
        usage_and_exit();
    }

    let command = &args[1];

    if !matches!(
        command.as_str(),
        "lex" | "parse" | "check" | "hir" | "amir" | "run"
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

    let use_parallel = parallel || paths.len() > 1;

    if use_parallel {
        if matches!(command.as_str(), "lex" | "parse" | "run") {
            eprintln!(
                "parallel/multi-file mode is not supported for command '{}'",
                command
            );
            process::exit(1);
        }

        match arandu_semantics::compile_parallel(paths.clone()) {
            Ok(output) => match command.as_str() {
                "check" => {
                    println!("ok");
                }
                "hir" => {
                    for (i, hir) in output.hirs.iter().enumerate() {
                        let filepath = output.paths[i].to_string_lossy();
                        println!("--- HIR for {} ---", filepath);
                        if debug {
                            println!("{hir:#?}");
                        } else {
                            let ctx = arandu_semantics::hir::HirPrettyCtx {
                                pool: &hir.pool,
                                symbols: &output.symbols[i],
                                show_spans: false,
                                type_interner: Some(&output.type_interners[i]),
                            };
                            print!("{}", hir.pretty_print(&ctx));
                        }
                    }
                }
                "amir" => {
                    for (i, amir) in output.amirs.iter().enumerate() {
                        let filepath = output.paths[i].to_string_lossy();
                        println!("--- AMIR for {} ---", filepath);
                        if debug {
                            println!("{amir:#?}");
                        } else {
                            print!(
                                "{}",
                                amir.pretty_print(&output.symbols[i], &output.type_interners[i])
                            );
                        }
                    }
                }
                _ => unreachable!(),
            },
            Err(diags) => {
                let mut registry = arandu_base::SourceRegistry::default();
                for p in &paths {
                    if let Ok(source) = fs::read_to_string(p) {
                        registry.register(&p.to_string_lossy(), &source);
                    }
                }
                for diag in diags {
                    eprintln!("{}", diag.format_for_cli(&registry));
                }
                process::exit(1);
            }
        }
    } else {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("failed to read {}: {err}", path.display());

                process::exit(1);
            }
        };

        let filepath = path.to_string_lossy();

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
                let checked = {
                    arandu_base::time_pass!("parse+check");
                    parse_and_check(&source, &filepath)
                };
                arandu_base::perf_info!("Syntax analysis and type-check completed");

                let hir = {
                    arandu_base::time_pass!("lower-hir");
                    match arandu_semantics::lower_to_hir(&checked.type_check, &checked.program) {
                        Ok(hir) => hir,
                        Err(diags) => print_diagnostics_and_exit(&diags, &filepath),
                    }
                };
                arandu_base::perf_info!("HIR lowering completed");

                validate_hir_and_analyze(&hir, &checked.type_check, &filepath);
                {
                    arandu_base::time_pass!("lower-amir");
                    if let Err(diags) = arandu_semantics::lower_to_amir(&checked.type_check, &hir) {
                        print_diagnostics_and_exit(&diags, &filepath);
                    }
                }
                arandu_base::perf_info!("Compilation verified successfully — no errors found");
                println!("ok");
            }

            "hir" => {
                let checked = {
                    arandu_base::time_pass!("parse+check");
                    parse_and_check(&source, &filepath)
                };
                let hir = {
                    arandu_base::time_pass!("lower-hir");
                    match arandu_semantics::lower_to_hir(&checked.type_check, &checked.program) {
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
                    print!("{}", hir.pretty_print(&ctx));
                }
            }

            "amir" => {
                let checked = {
                    arandu_base::time_pass!("parse+check");
                    parse_and_check(&source, &filepath)
                };
                let hir = {
                    arandu_base::time_pass!("lower-hir");
                    match arandu_semantics::lower_to_hir(&checked.type_check, &checked.program) {
                        Ok(hir) => hir,
                        Err(diags) => print_diagnostics_and_exit(&diags, &filepath),
                    }
                };
                validate_hir_and_analyze(&hir, &checked.type_check, &filepath);

                let mut amir = {
                    arandu_base::time_pass!("lower-amir");
                    match arandu_semantics::lower_to_amir(&checked.type_check, &hir) {
                        Ok(amir) => amir,
                        Err(diags) => print_diagnostics_and_exit(&diags, &filepath),
                    }
                };
                if opt {
                    arandu_base::time_pass!("optimize-amir");
                    arandu_semantics::optimize_amir(&mut amir);
                }

                if debug {
                    println!("{amir:#?}");
                } else {
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
                let checked = {
                    arandu_base::time_pass!("parse+check");
                    parse_and_check(&source, &filepath)
                };
                arandu_base::perf_info!("Syntax analysis and type-check completed");

                let hir = {
                    arandu_base::time_pass!("lower-hir");
                    match arandu_semantics::lower_to_hir(&checked.type_check, &checked.program) {
                        Ok(hir) => hir,
                        Err(diags) => print_diagnostics_and_exit(&diags, &filepath),
                    }
                };
                arandu_base::perf_info!("HIR lowering completed");
                validate_hir_and_analyze(&hir, &checked.type_check, &filepath);

                let mut amir = {
                    arandu_base::time_pass!("lower-amir");
                    match arandu_semantics::lower_to_amir(&checked.type_check, &hir) {
                        Ok(amir) => amir,
                        Err(diags) => print_diagnostics_and_exit(&diags, &filepath),
                    }
                };
                arandu_base::perf_info!("AMIR lowering completed");

                if opt {
                    arandu_base::time_pass!("optimize-amir");
                    arandu_semantics::optimize_amir(&mut amir);
                    arandu_base::perf_info!("Optimisation passes applied");
                }

                use arandu_semantics::{CodegenBackend, CompiledCode};
                let output = {
                    arandu_base::time_pass!("codegen");
                    let backend = arandu_backend_cranelift::CraneliftBackend::new();
                    match CodegenBackend::compile(backend, &amir, &checked.type_check.symbols, &())
                    {
                        Ok(out) => out,
                        Err(diag) => print_diagnostics_and_exit(&[diag], &filepath),
                    }
                };
                arandu_base::perf_info!("Machine code generated (Cranelift JIT backend)");

                arandu_base::print_perf_summary();

                unsafe {
                    if let Some(main_fn) =
                        CompiledCode::get_fn::<unsafe fn() -> i32>(&output, "main")
                    {
                        let code = main_fn();
                        process::exit(code);
                    } else {
                        eprintln!("Error: 'main' function not found in compiled program");
                        process::exit(1);
                    }
                }
            }

            _ => unreachable!(),
        }
    }

    arandu_base::print_perf_summary();
}
