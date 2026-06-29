use super::ar_type::ArType;
use super::type_interner::TypeId;
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
pub fn substitute_type(ty: &ArType, subst: &GenericSubst) -> ArType {
    match ty {
        ArType::Named(id, args) => {
            if let Some((_, concrete)) = subst.iter().find(|(param, _)| param == id) {
                return concrete.clone();
            }
            let new_args: Vec<TypeId> = args
                .iter()
                .map(|&a| {
                    let resolved = super::type_interner::with_resolved_type(a, |t| t.clone());
                    let substituted = substitute_type(&resolved, subst);
                    super::type_interner::intern_type(substituted)
                })
                .collect();
            ArType::Named(*id, new_args)
        }
        ArType::Nullable(inner) => {
            let resolved = super::type_interner::with_resolved_type(*inner, |t| t.clone());
            let substituted = substitute_type(&resolved, subst);
            let id = super::type_interner::intern_type(substituted);
            ArType::Nullable(id)
        }
        ArType::Option(inner) => {
            let resolved = super::type_interner::with_resolved_type(*inner, |t| t.clone());
            let substituted = substitute_type(&resolved, subst);
            let id = super::type_interner::intern_type(substituted);
            ArType::Option(id)
        }
        ArType::Range(inner) => {
            let resolved = super::type_interner::with_resolved_type(*inner, |t| t.clone());
            let substituted = substitute_type(&resolved, subst);
            let id = super::type_interner::intern_type(substituted);
            ArType::Range(id)
        }
        ArType::Result(ok, err) => {
            let resolved_ok = super::type_interner::with_resolved_type(*ok, |t| t.clone());
            let resolved_err = super::type_interner::with_resolved_type(*err, |t| t.clone());
            let ok_id = super::type_interner::intern_type(substitute_type(&resolved_ok, subst));
            let err_id = super::type_interner::intern_type(substitute_type(&resolved_err, subst));
            ArType::Result(ok_id, err_id)
        }
        ArType::Ptr(inner) => {
            let resolved = super::type_interner::with_resolved_type(*inner, |t| t.clone());
            let substituted = substitute_type(&resolved, subst);
            let id = super::type_interner::intern_type(substituted);
            ArType::Ptr(id)
        }
        ArType::Slice(inner) => {
            let resolved = super::type_interner::with_resolved_type(*inner, |t| t.clone());
            let substituted = substitute_type(&resolved, subst);
            let id = super::type_interner::intern_type(substituted);
            ArType::Slice(id)
        }
        ArType::Array(n, inner) => {
            let resolved = super::type_interner::with_resolved_type(*inner, |t| t.clone());
            let substituted = substitute_type(&resolved, subst);
            let id = super::type_interner::intern_type(substituted);
            ArType::Array(*n, id)
        }
        ArType::Tuple(items) => {
            let new_items: Vec<TypeId> = items
                .iter()
                .map(|&t| {
                    let resolved = super::type_interner::with_resolved_type(t, |x| x.clone());
                    let substituted = substitute_type(&resolved, subst);
                    super::type_interner::intern_type(substituted)
                })
                .collect();
            ArType::Tuple(new_items)
        }
        ArType::Func(params, ret) => {
            let new_params: Vec<TypeId> = params
                .iter()
                .map(|&p| {
                    let resolved = super::type_interner::with_resolved_type(p, |x| x.clone());
                    let substituted = substitute_type(&resolved, subst);
                    super::type_interner::intern_type(substituted)
                })
                .collect();
            let resolved_ret = super::type_interner::with_resolved_type(*ret, |t| t.clone());
            let ret_id = super::type_interner::intern_type(substitute_type(&resolved_ret, subst));
            ArType::Func(new_params, ret_id)
        }
        _ => ty.clone(),
    }
}
