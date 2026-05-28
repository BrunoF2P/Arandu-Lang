//! Shared helpers for HIR and AMIR lowering passes.

use crate::diagnostics::{DiagCode, Diagnostic};
use crate::{NodeKey, ResolvedNames, SymbolId};
use arandu_lexer::Span;
use arandu_parser::ast_pool::ExprId;

/// Look up a definition symbol by source span; returns a lowering diagnostic on failure.
pub fn require_def_symbol(resolved: &ResolvedNames, span: Span) -> Result<SymbolId, Diagnostic> {
    resolved
        .definitions
        .get(&NodeKey::from(span))
        .copied()
        .ok_or_else(|| {
            Diagnostic::error(
                DiagCode::L001LoweringUnresolvedSymbol,
                "cannot lower node: symbol not resolved",
                span,
            )
        })
}

/// Look up a value reference symbol by expression id.
pub fn require_value_symbol(
    resolved: &ResolvedNames,
    expr: ExprId,
    span: Span,
) -> Result<SymbolId, Diagnostic> {
    resolved.expr_symbol(expr).ok_or_else(|| {
        Diagnostic::error(
            DiagCode::L001LoweringUnresolvedSymbol,
            "cannot lower expression: value symbol not resolved",
            span,
        )
    })
}

/// Look up a type reference symbol by source span.
pub fn require_type_symbol(resolved: &ResolvedNames, span: Span) -> Result<SymbolId, Diagnostic> {
    resolved
        .type_refs
        .get(&NodeKey::from(span))
        .copied()
        .ok_or_else(|| {
            Diagnostic::error(
                DiagCode::L001LoweringUnresolvedSymbol,
                "cannot lower type path: type symbol not resolved",
                span,
            )
        })
}
