use arandu_lexer::Span;
use arandu_parser::{ResultType, TypeExpr, TypeName};

use super::ar_type::ArType;
use super::primitive::Primitive;
use super::result_option::{lower_builtin_generic, type_name_base};
use crate::{ResolvedNames, ScopeId, SymbolTable};

// ── Lowering from AST TypeExpr to ArType ────────────────────────────

/// Convert an AST `TypeExpr` into an internal `ArType`.
///
/// Uses the symbol table and resolved names to resolve named types to
/// their `SymbolId`. Returns `ArType::Error` for types that cannot be
/// resolved (the name resolver already reported the error).
#[must_use]
pub fn lower_type_expr(
    expr: &TypeExpr,
    symbols: &SymbolTable,
    _scope: ScopeId,
    resolved: &ResolvedNames,
) -> ArType {
    match expr {
        TypeExpr::Primitive { name, .. } => {
            if name == "Err" {
                return ArType::Err;
            }
            match Primitive::from_name(name) {
                Some(p) => ArType::Primitive(p),
                None => ArType::Error,
            }
        }
        TypeExpr::Named { span, name, args } => {
            lower_named_type(*span, name, args, symbols, _scope, resolved)
        }
        TypeExpr::Nullable { inner, .. } => {
            let inner_ty = lower_type_expr(inner, symbols, _scope, resolved);
            ArType::Nullable(Box::new(inner_ty))
        }
        TypeExpr::Pointer { inner, .. } => {
            let inner_ty = lower_type_expr(inner, symbols, _scope, resolved);
            ArType::Ptr(Box::new(inner_ty))
        }
        TypeExpr::Slice { inner, .. } => {
            let inner_ty = lower_type_expr(inner, symbols, _scope, resolved);
            ArType::Slice(Box::new(inner_ty))
        }
        TypeExpr::Array { size, elem, .. } => {
            let elem_ty = lower_type_expr(elem, symbols, _scope, resolved);
            let n = size.parse::<u64>().unwrap_or(0);
            ArType::Array(n, Box::new(elem_ty))
        }
        TypeExpr::Func { params, result, .. } => {
            let param_types: Vec<ArType> = params
                .iter()
                .map(|p| lower_type_expr(p, symbols, _scope, resolved))
                .collect();
            let ret = match result {
                Some(r) => lower_result_type(r, symbols, _scope, resolved),
                None => ArType::Void,
            };
            ArType::Func(param_types, Box::new(ret))
        }
        TypeExpr::Group { inner, .. } => lower_type_expr(inner, symbols, _scope, resolved),
    }
}

/// Convert an AST `ResultType` into an `ArType`.
#[must_use]
pub fn lower_result_type(
    result: &ResultType,
    symbols: &SymbolTable,
    scope: ScopeId,
    resolved: &ResolvedNames,
) -> ArType {
    match result {
        ResultType::Single { ty, .. } => lower_type_expr(ty, symbols, scope, resolved),
        ResultType::Multi { types, .. } => {
            let tys: Vec<ArType> = types
                .iter()
                .map(|t| lower_type_expr(t, symbols, scope, resolved))
                .collect();
            ArType::Tuple(tys)
        }
    }
}

pub(crate) fn lower_named_type(
    _span: Span,
    name: &TypeName,
    args: &[TypeExpr],
    symbols: &SymbolTable,
    scope: ScopeId,
    resolved: &ResolvedNames,
) -> ArType {
    if type_name_base(name) == "void" && args.is_empty() {
        return ArType::Void;
    }
    if let Some(builtin) = lower_builtin_generic(name, args, symbols, scope, resolved) {
        return builtin;
    }

    // The name resolver already resolved this name — look up the symbol ID.
    let key = crate::NodeKey::from(name.span);
    if let Some(&symbol_id) = resolved.type_refs.get(&key) {
        let generic_args: Vec<ArType> = args
            .iter()
            .map(|a| lower_type_expr(a, symbols, scope, resolved))
            .collect();
        ArType::Named(symbol_id, generic_args)
    } else {
        // Name was not resolved — name resolver already emitted an error.
        ArType::Error
    }
}
