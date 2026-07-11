use crate::type_checker::TypeChecker;
use crate::type_checker::types::{self, ArType};

use arandu_middle::types::type_interner::TypeId;

pub(crate) fn infer_and_instantiate_func(
    checker: &mut TypeChecker<'_>,
    type_params: &[arandu_middle::SymbolId],
    formals: &[TypeId],
    ret: TypeId,
    arg_tys: &[TypeId],
    expected_ret: Option<TypeId>,
) -> Option<(Vec<TypeId>, TypeId)> {
    if formals.len() != arg_tys.len() {
        return None;
    }
    let mut bindings: rustc_hash::FxHashMap<arandu_middle::SymbolId, TypeId> =
        rustc_hash::FxHashMap::default();
    for (&formal_id, &arg_id) in formals.iter().zip(arg_tys.iter()) {
        let formal = checker.resolve(formal_id);
        bind_type_params(checker, type_params, &formal, arg_id, &mut bindings);
    }
    // `join<T>(handle)` has no `T` in args — infer from expected return type.
    if let Some(exp) = expected_ret {
        let ret_ty = checker.resolve(ret);
        bind_type_params(checker, type_params, &ret_ty, exp, &mut bindings);
    }
    let mut concrete = Vec::with_capacity(type_params.len());
    for &p in type_params {
        let tid = bindings.get(&p).copied()?;
        if checker.resolve(tid).is_error() {
            return None;
        }
        concrete.push(checker.resolve(tid));
    }
    let subst = types::build_subst(type_params, &concrete);
    let new_params: Vec<TypeId> = formals
        .iter()
        .map(|&fid| {
            let ty = checker.resolve(fid);
            let inst = types::substitute_type(&ty, &subst, &checker.type_info.type_interner);
            checker.intern(inst)
        })
        .collect();
    let ret_ty = checker.resolve(ret);
    let ret_inst = types::substitute_type(&ret_ty, &subst, &checker.type_info.type_interner);
    Some((new_params, checker.intern(ret_inst)))
}

pub(crate) fn bind_type_params(
    checker: &TypeChecker<'_>,
    type_params: &[arandu_middle::SymbolId],
    formal: &ArType,
    actual_id: TypeId,
    bindings: &mut rustc_hash::FxHashMap<arandu_middle::SymbolId, TypeId>,
) {
    let interner = &checker.type_info.type_interner;
    match formal {
        ArType::Named(id, args) if args.is_empty() && type_params.contains(id) => {
            bindings.entry(*id).or_insert(actual_id);
        }
        ArType::Named(_, args) => {
            if let ArType::Named(_, act_args) = interner.resolve(actual_id)
                && args.len() == act_args.len()
            {
                for (&fa, &aa) in args.iter().zip(act_args.iter()) {
                    bind_type_params(checker, type_params, &interner.resolve(fa), aa, bindings);
                }
            }
        }
        ArType::Ptr(inner)
        | ArType::Nullable(inner)
        | ArType::Slice(inner)
        | ArType::Option(inner)
        | ArType::Array(_, inner)
        | ArType::Ref(inner)
        | ArType::RefMut(inner)
        | ArType::Coroutine(inner)
        | ArType::Poll(inner) => {
            let act_inner = match interner.resolve(actual_id) {
                ArType::Ptr(i)
                | ArType::Nullable(i)
                | ArType::Slice(i)
                | ArType::Option(i)
                | ArType::Array(_, i)
                | ArType::Ref(i)
                | ArType::RefMut(i)
                | ArType::Coroutine(i)
                | ArType::Poll(i) => Some(i),
                _ => None,
            };
            if let Some(ai) = act_inner {
                bind_type_params(
                    checker,
                    type_params,
                    &interner.resolve(*inner),
                    ai,
                    bindings,
                );
            }
        }
        ArType::Result(ok, err) => {
            if let ArType::Result(aok, aerr) = interner.resolve(actual_id) {
                bind_type_params(checker, type_params, &interner.resolve(*ok), aok, bindings);
                bind_type_params(
                    checker,
                    type_params,
                    &interner.resolve(*err),
                    aerr,
                    bindings,
                );
            }
        }
        _ => {}
    }
}
