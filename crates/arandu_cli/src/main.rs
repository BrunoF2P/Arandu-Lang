use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 || !matches!(args[1].as_str(), "lex" | "parse" | "check") {
        eprintln!("usage: arandu_cli <lex|parse|check> <path>");
        process::exit(2);
    }

    let source = match fs::read_to_string(&args[2]) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("failed to read {}: {err}", args[2]);
            process::exit(1);
        }
    };

    match args[1].as_str() {
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
        "check" => match arandu_parser::parse(&source) {
            Ok(program) => {
                let resolution = arandu_semantics::resolve(&program);
                let type_check_result = arandu_semantics::type_check(resolution, &program);

                if type_check_result.diagnostics.is_empty() {
                    println!("ok");
                } else {
                    for diagnostic in &type_check_result.diagnostics {
                        eprint!("{diagnostic}");
                    }
                }
                if !type_check_result.diagnostics.is_empty() {
                    process::exit(1);
                }
            }
            Err(err) => {
                eprintln!("{err}");
                process::exit(1);
            }
        },
        command => {
            eprintln!("unknown command: {command}");
            process::exit(2);
        }
    }
}
