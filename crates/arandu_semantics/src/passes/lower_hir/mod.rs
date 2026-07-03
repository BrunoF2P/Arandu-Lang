use crate::TypeCheckResult;
use crate::diagnostics::{Diagnostic, Severity};
use crate::hir::HirProgram;
use arandu_parser::Program;

mod decl;
mod expr;
mod pattern;
mod place;
mod stmt;

#[tracing::instrument(level = "trace", target = "arandu_semantics", skip(type_check, program))]
pub fn lower_to_hir(
    type_check: &mut TypeCheckResult,
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
    for decl_id in &program.decls {
        let decl = program.pool.decl(*decl_id);
        if let Some(hir_decl) =
            decl::lower_decl(type_check, &program.pool, &mut hir_pool, decl).map_err(|e| vec![e])?
        {
            let hir_decl_id = hir_pool.alloc_decl(hir_decl);
            decls.push(hir_decl_id);
        }
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
