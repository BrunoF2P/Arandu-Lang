use arandu_lexer::Span;
use arandu_parser::TypeName;
use arandu_parser::ast_pool::{AstPool, ExprId, ExprKind, IndexRange};

use super::super::TypeChecker;
use super::super::constraints::ConstraintOrigin;
use super::super::types::{ArType, TypeId};
use super::expr::synth_expr;

fn type_path_member(pool: &AstPool, callee: ExprId) -> Option<(&TypeName, &str)> {
    match pool.expr(callee) {
        ExprKind::TypePath { type_name, member } => Some((type_name, member.as_str())),
        _ => None,
    }
}

pub(crate) fn synth_result_ctor(
    checker: &mut TypeChecker<'_>,
    callee: ExprId,
    args: IndexRange,
    span: Span,
) -> Option<ArType> {
    let (type_name, member) = type_path_member(checker.pool, callee)?;
    if super::super::types::type_name_base(type_name) != "Result" {
        return None;
    }
    let arg_ids = checker.pool.expr_list(args).to_vec();
    match member {
        "Ok" => {
            if arg_ids.len() != 1 {
                let diag = crate::Diagnostic::error(
                    crate::DiagCode::T012WrongArgCount,
                    format!("Result.Ok expects 1 argument, found {}", arg_ids.len()),
                    span,
                )
                .with_label(checker.pool.expr_span(callee), "call target is here")
                .with_label(span, format!("{} arguments provided", arg_ids.len()));
                checker.diagnostics.push(diag);
                return Some(ArType::Error);
            }
            let ok_ty_id = synth_expr(checker, arg_ids[0]);
            let err_literal_id = checker.intern(ArType::Err);
            Some(ArType::Result(ok_ty_id, err_literal_id))
        }
        "Err" => {
            if arg_ids.len() != 1 {
                let diag = crate::Diagnostic::error(
                    crate::DiagCode::T012WrongArgCount,
                    format!("Result.Err expects 1 argument, found {}", arg_ids.len()),
                    span,
                )
                .with_label(checker.pool.expr_span(callee), "call target is here")
                .with_label(span, format!("{} arguments provided", arg_ids.len()));
                checker.diagnostics.push(diag);
                return Some(ArType::Error);
            }
            let err_ty_id = synth_expr(checker, arg_ids[0]);
            let err_id = checker.intern(ArType::Error);
            Some(ArType::Result(err_id, err_ty_id))
        }
        _ => None,
    }
}

pub(crate) fn synth_option_ctor(
    checker: &mut TypeChecker<'_>,
    callee: ExprId,
    args: IndexRange,
    span: Span,
) -> Option<ArType> {
    let (type_name, member) = type_path_member(checker.pool, callee)?;
    if super::super::types::type_name_base(type_name) != "Option" {
        return None;
    }
    let arg_ids = checker.pool.expr_list(args).to_vec();
    match member {
        "Some" => {
            if arg_ids.len() != 1 {
                let diag = crate::Diagnostic::error(
                    crate::DiagCode::T012WrongArgCount,
                    format!("Option.Some expects 1 argument, found {}", arg_ids.len()),
                    span,
                )
                .with_label(checker.pool.expr_span(callee), "call target is here")
                .with_label(span, format!("{} arguments provided", arg_ids.len()));
                checker.diagnostics.push(diag);
                return Some(ArType::Error);
            }
            let inner_id = synth_expr(checker, arg_ids[0]);
            Some(ArType::Option(inner_id))
        }
        _ => None,
    }
}

#[tracing::instrument(level = "trace", target = "arandu_typeck", skip(checker))]
pub(crate) fn synth_method_call(
    checker: &mut TypeChecker<'_>,
    base: ExprId,
    callee: ExprId,
    method: &str,
    field_span: arandu_lexer::Span,
    args: IndexRange,
    call_span: arandu_lexer::Span,
) -> Option<TypeId> {
    let base_ty_id = synth_expr(checker, base);
    if checker.resolve(base_ty_id).is_error() {
        return Some(checker.intern(ArType::Error));
    }

    let actual_base_ty_id = match checker.resolve(base_ty_id) {
        ArType::Nullable(inner) => *inner,
        _ => base_ty_id,
    };

    let struct_id = match checker.resolve(actual_base_ty_id) {
        ArType::Named(id, _) => Some(*id),
        ArType::Ptr(inner) => match checker.resolve(*inner) {
            ArType::Named(id, _) => Some(*id),
            _ => None,
        },
        _ => None,
    }?;

    let struct_name = checker.symbols.get(struct_id).name.clone();
    let method_sym = checker
        .symbols
        .lookup_associated_member(&struct_name, method);

    let mut resolved_method = None;
    if method_sym.is_none()
        && let Some(constraints) = checker.type_info.param_constraints.get(&struct_id)
    {
        for &iface_sym in constraints {
            if let Some(iface_info) = checker.type_info.interfaces.get(&iface_sym)
                && let Some((_, method_sig)) = iface_info.methods.iter().find(|(m, _)| m == method)
            {
                resolved_method = Some(method_sig.clone());
                break;
            }
        }
    }

    let (params, ret, method_sym_recorded) = if let Some(method_sig) = resolved_method {
        if let ArType::Func(params, ret) = method_sig {
            // The interface method signature already has `self` as params[0]
            // (typed as the interface). Replace it with the actual base type
            // so the receiver unification works correctly and arg count matches.
            let mut new_params = vec![actual_base_ty_id];
            if params.len() > 1 {
                new_params.extend_from_slice(&params[1..]);
            }
            (new_params, ret, None)
        } else {
            return None;
        }
    } else {
        let sym = method_sym?;
        let method_ty = checker.decl_type(sym)?;
        let (params, ret) = match &method_ty {
            ArType::Func(params, ret) => (params.clone(), *ret),
            _ => return None,
        };
        (params, ret, Some(sym))
    };

    if params.is_empty() {
        return None;
    }

    let receiver_ty_id = params[0];
    if !checker.unify_ids(receiver_ty_id, actual_base_ty_id) {
        checker.add_constraint(
            receiver_ty_id,
            actual_base_ty_id,
            ConstraintOrigin::CallArg {
                call_span,
                param_span: field_span,
                arg_span: checker.pool.expr_span(base),
                arg_index: 0,
            },
        );
    }

    let explicit_params = &params[1..];
    let arg_ids = checker.pool.expr_list(args).to_vec();
    if explicit_params.len() != arg_ids.len() {
        let diag = crate::Diagnostic::error(
            crate::DiagCode::T012WrongArgCount,
            format!(
                "method '{struct_name}.{method}' expects {} argument(s), found {}",
                explicit_params.len(),
                arg_ids.len()
            ),
            call_span,
        )
        .with_label(field_span, "call target is here")
        .with_label(call_span, format!("{} arguments provided", arg_ids.len()));
        checker.diagnostics.push(diag);
    }

    for (i, arg_id) in arg_ids.iter().copied().enumerate() {
        let arg_ty_id = synth_expr(checker, arg_id);
        if let Some(&expected_id) = explicit_params.get(i)
            && !checker.unify_ids(expected_id, arg_ty_id)
        {
            checker.add_constraint(
                expected_id,
                arg_ty_id,
                ConstraintOrigin::CallArg {
                    call_span,
                    param_span: field_span,
                    arg_span: checker.pool.expr_span(arg_id),
                    arg_index: i + 1,
                },
            );
        }
    }

    if let Some(sym) = method_sym_recorded {
        checker.resolved.value_ref(field_span, sym);
    }
    let func_id = checker.intern(ArType::Func(params, ret));
    checker.record_expr_type(callee, func_id);

    Some(ret)
}
