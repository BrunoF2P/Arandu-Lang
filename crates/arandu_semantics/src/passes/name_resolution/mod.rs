use arandu_parser::Program;

use crate::{ResolutionResult, ResolvedNames, SymbolTable};

mod collect;
mod decls;
mod expr;
mod program;
mod stmt;
mod symbols;
mod types;
mod util;

#[must_use]
pub fn resolve(program: &Program) -> ResolutionResult {
    Resolver::new().resolve_program(program)
}

struct Resolver {
    symbols: SymbolTable,
    resolved: ResolvedNames,
    docs: crate::DocCommentMap,
    diagnostics: Vec<crate::Diagnostic>,
}
