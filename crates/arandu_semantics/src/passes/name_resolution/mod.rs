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
    Resolver::new(&program.pool).resolve_program(program)
}

struct Resolver<'a> {
    symbols: SymbolTable,
    resolved: ResolvedNames,
    docs: crate::DocCommentMap,
    diagnostics: Vec<crate::Diagnostic>,
    pool: &'a arandu_parser::ast_pool::AstPool,
}
