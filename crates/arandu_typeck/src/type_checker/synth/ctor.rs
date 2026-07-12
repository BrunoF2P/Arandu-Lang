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

/// T2.2: `.Ok(x)` / `.None` / `.Some(v)` / `.Pending` with expected type context.
pub(crate) fn synth_variant_sugar(
    checker: &mut TypeChecker<'_>,
    expr: ExprId,
    name: &str,
    args: IndexRange,
    expected: Option<TypeId>,
    span: Span,
) -> TypeId {
    let arg_ids = checker.pool.expr_list(args).to_vec();
    let Some(expected_id) = expected.filter(|id| !checker.resolve(*id).is_error()) else {
        checker.diagnostics.push(
            crate::Diagnostic::error(
                crate::DiagCode::T003IncompatibleCallArg,
                format!(
                    "variant sugar `.{name}` requires an expected type (e.g. return type or annotation)"
                ),
                span,
            )
            .with_note(
                "write `Result.Ok(...)` / `Option.None` explicitly, or use in a typed context"
                    .to_string(),
            ),
        );
        return checker.intern(ArType::Error);
    };
    let expected_ty = checker.resolve(expected_id);

    match expected_ty {
        ArType::Result(ok_id, err_id) => match name {
            "Ok" => {
                if arg_ids.len() != 1 {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T012WrongArgCount,
                        format!(".Ok expects 1 argument, found {}", arg_ids.len()),
                        span,
                    ));
                    return checker.intern(ArType::Error);
                }
                let got = synth_expr(checker, arg_ids[0]);
                if !checker.unify_ids(ok_id, got) {
                    checker.add_constraint(
                        ok_id,
                        got,
                        ConstraintOrigin::CallArg {
                            call_span: span,
                            param_span: span,
                            arg_span: checker.pool.expr_span(arg_ids[0]),
                            arg_index: 0,
                        },
                    );
                }
                expected_id
            }
            "Err" => {
                if arg_ids.len() != 1 {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T012WrongArgCount,
                        format!(".Err expects 1 argument, found {}", arg_ids.len()),
                        span,
                    ));
                    return checker.intern(ArType::Error);
                }
                let got = synth_expr(checker, arg_ids[0]);
                if !checker.unify_ids(err_id, got) {
                    checker.add_constraint(
                        err_id,
                        got,
                        ConstraintOrigin::CallArg {
                            call_span: span,
                            param_span: span,
                            arg_span: checker.pool.expr_span(arg_ids[0]),
                            arg_index: 0,
                        },
                    );
                }
                expected_id
            }
            _ => {
                checker.diagnostics.push(crate::Diagnostic::error(
                    crate::DiagCode::T018UndefinedField,
                    format!("`.{name}` is not a Result variant (expected Ok or Err)"),
                    span,
                ));
                checker.intern(ArType::Error)
            }
        },
        ArType::Option(inner_id) => match name {
            "Some" => {
                if arg_ids.len() != 1 {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T012WrongArgCount,
                        format!(".Some expects 1 argument, found {}", arg_ids.len()),
                        span,
                    ));
                    return checker.intern(ArType::Error);
                }
                let got = synth_expr(checker, arg_ids[0]);
                if !checker.unify_ids(inner_id, got) {
                    checker.add_constraint(
                        inner_id,
                        got,
                        ConstraintOrigin::CallArg {
                            call_span: span,
                            param_span: span,
                            arg_span: checker.pool.expr_span(arg_ids[0]),
                            arg_index: 0,
                        },
                    );
                }
                expected_id
            }
            "None" => {
                if !arg_ids.is_empty() {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T012WrongArgCount,
                        format!(".None expects 0 arguments, found {}", arg_ids.len()),
                        span,
                    ));
                    return checker.intern(ArType::Error);
                }
                expected_id
            }
            _ => {
                checker.diagnostics.push(crate::Diagnostic::error(
                    crate::DiagCode::T018UndefinedField,
                    format!("`.{name}` is not an Option variant (expected Some or None)"),
                    span,
                ));
                checker.intern(ArType::Error)
            }
        },
        ArType::Poll(inner_id) => match name {
            "Ready" => {
                if arg_ids.len() != 1 {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T012WrongArgCount,
                        format!(".Ready expects 1 argument, found {}", arg_ids.len()),
                        span,
                    ));
                    return checker.intern(ArType::Error);
                }
                let got = synth_expr(checker, arg_ids[0]);
                if !checker.unify_ids(inner_id, got) {
                    checker.add_constraint(
                        inner_id,
                        got,
                        ConstraintOrigin::CallArg {
                            call_span: span,
                            param_span: span,
                            arg_span: checker.pool.expr_span(arg_ids[0]),
                            arg_index: 0,
                        },
                    );
                }
                expected_id
            }
            "Pending" => {
                if !arg_ids.is_empty() {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T012WrongArgCount,
                        format!(".Pending expects 0 arguments, found {}", arg_ids.len()),
                        span,
                    ));
                    return checker.intern(ArType::Error);
                }
                expected_id
            }
            _ => {
                checker.diagnostics.push(crate::Diagnostic::error(
                    crate::DiagCode::T018UndefinedField,
                    format!("`.{name}` is not a Poll variant (expected Ready or Pending)"),
                    span,
                ));
                checker.intern(ArType::Error)
            }
        },
        ArType::Named(enum_id, _) => {
            let enum_name = checker.symbols.get(enum_id).name.clone();
            let Some(variant_sym) = checker.symbols.lookup_associated_member(&enum_name, name)
            else {
                checker.diagnostics.push(crate::Diagnostic::error(
                    crate::DiagCode::T018UndefinedField,
                    format!("`.{name}` is not a variant of `{enum_name}`"),
                    span,
                ));
                return checker.intern(ArType::Error);
            };
            // Record resolution for HIR (same as TypePath member).
            checker.resolved.value_ref(span, variant_sym);
            checker.resolved.expr_ref(expr, variant_sym);
            // Type args of payload: use variant decl type if Func-like, else unit.
            if let Some(ArType::Func(params, ret)) = checker.decl_type(variant_sym) {
                if params.len() != arg_ids.len() {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T012WrongArgCount,
                        format!(
                            ".{name} expects {} argument(s), found {}",
                            params.len(),
                            arg_ids.len()
                        ),
                        span,
                    ));
                    return checker.intern(ArType::Error);
                }
                for (i, &arg) in arg_ids.iter().enumerate() {
                    let got = synth_expr(checker, arg);
                    if let Some(&param) = params.get(i)
                        && !checker.unify_ids(param, got)
                    {
                        checker.add_constraint(
                            param,
                            got,
                            ConstraintOrigin::CallArg {
                                call_span: span,
                                param_span: span,
                                arg_span: checker.pool.expr_span(arg),
                                arg_index: i,
                            },
                        );
                    }
                }
                let _ = ret;
            } else if !arg_ids.is_empty() {
                checker.diagnostics.push(crate::Diagnostic::error(
                    crate::DiagCode::T012WrongArgCount,
                    format!(".{name} expects 0 arguments, found {}", arg_ids.len()),
                    span,
                ));
                return checker.intern(ArType::Error);
            }
            expected_id
        }
        other => {
            let interner = &checker.type_info.type_interner;
            let disp = other.display(&checker.symbols, interner);
            checker.diagnostics.push(crate::Diagnostic::error(
                crate::DiagCode::T003IncompatibleCallArg,
                format!("variant sugar `.{name}` cannot target type `{disp}`"),
                span,
            ));
            checker.intern(ArType::Error)
        }
    }
}

/// A3.6: `Poll.Ready(v)` / `Poll.Pending` (builtin generic like Option).
pub(crate) fn synth_poll_ctor(
    checker: &mut TypeChecker<'_>,
    callee: ExprId,
    args: IndexRange,
    span: Span,
) -> Option<ArType> {
    let (type_name, member) = type_path_member(checker.pool, callee)?;
    if super::super::types::type_name_base(type_name) != "Poll" {
        return None;
    }
    let arg_ids = checker.pool.expr_list(args).to_vec();
    match member {
        "Ready" => {
            if arg_ids.len() != 1 {
                let diag = crate::Diagnostic::error(
                    crate::DiagCode::T012WrongArgCount,
                    format!("Poll.Ready expects 1 argument, found {}", arg_ids.len()),
                    span,
                )
                .with_label(checker.pool.expr_span(callee), "call target is here")
                .with_label(span, format!("{} arguments provided", arg_ids.len()));
                checker.diagnostics.push(diag);
                return Some(ArType::Error);
            }
            let inner_id = synth_expr(checker, arg_ids[0]);
            Some(ArType::Poll(inner_id))
        }
        "Pending" => {
            if !arg_ids.is_empty() {
                let diag = crate::Diagnostic::error(
                    crate::DiagCode::T012WrongArgCount,
                    format!("Poll.Pending expects 0 arguments, found {}", arg_ids.len()),
                    span,
                )
                .with_label(checker.pool.expr_span(callee), "call target is here");
                checker.diagnostics.push(diag);
                return Some(ArType::Error);
            }
            // Inner type from expected context; Error placeholder if unknown.
            let placeholder = checker.intern(ArType::Error);
            Some(ArType::Poll(placeholder))
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
    // If `base` is a namespace module path (`io.foo`), this is not a method
    // call — let the Call path handle namespace members. Returning `Some(Error)`
    // here previously poisoned `io.println(...)` and skipped argument typing.
    if let ExprKind::Path { path } = checker.pool.expr(base)
        && path.len() == 1
        && checker
            .symbols
            .lookup_module(checker.symbols.global_scope(), path[0].as_str())
            .is_some()
    {
        return None;
    }

    let base_ty_id = synth_expr(checker, base);
    if checker.resolve(base_ty_id).is_error() {
        // Receiver already failed to type; avoid cascading "no method" noise.
        return Some(checker.intern(ArType::Error));
    }

    let actual_base_ty_id = match checker.resolve(base_ty_id) {
        ArType::Nullable(inner) => inner,
        _ => base_ty_id,
    };

    // ToStr v0.1 intrinsic: `receiver.to_str()` with zero args → `str`.
    if method == "to_str" {
        let arg_ids = checker.pool.expr_list(args).to_vec();
        if !arg_ids.is_empty() {
            checker.diagnostics.push(
                crate::Diagnostic::error(
                    crate::DiagCode::T012WrongArgCount,
                    format!(
                        "method 'to_str' expects 0 argument(s), found {}",
                        arg_ids.len()
                    ),
                    call_span,
                )
                .with_label(field_span, "call target is here"),
            );
            return Some(checker.intern(ArType::Error));
        }
        let base_ty = checker.resolve(actual_base_ty_id);
        let str_id = checker.intern(ArType::Primitive(
            crate::type_checker::types::Primitive::Str,
        ));
        if base_ty.is_to_str_v01() {
            let func_id = checker.intern(ArType::Func(vec![actual_base_ty_id], str_id));
            checker.record_expr_type(callee, func_id);
            return Some(str_id);
        }
        let interner = &checker.type_info.type_interner;
        let found = base_ty.display(&checker.symbols, interner);
        checker.diagnostics.push(
            crate::Diagnostic::error(
                crate::DiagCode::T034CannotFormat,
                format!("cannot format value of type `{found}` as `str`"),
                checker.pool.expr_span(base),
            )
            .with_note(
                "only bool, integers, floats, char, and str are supported in v0.1".to_string(),
            )
            .with_label(field_span, "to_str is not available for this type"),
        );
        return Some(checker.intern(ArType::Error));
    }

    let base_resolved = checker.resolve(actual_base_ty_id);

    // Built-in `Result` / `Option` methods (`expectOrAbort`) live under the type
    // name string, while the receiver is `ArType::Result` / `Option` (not Named).
    let builtin_name: Option<&str> = match &base_resolved {
        ArType::Result(_, _) => Some("Result"),
        ArType::Option(_) => Some("Option"),
        _ => None,
    };

    let struct_id = match &base_resolved {
        ArType::Named(id, _) => Some(*id),
        ArType::Ptr(inner) => match checker.resolve(*inner) {
            ArType::Named(id, _) => Some(id),
            _ => None,
        },
        _ => None,
    };

    let struct_name = if let Some(id) = struct_id {
        checker.symbols.get(id).name.clone()
    } else if let Some(n) = builtin_name {
        n.into()
    } else {
        return None;
    };

    let method_sym = checker
        .symbols
        .lookup_associated_member(&struct_name, method);

    let mut resolved_method = None;
    if method_sym.is_none()
        && let Some(sid) = struct_id
        && let Some(constraints) = checker.type_info.param_constraints.get(&sid)
    {
        for &iface_sym in constraints.iter() {
            if let Some(iface_info) = checker.type_info.interfaces.get(&iface_sym)
                && let Some((_, method_tid)) = iface_info.methods.iter().find(|(m, _)| m == method)
            {
                resolved_method = Some(checker.resolve(*method_tid));
                break;
            }
        }
    }

    let (params, ret, method_sym_recorded) = if let Some(method_sig) = resolved_method {
        if let ArType::Func(params, ret) = method_sig {
            // Interface methods may declare an explicit `self`/`Self` receiver or
            // only the free-style payload (`Allocator.alloc(size, align)`).
            // Drop a leading `Self` formal if present, then always prepend the
            // concrete receiver so call sites stay uniform (TYP.2).
            let payload = if params
                .first()
                .is_some_and(|&p| is_self_type_formal(checker, p))
            {
                params[1..].to_vec()
            } else {
                params
            };
            let mut new_params = Vec::with_capacity(payload.len() + 1);
            new_params.push(actual_base_ty_id);
            new_params.extend(payload);
            (new_params, ret, None)
        } else {
            return None;
        }
    } else if let Some(sym) = method_sym {
        let method_ty = checker.decl_type(sym)?;
        let (params, ret) = match &method_ty {
            ArType::Func(params, ret) => (params.clone(), *ret),
            _ => return None,
        };
        (params, ret, Some(sym))
    } else {
        // Root fix: missing method (including private methods not present in
        // the import export table) must diagnose here. Returning `None` let the
        // Call path fall through without a reliable diagnostic.
        let interner = &checker.type_info.type_interner;
        let ty_disp = base_resolved.display(&checker.symbols, interner);
        checker.diagnostics.push(
            crate::Diagnostic::error(
                crate::DiagCode::T018UndefinedField,
                format!("no method `{method}` on type `{ty_disp}`"),
                field_span,
            )
            .with_label(
                checker.pool.expr_span(base),
                format!("receiver has type `{ty_disp}`"),
            )
            .with_note(
                "if this is a method from another module, ensure it is declared `public`"
                    .to_string(),
            ),
        );
        return Some(checker.intern(ArType::Error));
    };

    // Instantiate template method type with the receiver's concrete type args
    // so `BoxG<int>.get` sees `Func([BoxG<int>], int)` not `Func([BoxG<T>], T)`.
    // For Result/Option, substitute T/E from the builtin type shape.
    let (params, ret) = if let Some(sid) = struct_id {
        instantiate_method_sig_for_receiver(
            checker,
            sid,
            actual_base_ty_id,
            params,
            ret,
            method_sym_recorded,
        )
    } else {
        instantiate_method_sig_for_result_option(
            checker,
            actual_base_ty_id,
            params,
            ret,
            method_sym_recorded,
        )
    };

    if params.is_empty() {
        return None;
    }

    let receiver_ty_id = params[0];
    let receiver_ok = checker.unify_ids(receiver_ty_id, actual_base_ty_id)
        || match checker.resolve(receiver_ty_id) {
            // Auto-ref: method/`self` formal is `&T`/`&mut T`, receiver is `T`.
            ArType::Ref(inner) | ArType::RefMut(inner) => {
                checker.unify_ids(inner, actual_base_ty_id)
            }
            _ => match checker.resolve(actual_base_ty_id) {
                // Auto-deref: formal `T`, receiver is `&T`/`&mut T`.
                ArType::Ref(inner) | ArType::RefMut(inner) => {
                    checker.unify_ids(receiver_ty_id, inner)
                }
                _ => false,
            },
        };
    if !receiver_ok {
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
        let expected_id = explicit_params.get(i).copied();
        let arg_ty_id = super::expr::synth_expr_expected(checker, arg_id, expected_id);
        if let Some(expected_id) = expected_id {
            super::expr::check_call_arg(
                checker,
                expected_id,
                arg_ty_id,
                call_span,
                field_span,
                checker.pool.expr_span(arg_id),
                i + 1,
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

/// Instantiate `Result.expectOrAbort` / `Option.expectOrAbort` from a builtin
/// `ArType::Result` / `Option` receiver (not a Named type).
fn instantiate_method_sig_for_result_option(
    checker: &mut TypeChecker<'_>,
    actual_base_ty_id: TypeId,
    params: Vec<TypeId>,
    ret: TypeId,
    method_sym: Option<arandu_middle::SymbolId>,
) -> (Vec<TypeId>, TypeId) {
    use crate::type_checker::types::{build_subst, substitute_type};

    let concrete_args: Vec<ArType> = match checker.resolve(actual_base_ty_id) {
        ArType::Result(ok, err) => vec![checker.resolve(ok), checker.resolve(err)],
        ArType::Option(inner) => vec![checker.resolve(inner)],
        _ => return (params, ret),
    };

    let Some(sym) = method_sym else {
        // No generic map: still force receiver to the concrete Result/Option type.
        if params.is_empty() {
            return (params, ret);
        }
        let mut new_params = params;
        new_params[0] = actual_base_ty_id;
        return (new_params, ret);
    };

    let Some(gp) = checker.type_info.generic_params.get(&sym).cloned() else {
        if params.is_empty() {
            return (params, ret);
        }
        let mut new_params = params;
        new_params[0] = actual_base_ty_id;
        return (new_params, ret);
    };

    let n = gp.len().min(concrete_args.len());
    if n == 0 {
        return (params, ret);
    }
    let subst = build_subst(&gp[..n], &concrete_args[..n]);
    let new_params: Vec<TypeId> = params
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            if i == 0 {
                return actual_base_ty_id;
            }
            let ty = checker.resolve(p);
            let inst = substitute_type(&ty, &subst, &checker.type_info.type_interner);
            checker.intern(inst)
        })
        .collect();
    let ret_ty = checker.resolve(ret);
    let new_ret = checker.intern(substitute_type(
        &ret_ty,
        &subst,
        &checker.type_info.type_interner,
    ));
    (new_params, new_ret)
}

/// Substitute struct type parameters in a method signature using the concrete
/// receiver type (`BoxG<int>` → replace `T` with `int` in params/return).
fn instantiate_method_sig_for_receiver(
    checker: &mut TypeChecker<'_>,
    struct_id: arandu_middle::SymbolId,
    actual_base_ty_id: TypeId,
    params: Vec<TypeId>,
    ret: TypeId,
    method_sym: Option<arandu_middle::SymbolId>,
) -> (Vec<TypeId>, TypeId) {
    use crate::type_checker::types::{build_subst, substitute_type};

    // Peel ptr/ref so specialization works when the receiver is already a ref
    // (or when only the formal is `&T` after auto-ref at the call site).
    let mut base_id = actual_base_ty_id;
    for _ in 0..4 {
        match checker.resolve(base_id) {
            ArType::Ptr(inner) | ArType::Ref(inner) | ArType::RefMut(inner) => {
                base_id = inner;
            }
            _ => break,
        }
    }
    let recv_args: Vec<ArType> = match checker.resolve(base_id) {
        ArType::Named(id, args) if id == struct_id => {
            args.iter().map(|&a| checker.resolve(a)).collect()
        }
        _ => return (params, ret),
    };
    if recv_args.is_empty() {
        return (params, ret);
    }

    // Prefer method-level generic_params prefix (struct params first), else struct params.
    let param_syms: Vec<arandu_middle::SymbolId> = if let Some(sym) = method_sym
        && let Some(gp) = checker.type_info.generic_params.get(&sym)
    {
        let n = recv_args.len().min(gp.len());
        gp.iter().copied().take(n).collect()
    } else if let Some(gp) = checker.type_info.generic_params.get(&struct_id) {
        gp.iter().copied().take(recv_args.len()).collect()
    } else {
        return (params, ret);
    };
    if param_syms.len() != recv_args.len() {
        return (params, ret);
    }

    let subst = build_subst(&param_syms, &recv_args);
    let new_params: Vec<TypeId> = params
        .iter()
        .map(|&pid| {
            let ty = checker.resolve(pid);
            let inst = substitute_type(&ty, &subst, &checker.type_info.type_interner);
            checker.intern(inst)
        })
        .collect();
    let ret_ty = checker.resolve(ret);
    let ret_inst = substitute_type(&ret_ty, &subst, &checker.type_info.type_interner);
    let new_ret = checker.intern(ret_inst);
    (new_params, new_ret)
}

/// True when a formal is the interface receiver type `Self` (or a ref to it).
fn is_self_type_formal(checker: &TypeChecker<'_>, tid: TypeId) -> bool {
    match checker.resolve(tid) {
        ArType::Named(id, _) => checker.symbols.get(id).name == "Self",
        ArType::Ref(inner) | ArType::RefMut(inner) => is_self_type_formal(checker, inner),
        _ => false,
    }
}
