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
