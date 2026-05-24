use std::{
    env,
    fs,
    path::Path,
    process,
};

fn print_diagnostics_and_exit(
    diagnostics: &[arandu_semantics::Diagnostic],
    filepath: &str,
) -> ! {
    for diagnostic in diagnostics {
        eprintln!("{}", diagnostic.format_for_cli(filepath));
    }

    process::exit(1);
}

fn print_parse_error_and_exit(
    err: &arandu_parser::ParseError,
    filepath: &str,
) -> ! {
    eprintln!("{}", err.format_for_cli(filepath));
    process::exit(1);
}

struct CheckedProgram {
    program: arandu_parser::Program,
    type_check: arandu_semantics::TypeCheckResult,
}

fn parse_and_check(
    source: &str,
    filepath: &str,
) -> CheckedProgram {
    let program = match arandu_parser::parse(source) {
        Ok(program) => program,
        Err(err) => print_parse_error_and_exit(&err, filepath),
    };

    let resolution = arandu_semantics::resolve(&program);

    let type_check =
        arandu_semantics::type_check(resolution, &program);

    if !type_check.diagnostics.is_empty() {
        print_diagnostics_and_exit(
            &type_check.diagnostics,
            filepath,
        );
    }

    CheckedProgram {
        program,
        type_check,
    }
}

fn usage_and_exit() -> ! {
    eprintln!(
        "usage: arandu_cli <lex|parse|check|hir|amir> <path> [--debug]"
    );

    process::exit(2);
}

fn main() {
    let mut debug = false;
    let mut args = Vec::new();

    for arg in env::args() {
        if arg == "--debug" {
            debug = true;
        } else {
            args.push(arg);
        }
    }

    if args.len() != 3 {
        usage_and_exit();
    }

    let command = &args[1];

    if !matches!(
        command.as_str(),
        "lex" | "parse" | "check" | "hir" | "amir"
    ) {
        usage_and_exit();
    }

    let path = Path::new(&args[2]);

    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) => {
            eprintln!(
                "failed to read {}: {err}",
                path.display()
            );

            process::exit(1);
        }
    };

    let filepath = path.to_string_lossy();

    match command.as_str() {
        "lex" => {
            match arandu_lexer::lex_to_string(&source) {
                Ok(output) => println!("{output}"),
                Err(err) => {
                    eprintln!("{err}");
                    process::exit(1);
                }
            }
        }

        "parse" => {
            match arandu_parser::parse_to_string(&source) {
                Ok(output) => println!("{output}"),
                Err(err) => {
                    eprintln!("{err}");
                    process::exit(1);
                }
            }
        }

        "check" => {
            parse_and_check(&source, &filepath);
            println!("ok");
        }

        "hir" => {
            let checked =
                parse_and_check(&source, &filepath);

            let hir = match arandu_semantics::lower_to_hir(
                &checked.type_check,
                &checked.program,
            ) {
                Ok(hir) => hir,
                Err(diags) => {
                    print_diagnostics_and_exit(
                        &diags,
                        &filepath,
                    )
                }
            };

            if let Err(err) =
                hir.validate_invariants(
                    &checked.type_check.symbols,
                )
            {
                eprintln!(
                    "HIR invariant violation: {err}"
                );

                process::exit(1);
            }

            if debug {
                println!("{hir:#?}");
            } else {
                let ctx =
                    arandu_semantics::hir::HirPrettyCtx {
                        symbols: &checked.type_check.symbols,
                        show_spans: false,
                    };

                print!("{}", hir.pretty_print(&ctx));
            }
        }

        "amir" => {
            let checked =
                parse_and_check(&source, &filepath);

            let hir = match arandu_semantics::lower_to_hir(
                &checked.type_check,
                &checked.program,
            ) {
                Ok(hir) => hir,
                Err(diags) => {
                    print_diagnostics_and_exit(
                        &diags,
                        &filepath,
                    )
                }
            };

            if let Err(err) =
                hir.validate_invariants(
                    &checked.type_check.symbols,
                )
            {
                eprintln!(
                    "HIR invariant violation: {err}"
                );

                process::exit(1);
            }

            let amir =
                match arandu_semantics::lower_to_amir(
                    &checked.type_check,
                    &hir,
                ) {
                    Ok(amir) => amir,
                    Err(diags) => {
                        print_diagnostics_and_exit(
                            &diags,
                            &filepath,
                        )
                    }
                };

            if debug {
                println!("{amir:#?}");
            } else {
                print!(
                    "{}",
                    amir.pretty_print(
                        &checked.type_check.symbols
                    )
                );
            }
        }

        _ => unreachable!(),
    }
}