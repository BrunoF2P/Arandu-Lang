use arandu_backend_c::CEmitter;
use arandu_middle::layout::LayoutEngine;
use arandu_semantics::{
    CompileSession, lower_to_amir, lower_to_hir, resolve, type_check_with_session,
};
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: emit_c <path_to_aru>");
        std::process::exit(1);
    }
    let path = Path::new(&args[1]);
    let source = fs::read_to_string(path).expect("failed to read input file");

    let program = arandu_parser::parse(&source).expect("parse failed");
    let mut session = CompileSession::new();
    let resolution = resolve(&program);
    let mut tc = type_check_with_session(resolution, &program, &mut session);
    if !tc.diagnostics.is_empty() {
        eprintln!("Type check failed: {:?}", tc.diagnostics);
        std::process::exit(1);
    }

    let hir = lower_to_hir(&mut tc, &program).expect("HIR lowering failed");
    let amir = lower_to_amir(&tc, &hir).expect("AMIR lowering failed");

    let layout_engine = LayoutEngine::new(8);
    let emitter = CEmitter::new(
        &amir,
        &tc.symbols,
        &layout_engine,
        &tc.type_info,
        &tc.type_info.type_interner,
    );
    let c_code = emitter.emit();
    println!("{}", c_code);
}
