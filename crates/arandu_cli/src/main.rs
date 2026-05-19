use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut debug = false;
    let mut clean_args = Vec::new();
    for arg in args {
        if arg == "--debug" {
            debug = true;
        } else {
            clean_args.push(arg);
        }
    }

    if clean_args.len() != 3
        || !matches!(
            clean_args[1].as_str(),
            "lex" | "parse" | "check" | "hir" | "amir"
        )
    {
        eprintln!("usage: arandu_cli <lex|parse|check|hir|amir> <path> [--debug]");
        process::exit(2);
    }

    let source = match fs::read_to_string(&clean_args[2]) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("failed to read {}: {err}", clean_args[2]);
            process::exit(1);
        }
    };

    match clean_args[1].as_str() {
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
            let filepath = clean_args[2].replace('\\', "/");
            match arandu_parser::parse(&source) {
                Ok(program) => {
                    let resolution = arandu_semantics::resolve(&program);
                    let type_check_result = arandu_semantics::type_check(resolution, &program);

                    if type_check_result.diagnostics.is_empty() {
                        println!("ok");
                    } else {
                        for diagnostic in &type_check_result.diagnostics {
                            eprintln!("{}", diagnostic.format_for_cli(&filepath));
                        }
                        process::exit(1);
                    }
                }
                Err(err) => {
                    eprintln!("{}", err.format_for_cli(&filepath));
                    process::exit(1);
                }
            }
        }
        "hir" => {
            let filepath = clean_args[2].replace('\\', "/");
            match arandu_parser::parse(&source) {
                Ok(program) => {
                    let resolution = arandu_semantics::resolve(&program);
                    let type_check_result = arandu_semantics::type_check(resolution, &program);

                    if !type_check_result.diagnostics.is_empty() {
                        for diagnostic in &type_check_result.diagnostics {
                            eprintln!("{}", diagnostic.format_for_cli(&filepath));
                        }
                        process::exit(1);
                    }

                    let hir = match arandu_semantics::lower_to_hir(&type_check_result, &program) {
                        Ok(hir) => hir,
                        Err(diags) => {
                            for diagnostic in &diags {
                                eprintln!("{}", diagnostic.format_for_cli(&filepath));
                            }
                            process::exit(1);
                        }
                    };
                    if let Err(err) = hir.validate_invariants(&type_check_result.symbols) {
                        eprintln!("HIR Invariant violation: {}", err);
                        process::exit(1);
                    }
                    if debug {
                        println!("{:#?}", hir);
                    } else {
                        let ctx = arandu_semantics::hir::HirPrettyCtx {
                            symbols: &type_check_result.symbols,
                            show_spans: false,
                        };
                        print!("{}", hir.pretty_print(&ctx));
                    }
                }
                Err(err) => {
                    eprintln!("{}", err.format_for_cli(&filepath));
                    process::exit(1);
                }
            }
        }
        "amir" => {
            let filepath = clean_args[2].replace('\\', "/");
            match arandu_parser::parse(&source) {
                Ok(program) => {
                    let resolution = arandu_semantics::resolve(&program);
                    let type_check_result = arandu_semantics::type_check(resolution, &program);

                    if !type_check_result.diagnostics.is_empty() {
                        for diagnostic in &type_check_result.diagnostics {
                            eprintln!("{}", diagnostic.format_for_cli(&filepath));
                        }
                        process::exit(1);
                    }

                    let hir = match arandu_semantics::lower_to_hir(&type_check_result, &program) {
                        Ok(hir) => hir,
                        Err(diags) => {
                            for diagnostic in &diags {
                                eprintln!("{}", diagnostic.format_for_cli(&filepath));
                            }
                            process::exit(1);
                        }
                    };
                    if let Err(err) = hir.validate_invariants(&type_check_result.symbols) {
                        eprintln!("HIR Invariant violation: {}", err);
                        process::exit(1);
                    }
                    let amir = match arandu_semantics::lower_to_amir(&type_check_result, &hir) {
                        Ok(amir) => amir,
                        Err(diags) => {
                            for diagnostic in &diags {
                                eprintln!("{}", diagnostic.format_for_cli(&filepath));
                            }
                            process::exit(1);
                        }
                    };
                    if debug {
                        println!("{:#?}", amir);
                    } else {
                        print!("{}", amir.pretty_print(&type_check_result.symbols));
                    }
                }
                Err(err) => {
                    eprintln!("{}", err.format_for_cli(&filepath));
                    process::exit(1);
                }
            }
        }
        command => {
            eprintln!("unknown command: {command}");
            process::exit(2);
        }
    }
}
