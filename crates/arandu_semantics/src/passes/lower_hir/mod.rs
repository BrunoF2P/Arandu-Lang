//! HIR lowering pass.
//!
//! Transforms a type-checked [`Program`] AST into a [`HirProgram`] (High-level IR).
//! Must run after type-checking; aborts early if any errors are already present.

use crate::TypeCheckResult;
use crate::diagnostics::{Diagnostic, Severity};
use crate::hir::HirProgram;
use arandu_parser::Program;

mod decl;
mod expr;
mod link;
mod materialize;
mod pattern;
mod place;
mod stmt;

pub use link::link_hir_module;
pub use materialize::{MaterializationError, materialize_ctfe_scalar, materialize_ctfe_value};

/// Lower only an initially typed expression into the caller's HIR context.
/// The canonical AST/pool and declaration headers remain shared by the caller;
/// this does not lower the containing function or evaluate source syntax.
pub fn lower_expression_to_hir(
    type_check: &mut TypeCheckResult,
    pool: &arandu_parser::ast_pool::AstPool,
    hir: &mut HirProgram,
    expression: arandu_parser::ast_pool::ExprId,
) -> Result<crate::hir::HirExprId, Vec<Diagnostic>> {
    if type_check
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        return Err(type_check.diagnostics.clone());
    }
    expr::lower_expr(type_check, pool, &mut hir.pool, expression)
        .map_err(|diagnostic| vec![diagnostic])
}

/// Lower an initially typed isolated block into an existing declaration context.
/// No headers are rebuilt, no enclosing/sibling body is lowered, and no
/// callable/synthetic symbol is added.
/// The root's return type and semicolon-aware tail are supplied separately by
/// its initial checker; the containing function header is not its return target.
pub fn lower_block_to_hir(
    type_check: &mut TypeCheckResult,
    pool: &arandu_parser::ast_pool::AstPool,
    hir: &mut HirProgram,
    block: &arandu_parser::Block,
) -> Result<crate::hir::HirBlockId, Vec<Diagnostic>> {
    if type_check
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        return Err(type_check.diagnostics.clone());
    }
    stmt::lower_block(type_check, pool, &mut hir.pool, block).map_err(|diagnostic| vec![diagnostic])
}

/// Canonical HIR declaration context, including call modes and destructor
/// associations, but without lowering any function body. Constants must have
/// been typed by the caller through their ordinary item checker.
pub fn lower_declarations_to_hir(
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
    let mut pool = crate::hir::HirPool::new();
    let mut decls = Vec::new();
    let mut failure = None;
    program.for_each_decl_recursive(|_, declaration| {
        if failure.is_some() {
            return;
        }
        match decl::lower_declaration(type_check, &program.pool, &mut pool, declaration) {
            Ok(Some(declaration)) => decls.push(pool.alloc_decl(declaration)),
            Ok(None) => {}
            Err(diagnostic) => failure = Some(diagnostic),
        }
    });
    if let Some(diagnostic) = failure {
        return Err(vec![diagnostic]);
    }
    Ok(HirProgram {
        span: program.span,
        module: program.module.as_ref().map(|module| module.path.join(".")),
        decls,
        pool,
    })
}

/// Lowers a type-checked AST into a [`HirProgram`].
///
/// Returns `Err` immediately if `type_check` already contains any
/// [`Severity::Error`] diagnostics, avoiding work on a broken program.
#[tracing::instrument(
    level = "trace",
    target = "arandu_semantics",
    skip(type_check, program)
)]
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
    lower_decls_recursive(
        type_check,
        &program.pool,
        &mut hir_pool,
        &program.decls,
        &mut decls,
    )?;
    let module = program.module.as_ref().map(|m| m.path.join("."));
    Ok(HirProgram {
        span: program.span,
        module,
        decls,
        pool: hir_pool,
    })
}

/// Lower only the selected function using the canonical declaration/body
/// lowering. The caller supplies its item typecheck result and owns any later
/// specialization. No sibling body or AST pool is cloned or lowered here.
pub fn lower_function_to_hir(
    type_check: &mut TypeCheckResult,
    program: &Program,
    symbol: crate::SymbolId,
) -> Result<Option<HirProgram>, Vec<Diagnostic>> {
    lower_selected_declaration(type_check, program, symbol, false)
}

/// Retain one external declaration container, including its ABI and generic
/// signatures, without lowering unrelated declarations or function bodies.
pub fn lower_extern_to_hir(
    type_check: &mut TypeCheckResult,
    program: &Program,
    symbol: crate::SymbolId,
) -> Result<Option<HirProgram>, Vec<Diagnostic>> {
    lower_selected_declaration(type_check, program, symbol, true)
}

fn lower_selected_declaration(
    type_check: &mut TypeCheckResult,
    program: &Program,
    symbol: crate::SymbolId,
    external: bool,
) -> Result<Option<HirProgram>, Vec<Diagnostic>> {
    if type_check
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        return Err(type_check.diagnostics.clone());
    }
    let mut selected = None;
    program.for_each_decl_recursive(|_, item| {
        let matches = if external {
            matches!(item, arandu_parser::TopLevelDecl::Extern(declaration)
                if declaration.members.iter().any(|member|
                    type_check.resolved.definitions.get(&member.span.into()) == Some(&symbol)))
        } else {
            matches!(item, arandu_parser::TopLevelDecl::Func(_))
                && crate::primary_def_key(item)
                    .and_then(|key| type_check.resolved.definitions.get(&key))
                    == Some(&symbol)
        };
        if matches {
            selected = Some(item);
        }
    });
    let Some(item) = selected else {
        return Ok(None);
    };
    let mut pool = crate::hir::HirPool::new();
    let Some(declaration) = decl::lower_decl(type_check, &program.pool, &mut pool, item)
        .map_err(|error| vec![error])?
    else {
        return Ok(None);
    };
    let declaration = pool.alloc_decl(declaration);
    Ok(Some(HirProgram {
        span: crate::item_source_span(item),
        module: program.module.as_ref().map(|module| module.path.join(".")),
        decls: vec![declaration],
        pool,
    }))
}

fn lower_decls_recursive(
    type_check: &mut TypeCheckResult,
    pool: &arandu_parser::ast_pool::AstPool,
    hir_pool: &mut crate::hir::HirPool,
    decl_ids: &[arandu_parser::DeclId],
    decls: &mut Vec<crate::hir::HirDeclId>,
) -> Result<(), Vec<Diagnostic>> {
    for decl_id in decl_ids {
        let decl = pool.decl(*decl_id);
        if let arandu_parser::TopLevelDecl::Submodule(submod) = decl {
            lower_decls_recursive(type_check, pool, hir_pool, &submod.decls, decls)?;
        } else if let Some(hir_decl) =
            decl::lower_decl(type_check, pool, hir_pool, decl).map_err(|e| vec![e])?
        {
            let hir_decl_id = hir_pool.alloc_decl(hir_decl);
            decls.push(hir_decl_id);
        }
    }
    Ok(())
}

// population is done during decl lowering; no separate backfill required.
