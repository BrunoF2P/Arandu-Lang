use arandu_parser::{ResultType, TypeExprId, TypeName};

use super::ar_type::ArType;
use super::lower::LowerCtx;
use super::type_interner::TypeInterner;

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
pub fn is_err_type(ty: &ArType, interner: &TypeInterner) -> bool {
    matches!(ty, ArType::Err)
        || matches!(
            ty,
            ArType::Nullable(inner) if matches!(interner.resolve(*inner), ArType::Err)
        )
}

/// Extract ok/err from `Result<T,E>`.
#[must_use]
pub fn result_ok_err(ty: &ArType, interner: &TypeInterner) -> Option<(ArType, ArType)> {
    match ty {
        ArType::Result(ok, err) => {
            let ok_ty = interner.resolve(*ok).clone();
            let err_ty = interner.resolve(*err).clone();
            Some((ok_ty, err_ty))
        }
        _ => None,
    }
}

#[must_use]
pub fn is_result_type(ty: &ArType, interner: &TypeInterner) -> bool {
    result_ok_err(ty, interner).is_some()
}

#[must_use]
pub fn is_option_type(ty: &ArType) -> bool {
    matches!(ty, ArType::Option(_))
}

/// Types that support the `?` operator.
#[must_use]
pub fn try_ok_type(ty: &ArType, interner: &TypeInterner) -> Option<ArType> {
    if let Some((ok, _)) = result_ok_err(ty, interner) {
        return Some(ok);
    }
    match ty {
        ArType::Option(inner) => Some(interner.resolve(*inner).clone()),
        ArType::Nullable(inner) if !is_err_type(ty, interner) => {
            Some(interner.resolve(*inner).clone())
        }
        _ => None,
    }
}

#[must_use]
pub fn is_tryable_type(ty: &ArType, interner: &TypeInterner) -> bool {
    try_ok_type(ty, interner).is_some()
}

pub(crate) fn lower_builtin_generic(
    name: &TypeName,
    args: &[TypeExprId],
    ctx: &LowerCtx<'_>,
    interner: &mut TypeInterner,
) -> Option<ArType> {
    let base = type_name_base(name);
    let lowered: Vec<ArType> = args
        .iter()
        .map(|&a| super::lower::lower_type_expr_ctx(a, ctx, interner))
        .collect();
    match (base, lowered.len()) {
        ("Result", 2) => {
            let ok_id = interner.intern(lowered[0].clone());
            let err_id = interner.intern(lowered[1].clone());
            Some(ArType::Result(ok_id, err_id))
        }
        ("Option", 1) => {
            let id = interner.intern(lowered[0].clone());
            Some(ArType::Option(id))
        }
        ("Coroutine", 1) => {
            let id = interner.intern(lowered[0].clone());
            Some(ArType::Coroutine(id))
        }
        _ => None,
    }
}
