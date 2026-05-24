use std::collections::HashMap;

use arandu_parser::{Expr, TypeExpr};

use super::ar_type::ArType;
use super::lower::lower_type_expr;
use super::result_option::type_name_base;
use super::subst::{GenericSubst, build_subst, substitute_type};
use crate::SymbolId;
use crate::passes::type_checker::TypeChecker;

pub(crate) fn collect_generic_param_symbols(
    checker: &TypeChecker,
    generic_params: &[arandu_parser::GenericParam],
) -> Vec<SymbolId> {
    generic_params
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
pub(crate) fn instantiate_type(ty: &ArType, subst: &GenericSubst) -> ArType {
    substitute_type(ty, subst)
}

#[must_use]
pub(crate) fn struct_fields_instantiated(
    checker: &mut TypeChecker,
    struct_id: SymbolId,
    generic_args: &[ArType],
) -> Option<HashMap<String, ArType>> {
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
    Some(
        fields
            .into_iter()
            .map(|(name, ty)| (name, instantiate_type(&ty, &subst)))
            .collect(),
    )
}

/// Instantiate a generic callee (`identity<int>`, `Result.Ok<int>`, …) to its value type.
pub(crate) fn synth_generic_instantiation(
    checker: &mut TypeChecker,
    callee: &Expr,
    type_args: &[TypeExpr],
    span: arandu_lexer::Span,
) -> ArType {
    let arg_tys: Vec<ArType> = type_args
        .iter()
        .map(|a| lower_type_expr(a, &checker.symbols, checker.type_scope(), &checker.resolved))
        .collect();

    if let Expr::TypePath {
        type_name,
        member,
        span: tp_span,
    } = callee
    {
        let base = type_name_base(type_name);
        if base == "Result" {
            if arg_tys.len() != 1 {
                let diag = crate::Diagnostic::error(
                    crate::DiagCode::T012WrongArgCount,
                    format!(
                        "Result.{member} expects 1 type argument, found {}",
                        arg_tys.len()
                    ),
                    span,
                )
                .with_label(callee.span(), "generic callee is here")
                .with_label(span, format!("{} type arguments provided", arg_tys.len()));
                checker.diagnostics.push(diag);
                return ArType::Error;
            }
            let inner = arg_tys[0].clone();
            return match member.as_str() {
                "Ok" => ArType::Func(
                    vec![inner.clone()],
                    Box::new(ArType::Result(Box::new(inner), Box::new(ArType::Err))),
                ),
                "Err" => ArType::Func(
                    vec![ArType::Err],
                    Box::new(ArType::Result(
                        Box::new(ArType::Void),
                        Box::new(ArType::Err),
                    )),
                ),
                _ => {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T018UndefinedField,
                        format!("unknown Result member '{member}'"),
                        *tp_span,
                    ));
                    ArType::Error
                }
            };
        }
        if base == "Option" && member == "Some" {
            if arg_tys.len() != 1 {
                let diag = crate::Diagnostic::error(
                    crate::DiagCode::T012WrongArgCount,
                    format!(
                        "Option.Some expects 1 type argument, found {}",
                        arg_tys.len()
                    ),
                    span,
                )
                .with_label(callee.span(), "generic callee is here")
                .with_label(span, format!("{} type arguments provided", arg_tys.len()));
                checker.diagnostics.push(diag);
                return ArType::Error;
            }
            let inner = arg_tys[0].clone();
            return ArType::Func(
                vec![inner.clone()],
                Box::new(ArType::Option(Box::new(inner))),
            );
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
        .with_label(callee.span(), "generic callee is here")
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
    instantiate_type(&template, &subst)
}

fn resolve_generic_callee_symbol(checker: &TypeChecker, callee: &Expr) -> Option<SymbolId> {
    match callee {
        Expr::Path { span, .. } => checker
            .resolved
            .value_refs
            .get(&crate::NodeKey::from(*span))
            .copied(),
        Expr::TypePath { span, .. } => checker
            .resolved
            .value_refs
            .get(&crate::NodeKey::from(*span))
            .copied()
            .or_else(|| {
                checker
                    .resolved
                    .type_refs
                    .get(&crate::NodeKey::from(*span))
                    .copied()
            }),
        Expr::Field { base, field, span } => {
            let base_ty = checker.expr_type(crate::NodeKey::from(base.span()))?;
            let ArType::Named(struct_id, _) = base_ty else {
                return None;
            };
            let struct_name = checker.symbols.get(struct_id).name.clone();
            checker
                .symbols
                .lookup_associated_member(&struct_name, field)
                .or_else(|| {
                    checker
                        .resolved
                        .value_refs
                        .get(&crate::NodeKey::from(*span))
                        .copied()
                })
        }
        _ => None,
    }
}
