use crate::TypeCheckResult;
use crate::diagnostics::{Diagnostic, Severity};
use crate::hir::HirProgram;
use arandu_parser::Program;

mod decl;
mod expr;
mod pattern;
mod place;
mod stmt;

pub fn lower_to_hir(
    type_check: &TypeCheckResult,
    program: &Program,
) -> Result<HirProgram, Vec<Diagnostic>> {
    if type_check
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        return Err(type_check.diagnostics.clone());
    }

    let mut decls = Vec::new();
    for decl in &program.decls {
        decls.push(decl::lower_decl(type_check, decl).map_err(|e| vec![e])?);
    }
    let module = program.module.as_ref().map(|m| m.path.join("."));
    Ok(HirProgram {
        span: program.span,
        module,
        decls,
    })
}
