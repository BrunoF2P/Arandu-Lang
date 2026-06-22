use arandu_parser::{ResultType, TypeExprId, TypeName, ast_pool::AstPool};

use super::ar_type::ArType;
use crate::{ResolvedNames, ScopeId, SymbolTable};

#[must_use]
pub fn result_type_decl_span(result: &ResultType) -> arandu_lexer::Span {
    match result {
        ResultType::Single { span, .. } | ResultType::Multi { span, .. } => *span,
    }
}

#[must_use]
pub fn type_name_base(name: &TypeName) -> &str {
    name.path.last().map_or("", std::string::String::as_str)
}

#[must_use]
pub fn is_err_type(ty: &ArType) -> bool {
    matches!(ty, ArType::Err)
        || matches!(
            ty,
            ArType::Nullable(inner) if super::type_interner::with_resolved_type(*inner, |t| matches!(t, ArType::Err))
        )
}

/// Extract ok/err from `Result<T,E>`.
#[must_use]
pub fn result_ok_err(ty: &ArType) -> Option<(ArType, ArType)> {
    match ty {
        ArType::Result(ok, err) => {
            let ok_ty = super::type_interner::with_resolved_type(*ok, |t| t.clone());
            let err_ty = super::type_interner::with_resolved_type(*err, |t| t.clone());
            Some((ok_ty, err_ty))
        }
        _ => None,
    }
}

#[must_use]
pub fn is_result_type(ty: &ArType) -> bool {
    result_ok_err(ty).is_some()
}

#[must_use]
pub fn is_option_type(ty: &ArType) -> bool {
    matches!(ty, ArType::Option(_))
}

/// Types that support the `?` operator.
#[must_use]
pub fn try_ok_type(ty: &ArType) -> Option<ArType> {
    if let Some((ok, _)) = result_ok_err(ty) {
        return Some(ok);
    }
    match ty {
        ArType::Option(inner) => Some(super::type_interner::with_resolved_type(*inner, |t| {
            t.clone()
        })),
        ArType::Nullable(inner) if !is_err_type(ty) => {
            Some(super::type_interner::with_resolved_type(*inner, |t| {
                t.clone()
            }))
        }
        _ => None,
    }
}

#[must_use]
pub fn is_tryable_type(ty: &ArType) -> bool {
    try_ok_type(ty).is_some()
}

pub(crate) fn lower_builtin_generic(
    name: &TypeName,
    args: &[TypeExprId],
    pool: &AstPool,
    symbols: &SymbolTable,
    scope: ScopeId,
    resolved: &ResolvedNames,
) -> Option<ArType> {
    let base = type_name_base(name);
    let lowered: Vec<ArType> = args
        .iter()
        .map(|&a| super::lower::lower_type_expr(a, pool, symbols, scope, resolved))
        .collect();
    match (base, lowered.len()) {
        ("Result", 2) => {
            let ok_id = super::type_interner::intern_type(lowered[0].clone());
            let err_id = super::type_interner::intern_type(lowered[1].clone());
            Some(ArType::Result(ok_id, err_id))
        }
        ("Option", 1) => {
            let id = super::type_interner::intern_type(lowered[0].clone());
            Some(ArType::Option(id))
        }
        ("Coroutine", 1) => {
            let id = super::type_interner::intern_type(lowered[0].clone());
            Some(ArType::Coroutine(id))
        }
        _ => None,
    }
}
