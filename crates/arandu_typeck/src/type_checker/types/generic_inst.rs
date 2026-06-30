use rustc_hash::FxHashMap;

use arandu_middle::SymbolId;
use arandu_parser::{GenericParam, IndexRange};
use arandu_parser::ast_pool::{ExprId, ExprKind};

use crate::type_checker::TypeChecker;
use crate::type_checker::types::{
    ArType, GenericSubst, TypeInterner, build_subst, lower_type_expr,
    substitute_type, type_name_base,
};

#[must_use]
pub fn extract_generic_param_symbols(
    checker: &TypeChecker<'_>,
    params: &[GenericParam],
) -> Vec<SymbolId> {
    params
        .iter()
        .filter_map(|param| {
            checker
                .resolved
                .definitions
                .get(&crate::NodeKey::from(param.span))
                .copied()
        })
        .collect()
}

#[must_use]
pub(crate) fn instantiate_type(
    ty: &ArType,
    subst: &GenericSubst,
    interner: &mut TypeInterner,
) -> ArType {
    substitute_type(ty, subst, interner)
}

#[must_use]
pub fn struct_fields_instantiated(
    checker: &mut TypeChecker<'_>,
    struct_id: SymbolId,
    generic_args: &[ArType],
) -> Option<FxHashMap<String, ArType>> {
    let fields = checker.type_info.struct_fields.get(&struct_id)?.clone();
    let params = checker.type_info.generic_params.get(&struct_id)?.clone();
    if params.len() != generic_args.len() {
        return None;
    }
    let span = checker.symbols.get(struct_id).span;
    super::interfaces::check_instantiation_constraints(
        checker,
        struct_id,
        &params,
        generic_args,
        span,
    );
    let subst = build_subst(&params, generic_args);
    let res: FxHashMap<String, ArType> = fields
        .into_iter()
        .map(|(name, ty)| {
            let inst = instantiate_type(&ty, &subst, &mut checker.type_info.type_interner);
            (name, inst)
        })
        .collect();
    Some(res)
}

/// Instantiate a generic callee (`identity<int>`, `Result.Ok<int>`, …) to its value type.
pub fn synth_generic_instantiation(
    checker: &mut TypeChecker<'_>,
    callee: ExprId,
    type_args: IndexRange,
    span: arandu_lexer::Span,
) -> ArType {
    let arg_ids = checker.pool.type_expr_list(type_args).to_vec();
    let scope = checker.type_scope();
    let arg_tys: Vec<ArType> = arg_ids
        .iter()
        .map(|a| {
            lower_type_expr(
                *a,
                checker.pool,
                &checker.symbols,
                scope,
                &checker.resolved,
                &mut checker.type_info.type_interner,
            )
        })
        .collect();

    if let ExprKind::TypePath { type_name, member } = checker.pool.expr(callee) {
        let base_name = type_name_base(&type_name);
        if base_name == "Result" {
            if arg_tys.len() != 1 {
                let diag = crate::Diagnostic::error(
                    crate::DiagCode::T012WrongArgCount,
                    format!(
                        "Result.{member} expects 1 type argument, found {}",
                        arg_tys.len()
                    ),
                    span,
                )
                .with_label(checker.pool.expr_span(callee), "generic callee is here")
                .with_label(span, format!("{} type arguments provided", arg_tys.len()));
                checker.diagnostics.push(diag);
                return ArType::Error;
            }
            let inner = arg_tys[0].clone();
            return match member.as_str() {
                "Ok" => {
                    let inner_id = checker.intern(inner);
                    let err_id = checker.intern(ArType::Err);
                    let result_id = checker.intern(ArType::Result(inner_id, err_id));
                    ArType::Func(vec![inner_id], result_id)
                }
                "Err" => {
                    let err_id = checker.intern(ArType::Err);
                    let void_id = checker.intern(ArType::Void);
                    let result_id = checker.intern(ArType::Result(void_id, err_id));
                    ArType::Func(vec![err_id], result_id)
                }
                _ => {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T018UndefinedField,
                        format!("unknown Result member '{member}'"),
                        checker.pool.expr_span(callee),
                    ));
                    ArType::Error
                }
            };
        }
        if base_name == "Option" && member == "Some" {
            if arg_tys.len() != 1 {
                let diag = crate::Diagnostic::error(
                    crate::DiagCode::T012WrongArgCount,
                    format!(
                        "Option.Some expects 1 type argument, found {}",
                        arg_tys.len()
                    ),
                    span,
                )
                .with_label(checker.pool.expr_span(callee), "generic callee is here")
                .with_label(span, format!("{} type arguments provided", arg_tys.len()));
                checker.diagnostics.push(diag);
                return ArType::Error;
            }
            let inner = arg_tys[0].clone();
            let inner_id = checker.intern(inner);
            let opt_id = checker.intern(ArType::Option(inner_id));
            return ArType::Func(vec![inner_id], opt_id);
        }
    }

    let Some(callee_symbol) = resolve_generic_callee_symbol(checker, callee) else {
        checker.diagnostics.push(crate::Diagnostic::error(
            crate::DiagCode::N001UndefinedValue,
            "cannot resolve generic callee".to_string(),
            span,
        ));
        return ArType::Error;
    };

    let Some(param_symbols) = checker
        .type_info
        .generic_params
        .get(&callee_symbol)
        .cloned()
    else {
        checker.diagnostics.push(crate::Diagnostic::error(
            crate::DiagCode::T011GenericConstraintNotSatisfied,
            "callee is not generic".to_string(),
            span,
        ));
        return ArType::Error;
    };

    if param_symbols.len() != arg_tys.len() {
        let diag = crate::Diagnostic::error(
            crate::DiagCode::T012WrongArgCount,
            format!(
                "generic callee expects {} type argument(s), found {}",
                param_symbols.len(),
                arg_tys.len()
            ),
            span,
        )
        .with_label(checker.pool.expr_span(callee), "generic callee is here")
        .with_label(span, format!("{} type arguments provided", arg_tys.len()));
        checker.diagnostics.push(diag);
        return ArType::Error;
    }

    let Some(template) = checker.decl_type(callee_symbol) else {
        return ArType::Error;
    };

    let subst = build_subst(&param_symbols, &arg_tys);
    super::interfaces::check_instantiation_constraints(
        checker,
        callee_symbol,
        &param_symbols,
        &arg_tys,
        span,
    );
    instantiate_type(&template, &subst, &mut checker.type_info.type_interner)
}

fn resolve_generic_callee_symbol(checker: &mut TypeChecker<'_>, callee: ExprId) -> Option<SymbolId> {
    match checker.pool.expr(callee) {
        ExprKind::Path { .. } => checker.resolved.expr_symbol(callee),
        ExprKind::TypePath { .. } => checker.resolved.expr_symbol(callee).or_else(|| {
            checker
                .resolved
                .type_refs
                .get(&crate::NodeKey::from(checker.pool.expr_span(callee)))
                .copied()
        }),
        ExprKind::Field { base, field } => {
            let base_ty = checker.expr_type(*base).unwrap_or_else(|| {
                crate::passes::type_checker::synth::synth_expr(checker, *base)
            });
            let actual_base_ty = match &base_ty {
                ArType::Nullable(inner) => checker.resolve(*inner).clone(),
                other => other.clone(),
            };
            let struct_id = match &actual_base_ty {
                ArType::Named(id, _) => Some(*id),
                ArType::Ptr(inner) => match checker.resolve(*inner) {
                    ArType::Named(id, _) => Some(*id),
                    _ => None,
                },
                _ => None,
            };
            if let Some(struct_id) = struct_id {
                let struct_name = checker.symbols.get(struct_id).name.clone();
                if let Some(sym) = checker
                    .symbols
                    .lookup_associated_member(&struct_name, field)
                {
                    return Some(sym);
                }
            }
            checker.resolved.expr_symbol(callee)
        }
        _ => None,
    }
}
