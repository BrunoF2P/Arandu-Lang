use super::ar_type::ArType;
use super::type_interner::{TypeId, TypeInterner};
use crate::SymbolId;

/// Map from type-parameter `SymbolId` to concrete type.
pub type GenericSubst = smallvec::SmallVec<[(SymbolId, ArType); 2]>;

#[must_use]
pub fn build_subst(param_symbols: &[SymbolId], args: &[ArType]) -> GenericSubst {
    let mut subst = GenericSubst::new();
    for (param, arg) in param_symbols.iter().zip(args.iter()) {
        subst.push((*param, arg.clone()));
    }
    subst
}

#[must_use]
pub fn substitute_type(ty: &ArType, subst: &GenericSubst, interner: &mut TypeInterner) -> ArType {
    match ty {
        ArType::Named(id, args) => {
            if let Some((_, concrete)) = subst.iter().find(|(param, _)| param == id) {
                return concrete.clone();
            }
            let new_args: Vec<TypeId> = args
                .iter()
                .map(|&a| {
                    let resolved = interner.resolve(a).clone();
                    let substituted = substitute_type(&resolved, subst, interner);
                    interner.intern(substituted)
                })
                .collect();
            ArType::Named(*id, new_args)
        }
        ArType::Nullable(inner) => {
            let resolved = interner.resolve(*inner).clone();
            let substituted = substitute_type(&resolved, subst, interner);
            let id = interner.intern(substituted);
            ArType::Nullable(id)
        }
        ArType::Option(inner) => {
            let resolved = interner.resolve(*inner).clone();
            let substituted = substitute_type(&resolved, subst, interner);
            let id = interner.intern(substituted);
            ArType::Option(id)
        }
        ArType::Range(inner) => {
            let resolved = interner.resolve(*inner).clone();
            let substituted = substitute_type(&resolved, subst, interner);
            let id = interner.intern(substituted);
            ArType::Range(id)
        }
        ArType::Result(ok, err) => {
            let resolved_ok = interner.resolve(*ok).clone();
            let resolved_err = interner.resolve(*err).clone();
            let subst_ok = substitute_type(&resolved_ok, subst, interner);
            let ok_id = interner.intern(subst_ok);
            let subst_err = substitute_type(&resolved_err, subst, interner);
            let err_id = interner.intern(subst_err);
            ArType::Result(ok_id, err_id)
        }
        ArType::Ptr(inner) => {
            let resolved = interner.resolve(*inner).clone();
            let substituted = substitute_type(&resolved, subst, interner);
            let id = interner.intern(substituted);
            ArType::Ptr(id)
        }
        ArType::Slice(inner) => {
            let resolved = interner.resolve(*inner).clone();
            let substituted = substitute_type(&resolved, subst, interner);
            let id = interner.intern(substituted);
            ArType::Slice(id)
        }
        ArType::Array(n, inner) => {
            let resolved = interner.resolve(*inner).clone();
            let substituted = substitute_type(&resolved, subst, interner);
            let id = interner.intern(substituted);
            ArType::Array(*n, id)
        }
        ArType::Tuple(items) => {
            let new_items: Vec<TypeId> = items
                .iter()
                .map(|&t| {
                    let resolved = interner.resolve(t).clone();
                    let substituted = substitute_type(&resolved, subst, interner);
                    interner.intern(substituted)
                })
                .collect();
            ArType::Tuple(new_items)
        }
        ArType::Func(params, ret) => {
            let new_params: Vec<TypeId> = params
                .iter()
                .map(|&p| {
                    let resolved = interner.resolve(p).clone();
                    let substituted = substitute_type(&resolved, subst, interner);
                    interner.intern(substituted)
                })
                .collect();
            let resolved_ret = interner.resolve(*ret).clone();
            let subst_ret = substitute_type(&resolved_ret, subst, interner);
            let ret_id = interner.intern(subst_ret);
            ArType::Func(new_params, ret_id)
        }
        _ => ty.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_interner() -> TypeInterner {
        TypeInterner::new()
    }

    // ── build_subst ──

    #[test]
    fn build_subst_empty() {
        let subst = build_subst(&[], &[]);
        assert!(subst.is_empty());
    }

    #[test]
    fn build_subst_single() {
        let subst = build_subst(
            &[SymbolId(1)],
            &[ArType::Primitive(super::super::Primitive::Int)],
        );
        assert_eq!(subst.len(), 1);
        assert_eq!(subst[0].0, SymbolId(1));
        assert_eq!(subst[0].1, ArType::Primitive(super::super::Primitive::Int));
    }

    #[test]
    fn build_subst_multiple() {
        let subst = build_subst(
            &[SymbolId(1), SymbolId(2)],
            &[
                ArType::Primitive(super::super::Primitive::Int),
                ArType::Primitive(super::super::Primitive::Bool),
            ],
        );
        assert_eq!(subst.len(), 2);
    }

    #[test]
    fn build_subst_ignores_extra_params() {
        let subst = build_subst(
            &[SymbolId(1), SymbolId(2)],
            &[ArType::Primitive(super::super::Primitive::Int)],
        );
        assert_eq!(subst.len(), 1);
    }

    // ── substitute_type ──

    #[test]
    fn substitute_simple_named() {
        let mut i = new_interner();
        let subst = build_subst(
            &[SymbolId(1)],
            &[ArType::Primitive(super::super::Primitive::Int)],
        );
        let ty = ArType::Named(SymbolId(1), vec![]);
        let result = substitute_type(&ty, &subst, &mut i);
        assert_eq!(result, ArType::Primitive(super::super::Primitive::Int));
    }

    #[test]
    fn substitute_non_param_named_unchanged() {
        let mut i = new_interner();
        let subst = build_subst(
            &[SymbolId(1)],
            &[ArType::Primitive(super::super::Primitive::Int)],
        );
        let ty = ArType::Named(SymbolId(2), vec![]);
        let result = substitute_type(&ty, &subst, &mut i);
        assert_eq!(result, ArType::Named(SymbolId(2), vec![]));
    }

    #[test]
    fn substitute_named_with_generic_args() {
        let mut i = new_interner();
        let inner = i.intern(ArType::Named(SymbolId(1), vec![]));
        let ty = ArType::Named(SymbolId(3), vec![inner]);
        let subst = build_subst(
            &[SymbolId(1)],
            &[ArType::Primitive(super::super::Primitive::Int)],
        );
        let result = substitute_type(&ty, &subst, &mut i);
        let expected_inner = i.intern(ArType::Primitive(super::super::Primitive::Int));
        assert_eq!(result, ArType::Named(SymbolId(3), vec![expected_inner]));
    }

    #[test]
    fn substitute_nullable() {
        let mut i = new_interner();
        let inner = i.intern(ArType::Named(SymbolId(1), vec![]));
        let ty = ArType::Nullable(inner);
        let subst = build_subst(
            &[SymbolId(1)],
            &[ArType::Primitive(super::super::Primitive::Int)],
        );
        let result = substitute_type(&ty, &subst, &mut i);
        let expected_inner = i.intern(ArType::Primitive(super::super::Primitive::Int));
        assert_eq!(result, ArType::Nullable(expected_inner));
    }

    #[test]
    fn substitute_option() {
        let mut i = new_interner();
        let inner = i.intern(ArType::Named(SymbolId(1), vec![]));
        let ty = ArType::Option(inner);
        let subst = build_subst(
            &[SymbolId(1)],
            &[ArType::Primitive(super::super::Primitive::Int)],
        );
        let result = substitute_type(&ty, &subst, &mut i);
        let expected_inner = i.intern(ArType::Primitive(super::super::Primitive::Int));
        assert_eq!(result, ArType::Option(expected_inner));
    }

    #[test]
    fn substitute_result() {
        let mut i = new_interner();
        let ok_inner = i.intern(ArType::Named(SymbolId(1), vec![]));
        let err_inner = i.intern(ArType::Named(SymbolId(2), vec![]));
        let ty = ArType::Result(ok_inner, err_inner);
        let subst = build_subst(
            &[SymbolId(1), SymbolId(2)],
            &[
                ArType::Primitive(super::super::Primitive::Int),
                ArType::Primitive(super::super::Primitive::Str),
            ],
        );
        let result = substitute_type(&ty, &subst, &mut i);
        let expected_ok = i.intern(ArType::Primitive(super::super::Primitive::Int));
        let expected_err = i.intern(ArType::Primitive(super::super::Primitive::Str));
        assert_eq!(result, ArType::Result(expected_ok, expected_err));
    }

    #[test]
    fn substitute_slice() {
        let mut i = new_interner();
        let inner = i.intern(ArType::Named(SymbolId(1), vec![]));
        let ty = ArType::Slice(inner);
        let subst = build_subst(
            &[SymbolId(1)],
            &[ArType::Primitive(super::super::Primitive::Int)],
        );
        let result = substitute_type(&ty, &subst, &mut i);
        let expected_inner = i.intern(ArType::Primitive(super::super::Primitive::Int));
        assert_eq!(result, ArType::Slice(expected_inner));
    }

    #[test]
    fn substitute_array() {
        let mut i = new_interner();
        let inner = i.intern(ArType::Named(SymbolId(1), vec![]));
        let ty = ArType::Array(4, inner);
        let subst = build_subst(
            &[SymbolId(1)],
            &[ArType::Primitive(super::super::Primitive::Int)],
        );
        let result = substitute_type(&ty, &subst, &mut i);
        let expected_inner = i.intern(ArType::Primitive(super::super::Primitive::Int));
        assert_eq!(result, ArType::Array(4, expected_inner));
    }

    #[test]
    fn substitute_ptr() {
        let mut i = new_interner();
        let inner = i.intern(ArType::Named(SymbolId(1), vec![]));
        let ty = ArType::Ptr(inner);
        let subst = build_subst(
            &[SymbolId(1)],
            &[ArType::Primitive(super::super::Primitive::Int)],
        );
        let result = substitute_type(&ty, &subst, &mut i);
        let expected_inner = i.intern(ArType::Primitive(super::super::Primitive::Int));
        assert_eq!(result, ArType::Ptr(expected_inner));
    }

    #[test]
    fn substitute_tuple() {
        let mut i = new_interner();
        let a = i.intern(ArType::Named(SymbolId(1), vec![]));
        let b = i.intern(ArType::Named(SymbolId(2), vec![]));
        let ty = ArType::Tuple(vec![a, b]);
        let subst = build_subst(
            &[SymbolId(1), SymbolId(2)],
            &[
                ArType::Primitive(super::super::Primitive::Int),
                ArType::Primitive(super::super::Primitive::Bool),
            ],
        );
        let result = substitute_type(&ty, &subst, &mut i);
        let expected_a = i.intern(ArType::Primitive(super::super::Primitive::Int));
        let expected_b = i.intern(ArType::Primitive(super::super::Primitive::Bool));
        assert_eq!(result, ArType::Tuple(vec![expected_a, expected_b]));
    }

    #[test]
    fn substitute_func() {
        let mut i = new_interner();
        let param = i.intern(ArType::Named(SymbolId(1), vec![]));
        let ret = i.intern(ArType::Named(SymbolId(2), vec![]));
        let ty = ArType::Func(vec![param], ret);
        let subst = build_subst(
            &[SymbolId(1), SymbolId(2)],
            &[
                ArType::Primitive(super::super::Primitive::Int),
                ArType::Primitive(super::super::Primitive::Bool),
            ],
        );
        let result = substitute_type(&ty, &subst, &mut i);
        let expected_param = i.intern(ArType::Primitive(super::super::Primitive::Int));
        let expected_ret = i.intern(ArType::Primitive(super::super::Primitive::Bool));
        assert_eq!(result, ArType::Func(vec![expected_param], expected_ret));
    }

    #[test]
    fn substitute_primitive_unchanged() {
        let mut i = new_interner();
        let subst = build_subst(
            &[SymbolId(1)],
            &[ArType::Primitive(super::super::Primitive::Int)],
        );
        assert_eq!(
            substitute_type(
                &ArType::Primitive(super::super::Primitive::Bool),
                &subst,
                &mut i
            ),
            ArType::Primitive(super::super::Primitive::Bool)
        );
    }

    #[test]
    fn substitute_range() {
        let mut i = new_interner();
        let inner = i.intern(ArType::Named(SymbolId(1), vec![]));
        let ty = ArType::Range(inner);
        let subst = build_subst(
            &[SymbolId(1)],
            &[ArType::Primitive(super::super::Primitive::Int)],
        );
        let result = substitute_type(&ty, &subst, &mut i);
        let expected_inner = i.intern(ArType::Primitive(super::super::Primitive::Int));
        assert_eq!(result, ArType::Range(expected_inner));
    }

    #[test]
    fn substitute_void_err_literals_unchanged() {
        let mut i = new_interner();
        let subst = build_subst(
            &[SymbolId(1)],
            &[ArType::Primitive(super::super::Primitive::Int)],
        );
        assert_eq!(substitute_type(&ArType::Void, &subst, &mut i), ArType::Void);
        assert_eq!(substitute_type(&ArType::Err, &subst, &mut i), ArType::Err);
        assert_eq!(
            substitute_type(&ArType::IntLiteral, &subst, &mut i),
            ArType::IntLiteral
        );
        assert_eq!(
            substitute_type(&ArType::Error, &subst, &mut i),
            ArType::Error
        );
    }
}
