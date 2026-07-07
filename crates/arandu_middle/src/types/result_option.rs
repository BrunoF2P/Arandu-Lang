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
    name.path.last().map_or("", |s| s.as_str())
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
            let ok_ty = interner.resolve(*ok);
            let err_ty = interner.resolve(*err);
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
        ArType::Option(inner) => Some(interner.resolve(*inner)),
        ArType::Nullable(inner) if !is_err_type(ty, interner) => Some(interner.resolve(*inner)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Primitive;

    fn interner() -> TypeInterner {
        TypeInterner::new()
    }

    fn int_id(i: &mut TypeInterner) -> super::super::type_interner::TypeId {
        i.intern(ArType::Primitive(Primitive::Int))
    }
    fn str_id(i: &mut TypeInterner) -> super::super::type_interner::TypeId {
        i.intern(ArType::Primitive(Primitive::Str))
    }
    // ── is_err_type ──

    #[test]
    fn err_is_err_type() {
        let i = interner();
        assert!(is_err_type(&ArType::Err, &i));
    }

    #[test]
    fn nullable_err_is_err_type() {
        let mut i = interner();
        let inner = i.intern(ArType::Err);
        assert!(is_err_type(&ArType::Nullable(inner), &i));
    }

    #[test]
    fn int_is_not_err() {
        let i = interner();
        assert!(!is_err_type(&ArType::Primitive(Primitive::Int), &i));
    }

    // ── result_ok_err ──

    #[test]
    fn result_ok_err_extracts() {
        let mut i = interner();
        let ok = int_id(&mut i);
        let err = str_id(&mut i);
        let r = ArType::Result(ok, err);
        let (got_ok, got_err) = result_ok_err(&r, &i).unwrap();
        assert_eq!(got_ok, ArType::Primitive(Primitive::Int));
        assert_eq!(got_err, ArType::Primitive(Primitive::Str));
    }

    #[test]
    fn non_result_returns_none() {
        let i = interner();
        assert!(result_ok_err(&ArType::Primitive(Primitive::Int), &i).is_none());
        assert!(result_ok_err(&ArType::Void, &i).is_none());
    }

    // ── is_result_type ──

    #[test]
    fn is_result_type_true() {
        let mut i = interner();
        let r = ArType::Result(int_id(&mut i), str_id(&mut i));
        assert!(is_result_type(&r, &i));
    }

    #[test]
    fn is_result_type_false() {
        let i = interner();
        assert!(!is_result_type(&ArType::Primitive(Primitive::Int), &i));
        assert!(!is_result_type(
            &ArType::Option(int_id(&mut interner())),
            &i
        ));
    }

    // ── is_option_type ──

    #[test]
    fn option_type_recognized() {
        assert!(is_option_type(&ArType::Option(int_id(&mut interner()))));
    }

    #[test]
    fn non_option_not_recognized() {
        assert!(!is_option_type(&ArType::Primitive(Primitive::Int)));
        assert!(!is_option_type(&ArType::Result(
            int_id(&mut interner()),
            str_id(&mut interner())
        )));
    }

    // ── try_ok_type ──

    #[test]
    fn try_ok_from_result() {
        let mut i = interner();
        let r = ArType::Result(int_id(&mut i), str_id(&mut i));
        assert_eq!(try_ok_type(&r, &i), Some(ArType::Primitive(Primitive::Int)));
    }

    #[test]
    fn try_ok_from_option() {
        let mut i = interner();
        let opt = ArType::Option(int_id(&mut i));
        assert_eq!(
            try_ok_type(&opt, &i),
            Some(ArType::Primitive(Primitive::Int))
        );
    }

    #[test]
    fn try_ok_from_nullable_non_err() {
        let mut i = interner();
        let null = ArType::Nullable(int_id(&mut i));
        assert_eq!(
            try_ok_type(&null, &i),
            Some(ArType::Primitive(Primitive::Int))
        );
    }

    #[test]
    fn try_ok_from_nullable_err_returns_none() {
        let mut i = interner();
        let inner = i.intern(ArType::Err);
        let null_err = ArType::Nullable(inner);
        assert_eq!(try_ok_type(&null_err, &i), None);
    }

    #[test]
    fn try_ok_non_tryable_returns_none() {
        let i = interner();
        assert_eq!(try_ok_type(&ArType::Primitive(Primitive::Int), &i), None);
        assert_eq!(try_ok_type(&ArType::Void, &i), None);
    }

    // ── is_tryable_type ──

    #[test]
    fn result_option_and_non_err_nullable_are_tryable() {
        let mut i = interner();
        assert!(is_tryable_type(
            &ArType::Result(int_id(&mut i), str_id(&mut i)),
            &i
        ));
        assert!(is_tryable_type(&ArType::Option(int_id(&mut i)), &i));
        assert!(is_tryable_type(&ArType::Nullable(int_id(&mut i)), &i));
    }

    #[test]
    fn nullable_err_is_not_tryable() {
        let mut i = interner();
        let inner = i.intern(ArType::Err);
        assert!(!is_tryable_type(&ArType::Nullable(inner), &i));
    }

    #[test]
    fn plain_type_not_tryable() {
        let i = interner();
        assert!(!is_tryable_type(&ArType::Primitive(Primitive::Int), &i));
    }

    // ── type_name_base ──

    #[test]
    fn type_name_base_single_path() {
        let name = arandu_parser::TypeName {
            span: arandu_lexer::Span::new(0, 0, 0),
            path: vec![smol_str::SmolStr::new("Result")].into(),
        };
        assert_eq!(type_name_base(&name), "Result");
    }

    #[test]
    fn type_name_base_multi_path() {
        let name = arandu_parser::TypeName {
            span: arandu_lexer::Span::new(0, 0, 0),
            path: vec![
                smol_str::SmolStr::new("std"),
                smol_str::SmolStr::new("core"),
                smol_str::SmolStr::new("String"),
            ]
            .into(),
        };
        assert_eq!(type_name_base(&name), "String");
    }

    #[test]
    fn type_name_base_empty_path() {
        let name = arandu_parser::TypeName {
            span: arandu_lexer::Span::new(0, 0, 0),
            path: smallvec::SmallVec::new(),
        };
        assert_eq!(type_name_base(&name), "");
    }

    // ── lower_builtin_generic ──

    #[test]
    fn lower_builtin_wrong_name_returns_none() {
        let mut i = interner();
        let name = arandu_parser::TypeName {
            span: arandu_lexer::Span::new(0, 0, 0),
            path: vec![smol_str::SmolStr::new("NonExistent")].into(),
        };
        let result = lower_builtin_generic(&name, &[], &create_dummy_ctx(), &mut i);
        assert!(result.is_none());
    }

    fn create_dummy_ctx() -> LowerCtx<'static> {
        use crate::ResolvedNames;
        let pool = Box::new(arandu_parser::ast_pool::AstPool::new());
        let symbols = Box::new(crate::SymbolTable::new(0));
        let resolved = Box::new(ResolvedNames::default());
        LowerCtx {
            pool: Box::leak(pool),
            symbols: Box::leak(symbols),
            scope: crate::ScopeId(0),
            resolved: Box::leak(resolved),
        }
    }
}
