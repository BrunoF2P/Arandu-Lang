use arandu_lexer::Span;
use arandu_parser::ast_pool::{ExprId, ExprKind};
use arandu_parser::CatchHandler;

use crate::type_checker::TypeChecker;
use crate::type_checker::constraints::ConstraintOrigin;
use crate::type_checker::synth::{
    resolve_field, resolve_index, resolve_namespace_field, resolve_namespace_member_type,
    synth_method_call, synth_option_ctor, synth_result_ctor,
};
use crate::type_checker::types::{self, ArType};
use super::synth_expr;

pub(super) fn synth_call_expr(
    checker: &mut TypeChecker<'_>,
    expr: ExprId,
    kind: &ExprKind,
    span: Span,
) -> Option<ArType> {
    match kind {
        ExprKind::Path { path: _ } => {
            if let Some(symbol_id) = checker.resolved.expr_symbol(expr) {
                if let Some(ty) = checker.ctx.lookup(symbol_id) {
                    return Some(ty.clone());
                }
                if let Some(ty) = checker.decl_type(symbol_id) {
                    return Some(ty);
                }
            }
            Some(ArType::Error)
        }
        ExprKind::TypePath { type_name, member } => {
            if types::type_name_base(type_name) == "Result" {
                return Some(match member.as_str() {
                    "Ok" => {
                        let err_id = checker.intern(ArType::Error);
                        let err_literal_id = checker.intern(ArType::Err);
                        let res_ty = ArType::Result(err_id, err_literal_id);
                        let res_id = checker.intern(res_ty);
                        ArType::Func(vec![err_id], res_id)
                    }
                    "Err" => {
                        let err_id = checker.intern(ArType::Error);
                        let err_literal_id = checker.intern(ArType::Err);
                        let res_ty = ArType::Result(err_id, err_literal_id);
                        let res_id = checker.intern(res_ty);
                        ArType::Func(vec![err_id], res_id)
                    }
                    _ => ArType::Error,
                });
            }
            if types::type_name_base(type_name) == "Option" {
                return Some(match member.as_str() {
                    "Some" => {
                        let err_id = checker.intern(ArType::Error);
                        let opt_ty = ArType::Option(err_id);
                        let opt_id = checker.intern(opt_ty);
                        ArType::Func(vec![err_id], opt_id)
                    }
                    "None" => {
                        let err_id = checker.intern(ArType::Error);
                        ArType::Option(err_id)
                    }
                    _ => ArType::Error,
                });
            }

            let type_key = crate::NodeKey::from(type_name.span);
            if let Some(enum_symbol_id) = checker.resolved.type_refs.get(&type_key) {
                let mut variant_symbol_opt = None;
                for (&var_id, &(parent_id, _)) in &checker.type_info.enum_variants {
                    if parent_id == *enum_symbol_id {
                        let var_name = &checker.symbols.get(var_id).name;
                        if var_name == member || var_name.ends_with(&format!(".{}", member)) {
                            variant_symbol_opt = Some(var_id);
                            break;
                        }
                    }
                }
                if let Some(variant_symbol_id) = variant_symbol_opt
                    && let Some((_, shape)) =
                        checker.type_info.enum_variants.get(&variant_symbol_id).cloned()
                {
                    let enum_ty = ArType::Named(*enum_symbol_id, vec![]);
                    match shape {
                        crate::type_checker::EnumPayloadShape::Unit => {
                            return Some(enum_ty);
                        }
                        crate::type_checker::EnumPayloadShape::Tuple(tys) => {
                            let param_ids = tys
                                .iter()
                                .map(|t| checker.intern(t.clone()))
                                .collect();
                            let enum_id = checker.intern(enum_ty);
                            return Some(ArType::Func(param_ids, enum_id));
                        }
                    }
                }
            }

            if let Some(symbol_id) = checker.resolved.expr_symbol(expr)
                && let Some(ty) = checker.decl_type(symbol_id)
            {
                return Some(ty);
            }
            Some(ArType::Error)
        }
        ExprKind::Generic { callee, args } => {
            let callee_id = *callee;
            let args_range = *args;
            Some(types::synth_generic_instantiation(
                checker, callee_id, args_range, span,
            ))
        }
        ExprKind::Field { base, field } => {
            let base_id = *base;
            let field_str = field.clone();
            Some(
                if let Some(ty) = resolve_namespace_field(checker, base_id, expr, &field_str, span)
                {
                    ty
                } else if let Some(ty) = resolve_namespace_member_type(checker, expr) {
                    ty
                } else {
                    resolve_field(checker, base_id, &field_str, span, false)
                },
            )
        }
        ExprKind::SafeField { base, field } => {
            let base_id = *base;
            let field_str = field.clone();
            Some(
                if let Some(ty) = resolve_namespace_field(checker, base_id, expr, &field_str, span)
                {
                    ty
                } else if let Some(ty) = resolve_namespace_member_type(checker, expr) {
                    ty
                } else {
                    resolve_field(checker, base_id, &field_str, span, true)
                },
            )
        }
        ExprKind::Index { base, index } => {
            let base_id = *base;
            let index_id = *index;
            Some(resolve_index(checker, base_id, index_id, false))
        }
        ExprKind::SafeIndex { base, index } => {
            let base_id = *base;
            let index_id = *index;
            Some(resolve_index(checker, base_id, index_id, true))
        }
        ExprKind::Try { expr: inner_expr } => {
            let inner_id = *inner_expr;
            let inner_ty = synth_expr(checker, inner_id);
            Some(
                if let Some(ok_ty) = checker.try_ok_type(&inner_ty) {
                    ok_ty
                } else if inner_ty.is_error() {
                    ArType::Error
                } else {
                    checker.add_constraint(
                        ArType::Error,
                        inner_ty,
                        ConstraintOrigin::TryInvalid { span },
                    );
                    ArType::Error
                },
            )
        }
        ExprKind::Call {
            callee,
            args,
            trailing_block: _,
        } => {
            let callee_id = *callee;
            let args_range = *args;
            if let Some(result_ty) = synth_result_ctor(checker, callee_id, args_range, span) {
                return Some(result_ty);
            }
            if let Some(option_ty) = synth_option_ctor(checker, callee_id, args_range, span) {
                return Some(option_ty);
            }
            if let ExprKind::Field { base, field } = checker.pool.expr(callee_id) {
                let base_id = *base;
                let field_str = field.clone();
                let field_span = checker.pool.expr_span(callee_id);
                if let Some(ret) = synth_method_call(
                    checker, base_id, callee_id, &field_str, field_span, args_range, span,
                ) {
                    return Some(ret);
                }
            }
            if let ExprKind::Generic {
                callee: gen_callee,
                args: gen_args,
            } = checker.pool.expr(callee_id)
            {
                let gen_callee_id = *gen_callee;
                let gen_args_range = *gen_args;
                if let ExprKind::Field { base, field } = checker.pool.expr(gen_callee_id) {
                    let base_id = *base;
                    let field_span = checker.pool.expr_span(gen_callee_id);

                    let base_ty = synth_expr(checker, base_id);
                    if !base_ty.is_error() {
                        let instantiated_method_ty = types::synth_generic_instantiation(
                            checker,
                            gen_callee_id,
                            gen_args_range,
                            field_span,
                        );
                        if let ArType::Func(params, ret) = instantiated_method_ty
                            && !params.is_empty()
                        {
                            let actual_base_ty = match &base_ty {
                                ArType::Nullable(inner) => {
                                    checker.type_info.type_interner.resolve(*inner).clone()
                                }
                                other => other.clone(),
                            };
                            let receiver_ty_id = params[0];
                            let receiver_ty = checker.type_info.type_interner.resolve(receiver_ty_id).clone();
                            if !types::unify(&receiver_ty, &actual_base_ty, &checker.type_info.type_interner) {
                                checker.add_constraint(
                                    receiver_ty.clone(),
                                    actual_base_ty.clone(),
                                    ConstraintOrigin::CallArg {
                                        call_span: span,
                                        param_span: field_span,
                                        arg_span: checker.pool.expr_span(base_id),
                                        arg_index: 0,
                                    },
                                );
                            }
                            let explicit_params = &params[1..];
                            let arg_ids = checker.pool.expr_list(args_range).to_vec();
                            if explicit_params.len() != arg_ids.len() {
                                let struct_id = match &actual_base_ty {
                                    ArType::Named(id, _) => Some(*id),
                                    _ => None,
                                };
                                let struct_name = struct_id.map_or("Struct".to_string(), |id| {
                                    checker.symbols.get(id).name.clone()
                                });
                                let diag = crate::Diagnostic::error(
                                    crate::DiagCode::T012WrongArgCount,
                                    format!(
                                        "method '{struct_name}.{field}' expects {} argument(s), found {}",
                                        explicit_params.len(),
                                        arg_ids.len()
                                    ),
                                    span,
                                )
                                .with_label(field_span, "call target is here")
                                .with_label(span, format!("{} arguments provided", arg_ids.len()));
                                checker.diagnostics.push(diag);
                            }
                            for (i, arg_id) in arg_ids.iter().copied().enumerate() {
                                let arg_ty = synth_expr(checker, arg_id);
                                if let Some(&expected_id) = explicit_params.get(i) {
                                    let expected = checker.type_info.type_interner.resolve(expected_id).clone();
                                    if !types::unify(&expected, &arg_ty, &checker.type_info.type_interner) {
                                        checker.add_constraint(
                                            expected.clone(),
                                            arg_ty.clone(),
                                            ConstraintOrigin::CallArg {
                                                call_span: span,
                                                param_span: field_span,
                                                arg_span: checker.pool.expr_span(arg_id),
                                                arg_index: i + 1,
                                            },
                                        );
                                    }
                                }
                            }
                            checker.record_expr_type(callee_id, ArType::Func(params, ret));
                            let resolved_ret =
                                checker.type_info.type_interner.resolve(ret).clone();
                            return Some(resolved_ret);
                        }
                    }
                }
            }

            let callee_ty = synth_expr(checker, callee_id);
            let arg_ids = checker.pool.expr_list(args_range).to_vec();
            Some(match callee_ty {
                ArType::Func(params, ret) => {
                    if params.len() != arg_ids.len() {
                        let diag = crate::Diagnostic::error(
                            crate::DiagCode::T012WrongArgCount,
                            format!(
                                "expected {} arguments, found {}",
                                params.len(),
                                arg_ids.len()
                            ),
                            span,
                        )
                        .with_label(checker.pool.expr_span(callee_id), "call target is here")
                        .with_label(span, format!("{} arguments provided", arg_ids.len()));
                        checker.diagnostics.push(diag);
                    }
                    for (i, arg_id) in arg_ids.iter().copied().enumerate() {
                        let arg_ty = synth_expr(checker, arg_id);
                        if i < params.len() {
                            let param_id = params[i];
                            let param_ty = checker.type_info.type_interner.resolve(param_id).clone();
                            if !types::unify(&param_ty, &arg_ty, &checker.type_info.type_interner) {
                                checker.add_constraint(
                                    param_ty.clone(),
                                    arg_ty.clone(),
                                    ConstraintOrigin::CallArg {
                                        call_span: span,
                                        param_span: checker.pool.expr_span(callee_id),
                                        arg_span: checker.pool.expr_span(arg_id),
                                        arg_index: i,
                                    },
                                );
                            } else if !arg_ty.is_literal()
                                && arg_ty != param_ty
                                && param_ty.is_numeric()
                                && arg_ty.is_numeric()
                            {
                                checker.add_constraint(
                                    param_ty.clone(),
                                    arg_ty,
                                    ConstraintOrigin::ImplicitWidening {
                                        source_span: checker.pool.expr_span(arg_id),
                                        target_span: span,
                                    },
                                );
                            }
                        }
                    }
                    checker.type_info.type_interner.resolve(ret).clone()
                }
                ArType::Error => ArType::Error,
                other => {
                    let interner = &checker.type_info.type_interner;
                    let diag = crate::Diagnostic::error(
                        crate::DiagCode::T003IncompatibleCallArg,
                        format!(
                            "cannot call non-function type '{}'",
                            other.display(&checker.symbols, interner)
                        ),
                        span,
                    )
                    .with_label(
                        checker.pool.expr_span(callee_id),
                        format!("type is '{}'", other.display(&checker.symbols, interner)),
                    )
                    .with_label(span, "call site");
                    checker.diagnostics.push(diag);
                    ArType::Error
                }
            })
        }
        ExprKind::Catch {
            expr: inner_expr,
            handler,
        } => {
            let inner_id = *inner_expr;
            let handler_id = *handler;
            let inner_ty = synth_expr(checker, inner_id);
            let handler_def = checker.pool.catch_handler(handler_id);
            let handler_ty = match handler_def {
                CatchHandler::Expr {
                    expr: h,
                    span: h_span,
                } => {
                    let ty = synth_expr(checker, *h);
                    (*h_span, ty)
                }
                CatchHandler::Block {
                    block,
                    span: h_span,
                    ..
                } => {
                    let ty = crate::type_checker::check::check_block(
                        checker,
                        checker.pool,
                        block,
                    );
                    (*h_span, ty)
                }
            };
            Some(
                if let Some((ok_ty, _)) = checker.result_ok_err(&inner_ty) {
                    if !types::unify(&ok_ty, &handler_ty.1, &checker.type_info.type_interner) {
                        checker.add_constraint(
                            ok_ty.clone(),
                            handler_ty.1.clone(),
                            ConstraintOrigin::CatchHandler {
                                expr_span: checker.pool.expr_span(inner_id),
                                handler_span: handler_ty.0,
                            },
                        );
                    }
                    ok_ty
                } else if inner_ty.is_error() {
                    ArType::Error
                } else {
                    checker.diagnostics.push(
                        crate::Diagnostic::error(
                            crate::DiagCode::T005OperatorNotApplicable,
                            format!(
                                "`catch` requires a `Result` expression, found '{}'",
                                inner_ty.display(&checker.symbols, &checker.type_info.type_interner)
                            ),
                            span,
                        )
                        .with_label(
                            checker.pool.expr_span(inner_id),
                            "expression is not a Result",
                        ),
                    );
                    ArType::Error
                },
            )
        }
        _ => None,
    }
}
