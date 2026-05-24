use arandu_parser::{ResultType, TypeExpr, TypeName};

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
            ArType::Nullable(inner) if matches!(**inner, ArType::Err)
        )
}

/// Extract ok/err from `Result<T,E>`.
#[must_use]
pub fn result_ok_err(ty: &ArType) -> Option<(ArType, ArType)> {
    match ty {
        ArType::Result(ok, err) => Some((ok.as_ref().clone(), err.as_ref().clone())),
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
        ArType::Option(inner) => Some(inner.as_ref().clone()),
        ArType::Nullable(inner) if !is_err_type(ty) => Some(inner.as_ref().clone()),
        _ => None,
    }
}

#[must_use]
pub fn is_tryable_type(ty: &ArType) -> bool {
    try_ok_type(ty).is_some()
}

pub(crate) fn lower_builtin_generic(
    name: &TypeName,
    args: &[TypeExpr],
    symbols: &SymbolTable,
    scope: ScopeId,
    resolved: &ResolvedNames,
) -> Option<ArType> {
    let base = type_name_base(name);
    let lowered: Vec<ArType> = args
        .iter()
        .map(|a| super::lower::lower_type_expr(a, symbols, scope, resolved))
        .collect();
    match (base, lowered.len()) {
        ("Result", 2) => Some(ArType::Result(
            Box::new(lowered[0].clone()),
            Box::new(lowered[1].clone()),
        )),
        ("Option", 1) => Some(ArType::Option(Box::new(lowered[0].clone()))),
        _ => None,
    }
}
