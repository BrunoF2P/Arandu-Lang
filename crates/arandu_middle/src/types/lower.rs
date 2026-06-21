use arandu_lexer::Span;
use arandu_parser::{ResultType, TypeExpr, TypeExprId, TypeName, ast_pool::AstPool};

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
    expr_id: TypeExprId,
    pool: &AstPool,
    symbols: &SymbolTable,
    _scope: ScopeId,
    resolved: &ResolvedNames,
) -> ArType {
    let expr = pool.type_expr(expr_id);
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
            let arg_ids = pool.type_expr_list(*args);
            lower_named_type(*span, name, arg_ids, pool, symbols, _scope, resolved)
        }
        TypeExpr::Nullable { inner, .. } => {
            let inner_ty = lower_type_expr(*inner, pool, symbols, _scope, resolved);
            let id = super::type_interner::intern_type(inner_ty);
            ArType::Nullable(id)
        }
        TypeExpr::Pointer { inner, .. } => {
            let inner_ty = lower_type_expr(*inner, pool, symbols, _scope, resolved);
            let id = super::type_interner::intern_type(inner_ty);
            ArType::Ptr(id)
        }
        TypeExpr::Slice { inner, .. } => {
            let inner_ty = lower_type_expr(*inner, pool, symbols, _scope, resolved);
            let id = super::type_interner::intern_type(inner_ty);
            ArType::Slice(id)
        }
        TypeExpr::Array { size, elem, .. } => {
            let elem_ty = lower_type_expr(*elem, pool, symbols, _scope, resolved);
            let id = super::type_interner::intern_type(elem_ty);
            let n = size.parse::<u64>().unwrap_or(0);
            ArType::Array(n, id)
        }
        TypeExpr::Func { params, result, .. } => {
            let param_ids = pool.type_expr_list(*params);
            let param_types: Vec<super::type_interner::TypeId> = param_ids
                .iter()
                .map(|&p| {
                    let ty = lower_type_expr(p, pool, symbols, _scope, resolved);
                    super::type_interner::intern_type(ty)
                })
                .collect();
            let ret = match result {
                Some(r) => lower_result_type(r, pool, symbols, _scope, resolved),
                None => ArType::Void,
            };
            let ret_id = super::type_interner::intern_type(ret);
            ArType::Func(param_types, ret_id)
        }
        TypeExpr::Group { inner, .. } => lower_type_expr(*inner, pool, symbols, _scope, resolved),
    }
}

/// Convert an AST `ResultType` into an `ArType`.
#[must_use]
pub fn lower_result_type(
    result: &ResultType,
    pool: &AstPool,
    symbols: &SymbolTable,
    scope: ScopeId,
    resolved: &ResolvedNames,
) -> ArType {
    match result {
        ResultType::Single { ty, .. } => lower_type_expr(*ty, pool, symbols, scope, resolved),
        ResultType::Multi { types, .. } => {
            let list = pool.type_expr_list(*types);
            let tys: Vec<super::type_interner::TypeId> = list
                .iter()
                .map(|&t| {
                    let ty = lower_type_expr(t, pool, symbols, scope, resolved);
                    super::type_interner::intern_type(ty)
                })
                .collect();
            ArType::Tuple(tys)
        }
    }
}

pub fn lower_named_type(
    _span: Span,
    name: &TypeName,
    args: &[TypeExprId],
    pool: &AstPool,
    symbols: &SymbolTable,
    scope: ScopeId,
    resolved: &ResolvedNames,
) -> ArType {
    if type_name_base(name) == "void" && args.is_empty() {
        return ArType::Void;
    }
    if let Some(builtin) = lower_builtin_generic(name, args, pool, symbols, scope, resolved) {
        return builtin;
    }

    // The name resolver already resolved this name — look up the symbol ID.
    let key = crate::NodeKey::from(name.span);
    if let Some(&symbol_id) = resolved.type_refs.get(&key) {
        let generic_args: Vec<super::type_interner::TypeId> = args
            .iter()
            .map(|&a| {
                let ty = lower_type_expr(a, pool, symbols, scope, resolved);
                super::type_interner::intern_type(ty)
            })
            .collect();
        ArType::Named(symbol_id, generic_args)
    } else {
        // Name was not resolved — name resolver already emitted an error.
        ArType::Error
    }
}
