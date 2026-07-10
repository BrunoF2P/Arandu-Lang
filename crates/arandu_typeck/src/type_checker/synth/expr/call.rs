use arandu_lexer::Span;
use arandu_parser::CatchHandler;
use arandu_parser::ast_pool::{ExprId, ExprKind};

use super::synth_expr;
use crate::type_checker::TypeChecker;
use crate::type_checker::constraints::ConstraintOrigin;
use crate::type_checker::synth::{
    resolve_field, resolve_index, resolve_namespace_field, resolve_namespace_member_type,
    synth_method_call, synth_option_ctor, synth_result_ctor,
};
use crate::type_checker::types::{self, ArType, Primitive};

use arandu_middle::types::type_interner::TypeId;

/// Check a call argument against its formal parameter.
///
/// When the formal type is `str` and the argument is ToStr-v0.1-formatable,
/// accept without a constraint (AMIR lower inserts `ToStr`). When formal is
/// `str` and the argument is not formatable, emit T034. Otherwise fall back
/// to the usual CallArg constraint.
pub(crate) fn check_call_arg(
    checker: &mut TypeChecker<'_>,
    param_id: TypeId,
    arg_ty_id: TypeId,
    call_span: Span,
    param_span: Span,
    arg_span: Span,
    arg_index: usize,
) {
    if checker.unify_ids(param_id, arg_ty_id) {
        let arg_ty = checker.resolve(arg_ty_id);
        let param_ty = checker.resolve(param_id);
        if !arg_ty.is_literal()
            && arg_ty != param_ty
            && param_ty.is_numeric()
            && arg_ty.is_numeric()
        {
            checker.add_constraint(
                param_id,
                arg_ty_id,
                ConstraintOrigin::ImplicitWidening {
                    source_span: arg_span,
                    target_span: call_span,
                },
            );
        }
        return;
    }

    let param_ty = checker.resolve(param_id);
    let arg_ty = checker.resolve(arg_ty_id);
    if matches!(param_ty, ArType::Primitive(Primitive::Str)) {
        if arg_ty.is_error() || arg_ty.is_to_str_v01() {
            // ToStr v0.1: lower will insert AmirRvalue::ToStr.
            return;
        }
        let interner = &checker.type_info.type_interner;
        let found = arg_ty.display(&checker.symbols, interner);
        checker.diagnostics.push(
            crate::Diagnostic::error(
                crate::DiagCode::T034CannotFormat,
                format!("cannot format value of type `{found}` as `str`"),
                arg_span,
            )
            .with_note(
                "only bool, integers, floats, char, and str are supported in v0.1".to_string(),
            )
            .with_label(param_span, "parameter expects `str`"),
        );
        return;
    }

    checker.add_constraint(
        param_id,
        arg_ty_id,
        ConstraintOrigin::CallArg {
            call_span,
            param_span,
            arg_span,
            arg_index,
        },
    );
}

#[tracing::instrument(level = "trace", target = "arandu_typeck", skip(checker, expr))]
pub(super) fn synth_call_expr(
    checker: &mut TypeChecker<'_>,
    expr: ExprId,
    kind: &ExprKind,
    span: Span,
) -> Option<TypeId> {
    match kind {
        ExprKind::Path { path: _ } => {
            if let Some(symbol_id) = checker.resolved.expr_symbol(expr) {
                if let Some(ty_id) = checker.ctx.lookup(symbol_id) {
                    return Some(ty_id);
                }
                if let Some(ty_id) = checker.decl_type_id(symbol_id) {
                    return Some(ty_id);
                }
            }
            Some(checker.intern(ArType::Error))
        }
        ExprKind::TypePath { type_name, member } => {
            if types::type_name_base(type_name) == "Result" {
                return Some(match member.as_str() {
                    "Ok" => {
                        let err_id = checker.intern(ArType::Error);
                        let err_literal_id = checker.intern(ArType::Err);
                        let res_ty = ArType::Result(err_id, err_literal_id);
                        let res_id = checker.intern(res_ty);
                        checker.intern(ArType::Func(vec![err_id], res_id))
                    }
                    "Err" => {
                        let err_id = checker.intern(ArType::Error);
                        let err_literal_id = checker.intern(ArType::Err);
                        let res_ty = ArType::Result(err_id, err_literal_id);
                        let res_id = checker.intern(res_ty);
                        checker.intern(ArType::Func(vec![err_id], res_id))
                    }
                    _ => checker.intern(ArType::Error),
                });
            }
            if types::type_name_base(type_name) == "Option" {
                return Some(match member.as_str() {
                    "Some" => {
                        let err_id = checker.intern(ArType::Error);
                        let opt_ty = ArType::Option(err_id);
                        let opt_id = checker.intern(opt_ty);
                        checker.intern(ArType::Func(vec![err_id], opt_id))
                    }
                    "None" => {
                        let err_id = checker.intern(ArType::Error);
                        checker.intern(ArType::Option(err_id))
                    }
                    _ => checker.intern(ArType::Error),
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
                    && let Some((_, shape)) = checker
                        .type_info
                        .enum_variants
                        .get(&variant_symbol_id)
                        .cloned()
                {
                    let enum_ty = ArType::Named(*enum_symbol_id, vec![]);
                    match shape {
                        crate::type_checker::EnumPayloadShape::Unit => {
                            return Some(checker.intern(enum_ty));
                        }
                        crate::type_checker::EnumPayloadShape::Tuple(tys) => {
                            let param_ids = tys.iter().map(|t| checker.intern(t.clone())).collect();
                            let enum_id = checker.intern(enum_ty);
                            return Some(checker.intern(ArType::Func(param_ids, enum_id)));
                        }
                    }
                }
            }

            if let Some(symbol_id) = checker.resolved.expr_symbol(expr)
                && let Some(ty_id) = checker.decl_type_id(symbol_id)
            {
                return Some(ty_id);
            }
            Some(checker.intern(ArType::Error))
        }
        ExprKind::Generic { callee, args } => {
            let callee_id = *callee;
            let args_range = *args;
            let ty = types::synth_generic_instantiation(checker, callee_id, args_range, span);
            Some(checker.intern(ty))
        }
        ExprKind::Field { base, field } => {
            let base_id = *base;
            let field_str = field.clone();
            let ty_id = if let Some(ty_id) =
                resolve_namespace_field(checker, base_id, expr, &field_str, span)
            {
                ty_id
            } else if let Some(ty_id) = resolve_namespace_member_type(checker, expr) {
                ty_id
            } else {
                resolve_field(checker, base_id, &field_str, span, false)
            };
            Some(ty_id)
        }
        ExprKind::SafeField { base, field } => {
            let base_id = *base;
            let field_str = field.clone();
            let ty_id = if let Some(ty_id) =
                resolve_namespace_field(checker, base_id, expr, &field_str, span)
            {
                ty_id
            } else if let Some(ty_id) = resolve_namespace_member_type(checker, expr) {
                ty_id
            } else {
                resolve_field(checker, base_id, &field_str, span, true)
            };
            Some(ty_id)
        }
        ExprKind::Index { base, index } => {
            let base_id = *base;
            let index_id = *index;
            let ty_id = resolve_index(checker, base_id, index_id, false);
            Some(ty_id)
        }
        ExprKind::SafeIndex { base, index } => {
            let base_id = *base;
            let index_id = *index;
            let ty_id = resolve_index(checker, base_id, index_id, true);
            Some(ty_id)
        }
        ExprKind::Try { expr: inner_expr } => {
            let inner_id = *inner_expr;
            let inner_ty_id = synth_expr(checker, inner_id);
            let inner_ty = checker.resolve(inner_ty_id);
            Some(if let Some(ok_ty) = checker.try_ok_type(&inner_ty) {
                checker.intern(ok_ty)
            } else if inner_ty.is_error() {
                checker.intern(ArType::Error)
            } else {
                checker.add_constraint(
                    ArType::Error,
                    inner_ty_id,
                    ConstraintOrigin::TryInvalid { span },
                );
                checker.intern(ArType::Error)
            })
        }
        ExprKind::Call {
            callee,
            args,
            trailing_block: _,
        } => {
            let callee_id = *callee;
            let args_range = *args;
            if let Some(callee_sym) = checker.resolved.expr_symbol(callee_id) {
                let sym = checker.symbols.get(callee_sym);
                if sym.kind == arandu_middle::SymbolKind::ExternFunc && !checker.ctx.is_in_unsafe()
                {
                    checker.diagnostics.push(
                        crate::Diagnostic::error(
                            crate::DiagCode::O013ExternRequiresUnsafe,
                            "call to extern function requires an `unsafe` block",
                            span,
                        )
                        .with_label(span, "`extern` functions are unsafe and must be called inside an `unsafe` block"),
                    );
                }
                if Some(callee_sym) == checker.symbols.builtin_alloc {
                    let arg_ids = checker.pool.expr_list(args_range).to_vec();
                    let arg_ty = if let Some(first) = arg_ids.first() {
                        super::synth_expr(checker, *first)
                    } else {
                        checker.intern(ArType::Error)
                    };
                    let ptr_ty = checker.intern(ArType::Ptr(arg_ty));
                    checker.record_expr_type(expr, ptr_ty);
                    return Some(ptr_ty);
                }
                if Some(callee_sym) == checker.symbols.builtin_free {
                    let arg_ids = checker.pool.expr_list(args_range).to_vec();
                    if let Some(first) = arg_ids.first() {
                        let arg_ty_id = super::synth_expr(checker, *first);
                        let arg_ty = checker.resolve(arg_ty_id).clone();
                        if !arg_ty.is_error() && !matches!(arg_ty, ArType::Ptr(_)) {
                            let interner = &checker.type_info.type_interner;
                            checker.diagnostics.push(
                                crate::Diagnostic::error(
                                    crate::DiagCode::O011FreeRequiresPtr,
                                    format!(
                                        "`free` requires a pointer type (`ptr[T]`), found '{}'",
                                        arg_ty.display(&checker.symbols, interner)
                                    ),
                                    span,
                                )
                                .with_label(
                                    checker.pool.expr_span(*first),
                                    format!(
                                        "expression has type '{}'",
                                        arg_ty.display(&checker.symbols, interner)
                                    ),
                                ),
                            );
                        }
                    }
                    let void_ty = checker.intern(ArType::Void);
                    checker.record_expr_type(expr, void_ty);
                    return Some(void_ty);
                }
            }
            if let Some(result_ty) = synth_result_ctor(checker, callee_id, args_range, span) {
                return Some(checker.intern(result_ty));
            }
            if let Some(option_ty) = synth_option_ctor(checker, callee_id, args_range, span) {
                return Some(checker.intern(option_ty));
            }
            if let ExprKind::Field { base, field } = checker.pool.expr(callee_id) {
                let base_id = *base;
                let field_str = field.clone();
                let field_span = checker.pool.expr_span(callee_id);

                // Root cause fix (RC-NEST / namespace calls):
                // `io.println(x)` is a Field callee. Trying method dispatch first
                // synthesizes `io` as a value Path → Error, then returns
                // `Some(Error)` and never types the arguments (including nested
                // method calls). Resolve namespace members before methods.
                if let Some(ns_ty_id) =
                    resolve_namespace_field(checker, base_id, callee_id, &field_str, field_span)
                {
                    let arg_ids = checker.pool.expr_list(args_range).to_vec();
                    let ns_ty = checker.resolve(ns_ty_id);
                    if let ArType::Func(params, ret) = ns_ty {
                        let params = params.clone();
                        if params.len() != arg_ids.len() {
                            checker.diagnostics.push(
                                crate::Diagnostic::error(
                                    crate::DiagCode::T012WrongArgCount,
                                    format!(
                                        "expected {} arguments, found {}",
                                        params.len(),
                                        arg_ids.len()
                                    ),
                                    span,
                                )
                                .with_label(field_span, "call target is here")
                                .with_label(span, format!("{} arguments provided", arg_ids.len())),
                            );
                        }
                        for (i, arg_id) in arg_ids.iter().copied().enumerate() {
                            let arg_ty_id = synth_expr(checker, arg_id);
                            if let Some(&param_id) = params.get(i) {
                                check_call_arg(
                                    checker,
                                    param_id,
                                    arg_ty_id,
                                    span,
                                    field_span,
                                    checker.pool.expr_span(arg_id),
                                    i,
                                );
                            }
                        }
                        // Mark as a direct call target for HIR/codegen.
                        let func_ty_id = checker.intern(ArType::Func(params, ret));
                        checker.record_expr_type(callee_id, func_ty_id);
                        return Some(ret);
                    }
                }

                if let Some(ret_id) = synth_method_call(
                    checker, base_id, callee_id, &field_str, field_span, args_range, span,
                ) {
                    return Some(ret_id);
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

                    let base_ty_id = synth_expr(checker, base_id);
                    if !checker.resolve(base_ty_id).is_error() {
                        let instantiated_method_ty = types::synth_generic_instantiation(
                            checker,
                            gen_callee_id,
                            gen_args_range,
                            field_span,
                        );
                        if let ArType::Func(params, ret) = instantiated_method_ty
                            && !params.is_empty()
                        {
                            let actual_base_ty_id = match checker.resolve(base_ty_id) {
                                ArType::Nullable(inner) => inner,
                                _ => base_ty_id,
                            };
                            let receiver_ty_id = params[0];
                            if !checker.unify_ids(receiver_ty_id, actual_base_ty_id) {
                                checker.add_constraint(
                                    receiver_ty_id,
                                    actual_base_ty_id,
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
                                let struct_id = match checker.resolve(actual_base_ty_id) {
                                    ArType::Named(id, _) => Some(id),
                                    _ => None,
                                };
                                let struct_name = struct_id.map_or("Struct".to_string(), |id| {
                                    checker.symbols.get(id).name.to_string()
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
                                let arg_ty_id = synth_expr(checker, arg_id);
                                if let Some(&expected_id) = explicit_params.get(i) {
                                    check_call_arg(
                                        checker,
                                        expected_id,
                                        arg_ty_id,
                                        span,
                                        field_span,
                                        checker.pool.expr_span(arg_id),
                                        i + 1,
                                    );
                                }
                            }
                            let params_id = checker.intern(ArType::Func(params, ret));
                            checker.record_expr_type(callee_id, params_id);
                            return Some(ret);
                        }
                    }
                }
            }

            let callee_ty_id = synth_expr(checker, callee_id);
            let arg_ids = checker.pool.expr_list(args_range).to_vec();
            let callee_ty = checker.resolve(callee_ty_id);
            let func_info = if let ArType::Func(ref params, ret) = callee_ty {
                Some((params.clone(), ret))
            } else {
                None
            };
            let is_error = matches!(callee_ty, ArType::Error);

            Some(if let Some((params, ret)) = func_info {
                let mut is_direct = false;
                let mut current_callee = callee_id;
                if let ExprKind::Generic { callee: inner, .. } = checker.pool.expr(current_callee) {
                    current_callee = *inner;
                }
                match checker.pool.expr(current_callee) {
                    ExprKind::Path { .. } => {
                        if let Some(sym_id) = checker.resolved.expr_symbol(current_callee) {
                            let sym = checker.symbols.get(sym_id);
                            if matches!(
                                sym.kind,
                                arandu_middle::SymbolKind::Func
                                    | arandu_middle::SymbolKind::ExternFunc
                                    | arandu_middle::SymbolKind::EnumVariant
                                    | arandu_middle::SymbolKind::AssociatedFunc
                            ) {
                                is_direct = true;
                            }
                        }
                    }
                    ExprKind::TypePath { .. } => {
                        // Any TypePath that resolves to a Func type is a static constructor or associated function.
                        // It cannot be a local variable or field. Thus, if it's callable, it's a direct call.
                        is_direct = true;
                    }
                    _ => {}
                }

                if !is_direct {
                    let diag = crate::Diagnostic::error(
                        crate::DiagCode::T033IndirectCallNotSupported,
                        "indirect function calls are not supported",
                        span,
                    )
                    .with_label(
                        checker.pool.expr_span(callee_id),
                        "this expression evaluates to a function, but Arandu only supports direct calls",
                    );
                    checker.diagnostics.push(diag);
                }

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
                    let arg_ty_id = synth_expr(checker, arg_id);
                    if i < params.len() {
                        check_call_arg(
                            checker,
                            params[i],
                            arg_ty_id,
                            span,
                            checker.pool.expr_span(callee_id),
                            checker.pool.expr_span(arg_id),
                            i,
                        );
                    }
                }

                if !is_direct {
                    checker.intern(ArType::Error)
                } else {
                    ret
                }
            } else if is_error {
                checker.intern(ArType::Error)
            } else {
                let callee_ty = checker.resolve(callee_ty_id);
                let interner = &checker.type_info.type_interner;
                let diag = crate::Diagnostic::error(
                    crate::DiagCode::T003IncompatibleCallArg,
                    format!(
                        "cannot call non-function type '{}'",
                        callee_ty.display(&checker.symbols, interner)
                    ),
                    span,
                )
                .with_label(
                    checker.pool.expr_span(callee_id),
                    format!(
                        "this has type '{}'",
                        callee_ty.display(&checker.symbols, interner)
                    ),
                );
                checker.diagnostics.push(diag);
                checker.intern(ArType::Error)
            })
        }
        ExprKind::Catch {
            expr: inner_expr,
            handler,
        } => {
            let inner_id = *inner_expr;
            let handler_id = *handler;
            let inner_ty_id = synth_expr(checker, inner_id);
            let handler_def = checker.pool.catch_handler(handler_id);
            let handler_ty_id = match handler_def {
                CatchHandler::Expr {
                    expr: h,
                    span: h_span,
                } => {
                    let ty_id = synth_expr(checker, *h);
                    (*h_span, ty_id)
                }
                CatchHandler::Block {
                    block,
                    span: h_span,
                    error: _,
                    ..
                } => {
                    // Bind `|e|` to the Result error type before type-checking the body.
                    if let Some((_, err_ty_id)) = checker.result_ok_err_ids(inner_ty_id) {
                        let err_key = crate::NodeKey::from(*h_span);
                        if let Some(symbol_id) =
                            checker.resolved.definitions.get(&err_key).copied()
                        {
                            checker.ctx.bind(symbol_id, err_ty_id);
                            checker.record_decl_type(symbol_id, err_ty_id);
                        }
                    }
                    let ty = crate::type_checker::check::check_block(checker, checker.pool, block);
                    (*h_span, checker.intern(ty))
                }
            };
            Some(
                if let Some((ok_ty_id, _)) = checker.result_ok_err_ids(inner_ty_id) {
                    if !checker.unify_ids(ok_ty_id, handler_ty_id.1) {
                        checker.add_constraint(
                            ok_ty_id,
                            handler_ty_id.1,
                            ConstraintOrigin::CatchHandler {
                                expr_span: checker.pool.expr_span(inner_id),
                                handler_span: handler_ty_id.0,
                            },
                        );
                    }
                    ok_ty_id
                } else if checker.resolve(inner_ty_id).is_error() {
                    checker.intern(ArType::Error)
                } else {
                    let inner_ty = checker.resolve(inner_ty_id);
                    let interner = &checker.type_info.type_interner;
                    checker.diagnostics.push(
                        crate::Diagnostic::error(
                            crate::DiagCode::T005OperatorNotApplicable,
                            format!(
                                "operator `catch` requires a `Result` type, found '{}'",
                                inner_ty.display(&checker.symbols, interner)
                            ),
                            span,
                        )
                        .with_label(
                            checker.pool.expr_span(inner_id),
                            format!("type is '{}'", inner_ty.display(&checker.symbols, interner)),
                        ),
                    );
                    checker.intern(ArType::Error)
                },
            )
        }
        _ => None,
    }
}
