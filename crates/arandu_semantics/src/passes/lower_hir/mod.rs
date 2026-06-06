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
    // Create a HirPool to seed HIR allocations for future ID-based lowering.
    let mut hir_pool = crate::hir::HirPool::new();
    for decl in &program.decls {
        let hir_decl = decl::lower_decl(type_check, &program.pool, &mut hir_pool, decl)
            .map_err(|e| vec![e])?;
        decls.push(hir_decl);
    }
    let module = program.module.as_ref().map(|m| m.path.join("."));
    Ok(HirProgram {
        span: program.span,
        module,
        decls,
        pool: hir_pool,
    })
}

// population is done during decl lowering; no separate backfill required.
