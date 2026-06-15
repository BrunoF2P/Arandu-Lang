use rustc_hash::FxHashMap;

use super::ar_type::ArType;
use crate::SymbolId;

/// Map from type-parameter `SymbolId` to concrete type.
pub type GenericSubst = FxHashMap<SymbolId, ArType>;

#[must_use]
pub fn build_subst(param_symbols: &[SymbolId], args: &[ArType]) -> GenericSubst {
    param_symbols
        .iter()
        .zip(args.iter())
        .map(|(param, arg)| (*param, arg.clone()))
        .collect()
}

#[must_use]
pub fn substitute_type(ty: &ArType, subst: &GenericSubst) -> ArType {
    match ty {
        ArType::Named(id, args) => {
            if let Some(concrete) = subst.get(id) {
                return concrete.clone();
            }
            let new_args: Vec<ArType> = args.iter().map(|a| substitute_type(a, subst)).collect();
            ArType::Named(*id, new_args)
        }
        ArType::Nullable(inner) => ArType::Nullable(Box::new(substitute_type(inner, subst))),
        ArType::Option(inner) => ArType::Option(Box::new(substitute_type(inner, subst))),
        ArType::Result(ok, err) => ArType::Result(
            Box::new(substitute_type(ok, subst)),
            Box::new(substitute_type(err, subst)),
        ),
        ArType::Ptr(inner) => ArType::Ptr(Box::new(substitute_type(inner, subst))),
        ArType::Slice(inner) => ArType::Slice(Box::new(substitute_type(inner, subst))),
        ArType::Array(n, inner) => ArType::Array(*n, Box::new(substitute_type(inner, subst))),
        ArType::Tuple(items) => {
            ArType::Tuple(items.iter().map(|t| substitute_type(t, subst)).collect())
        }
        ArType::Func(params, ret) => {
            let new_params: Vec<ArType> =
                params.iter().map(|p| substitute_type(p, subst)).collect();
            ArType::Func(new_params, Box::new(substitute_type(ret, subst)))
        }
        _ => ty.clone(),
    }
}
