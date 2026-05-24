use arandu_parser::{BinaryOp, CatchHandler, Expr, UnaryOp};
use arandu_lexer::Span;

use super::super::TypeChecker;
use super::super::constraints::ConstraintOrigin;
use super::super::types::{ArType, Primitive};
use super::pattern::check_pattern;
use super::{resolve_field, resolve_index, resolve_namespace_field, resolve_namespace_member_type};
use super::{synth_method_call, synth_option_ctor, synth_result_ctor};

/// Stricter than `unify` for array literals: int and float literals must not mix.
fn array_element_types_compatible(a: &ArType, b: &ArType) -> bool {
    if matches!(
        (a, b),
        (ArType::IntLiteral, ArType::FloatLiteral) | (ArType::FloatLiteral, ArType::IntLiteral)
    ) {
        return false;
    }
    super::super::types::unify(a, b)
}

fn report_unsupported(checker: &mut TypeChecker, span: Span, feature: &str) {
    checker.diagnostics.push(crate::Diagnostic::error(
        crate::DiagCode::L002AmirUnsupportedFeature,
        format!("{feature} is not supported yet"),
        span,
    ));
}

pub fn synth_expr(checker: &mut TypeChecker, expr: &Expr) -> ArType {
    let ty = synth_expr_inner(checker, expr);
    checker
        .type_info
        .expr_types
        .insert(crate::NodeKey::from(expr.span()), ty.clone());
    ty
}

fn synth_expr_inner(checker: &mut TypeChecker, expr: &Expr) -> ArType {
    match expr {
        Expr::Int { .. } => ArType::IntLiteral,
        Expr::Float { .. } => ArType::FloatLiteral,
        Expr::Bool { .. } => ArType::Primitive(Primitive::Bool),
        Expr::Char { .. } => ArType::Primitive(Primitive::Char),
        Expr::InterpolatedString { parts, .. } => {
            for part in parts {
                if let arandu_parser::StringPart::Expr { expr, .. } = part {
                    let _ = synth_expr(checker, expr);
                }
            }
            ArType::Primitive(Primitive::Str)
        }
        Expr::Nil { .. } => {
            if let Some(ret) = checker.ctx.current_return() {
                if super::super::types::is_result_type(ret) {
                    return ret.clone();
                }
                if let Some((ok, _)) = super::super::types::result_ok_err(ret) {
                    return ok;
                }
                return ret.clone();
            }
            ArType::Nullable(Box::new(ArType::Error))
        }
        Expr::Path { span, .. } => {
            let key = crate::NodeKey::from(*span);
            if let Some(symbol_id) = checker.resolved.value_refs.get(&key) {
                if let Some(ty) = checker.ctx.lookup(*symbol_id) {
                    return ty.clone();
                }
                if let Some(ty) = checker.type_info.decl_types.get(symbol_id) {
                    return ty.clone();
                }
            }
            ArType::Error
        }
        Expr::TypePath {
            span,
            type_name,
            member,
        } => {
            if super::super::types::type_name_base(type_name) == "Result" {
                return match member.as_str() {
                    "Ok" => ArType::Func(
                        vec![ArType::Error],
                        Box::new(ArType::Result(
                            Box::new(ArType::Error),
                            Box::new(ArType::Err),
                        )),
                    ),
                    "Err" => ArType::Func(
                        vec![ArType::Error],
                        Box::new(ArType::Result(
                            Box::new(ArType::Error),
                            Box::new(ArType::Err),
                        )),
                    ),
                    _ => ArType::Error,
                };
            }
            if super::super::types::type_name_base(type_name) == "Option" {
                return match member.as_str() {
                    "Some" => ArType::Func(
                        vec![ArType::Error],
                        Box::new(ArType::Option(Box::new(ArType::Error))),
                    ),
                    "None" => ArType::Option(Box::new(ArType::Error)),
                    _ => ArType::Error,
                };
            }

            let type_key = crate::NodeKey::from(type_name.span);
            if let Some(enum_symbol_id) = checker.resolved.type_refs.get(&type_key) {
                let variant_symbol_opt = checker
                    .symbols
                    .lookup_associated_member(&type_name.path.join("."), member);
                if let Some(variant_symbol_id) = variant_symbol_opt
                    && let Some((_, shape)) =
                        checker.type_info.enum_variants.get(&variant_symbol_id)
                {
                    let enum_ty = ArType::Named(*enum_symbol_id, vec![]);
                    match shape {
                        super::super::EnumPayloadShape::Unit => {
                            return enum_ty;
                        }
                        super::super::EnumPayloadShape::Tuple(tys) => {
                            return ArType::Func(tys.clone(), Box::new(enum_ty));
                        }
                    }
                }
            }

            let key = crate::NodeKey::from(*span);
            if let Some(symbol_id) = checker.resolved.value_refs.get(&key)
                && let Some(ty) = checker.type_info.decl_types.get(symbol_id)
            {
                return ty.clone();
            }
            ArType::Error
        }
        Expr::Generic {
            callee,
            args,
            span,
        } => super::super::types::synth_generic_instantiation(checker, callee, args, *span),
        Expr::Field { span, base, field } => {
            if let Some(ty) = resolve_namespace_field(checker, base, field, *span) {
                ty
            } else if let Some(ty) = resolve_namespace_member_type(checker, *span) {
                ty
            } else {
                resolve_field(checker, base, field, *span, false)
            }
        }
        Expr::SafeField { span, base, field } => {
            if let Some(ty) = resolve_namespace_field(checker, base, field, *span) {
                ty
            } else if let Some(ty) = resolve_namespace_member_type(checker, *span) {
                ty
            } else {
                resolve_field(checker, base, field, *span, true)
            }
        }
        Expr::Index {
            span: _,
            base,
            index,
        } => resolve_index(checker, base, index, false),
        Expr::SafeIndex {
            span: _,
            base,
            index,
        } => resolve_index(checker, base, index, true),
        Expr::Try { span, expr } => {
            let inner_ty = synth_expr(checker, expr);
            if let Some(ok_ty) = super::super::types::try_ok_type(&inner_ty) {
                ok_ty
            } else if inner_ty.is_error() {
                ArType::Error
            } else {
                checker.add_constraint(
                    ArType::Error,
                    inner_ty,
                    ConstraintOrigin::TryInvalid { span: *span },
                );
                ArType::Error
            }
        }
        Expr::Call {
            span,
            callee,
            args,
            trailing_block: _,
        } => {
            if let Some(result_ty) = synth_result_ctor(checker, callee, args, *span) {
                return result_ty;
            }
            if let Some(option_ty) = synth_option_ctor(checker, callee, args, *span) {
                return option_ty;
            }
            if let Expr::Field {
                span: field_span,
                base,
                field,
            } = &**callee
                && let Some(ret) = synth_method_call(checker, base, field, *field_span, args, *span)
            {
                return ret;
            }

            let callee_ty = synth_expr(checker, callee);
            match callee_ty {
                ArType::Func(params, ret) => {
                    if params.len() != args.len() {
                        let diag = crate::Diagnostic::error(
                            crate::DiagCode::T012WrongArgCount,
                            format!("expected {} arguments, found {}", params.len(), args.len()),
                            *span,
                        )
                        .with_label(callee.span(), "call target is here")
                        .with_label(*span, format!("{} arguments provided", args.len()));
                        checker.diagnostics.push(diag);
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let arg_ty = synth_expr(checker, arg);
                        if i < params.len() {
                            if !super::super::types::unify(&params[i], &arg_ty) {
                                checker.add_constraint(
                                    params[i].clone(),
                                    arg_ty,
                                    ConstraintOrigin::CallArg {
                                        call_span: *span,
                                        param_span: callee.span(),
                                        arg_span: arg.span(),
                                        arg_index: i,
                                    },
                                );
                            } else if !arg_ty.is_literal()
                                && arg_ty != params[i]
                                && params[i].is_numeric()
                                && arg_ty.is_numeric()
                            {
                                checker.add_constraint(
                                    params[i].clone(),
                                    arg_ty,
                                    ConstraintOrigin::ImplicitWidening {
                                        source_span: arg.span(),
                                        target_span: *span,
                                    },
                                );
                            }
                        }
                    }
                    *ret
                }
                ArType::Error => ArType::Error,
                other => {
                    let diag = crate::Diagnostic::error(
                        crate::DiagCode::T003IncompatibleCallArg,
                        format!(
                            "cannot call non-function type '{}'",
                            other.display(&checker.symbols)
                        ),
                        *span,
                    )
                    .with_label(
                        callee.span(),
                        format!("type is '{}'", other.display(&checker.symbols)),
                    )
                    .with_label(*span, "call site");
                    checker.diagnostics.push(diag);
                    ArType::Error
                }
            }
        }
        Expr::StructLiteral { span, ty, fields } => {
            let struct_ty = super::super::types::lower_type_expr(
                ty,
                &checker.symbols,
                checker.type_scope(),
                &checker.resolved,
            );
            if let ArType::Named(symbol_id, generic_args) = &struct_ty {
                let field_map = super::super::types::struct_fields_instantiated(
                    checker,
                    *symbol_id,
                    generic_args,
                )
                .or_else(|| {
                    checker
                        .type_info
                        .struct_fields
                        .get(symbol_id)
                        .cloned()
                });
                if let Some(fields_def) = field_map {
                    for field in fields {
                        let field_val_ty = synth_expr(checker, &field.value);
                        let defined_field_ty = fields_def.get(&field.name).cloned();
                        if let Some(defined_field_ty) = defined_field_ty {
                            if !super::super::types::unify(&defined_field_ty, &field_val_ty) {
                                checker.add_constraint(
                                    defined_field_ty,
                                    field_val_ty,
                                    ConstraintOrigin::FieldInit {
                                        struct_span: *span,
                                        field_name: field.name.clone(),
                                        field_span: field.span,
                                        value_span: field.value.span(),
                                    },
                                );
                            }
                        } else {
                            checker.add_constraint(
                                struct_ty.clone(),
                                ArType::Error,
                                ConstraintOrigin::UndefinedField {
                                    base_span: ty.span(),
                                    field_span: field.span,
                                    field_name: field.name.clone(),
                                },
                            );
                        }
                    }
                } else {
                    for field in fields {
                        let _ = synth_expr(checker, &field.value);
                    }
                }
            } else {
                for field in fields {
                    let _ = synth_expr(checker, &field.value);
                }
            }
            struct_ty
        }
        Expr::Array { span, items } => {
            let mut elem_ty = ArType::Error;
            for (i, item) in items.iter().enumerate() {
                let item_ty = synth_expr(checker, item);
                if elem_ty.is_error() {
                    elem_ty = item_ty;
                } else if !array_element_types_compatible(&elem_ty, &item_ty) {
                    checker.add_constraint(
                        elem_ty.clone(),
                        item_ty,
                        ConstraintOrigin::ArrayLiteral {
                            array_span: *span,
                            item_span: item.span(),
                            item_index: i,
                        },
                    );
                    elem_ty = ArType::Error;
                }
            }
            ArType::Array(items.len() as u64, Box::new(elem_ty))
        }
        Expr::Lambda { span, .. } => {
            report_unsupported(checker, *span, "lambda/closure");
            ArType::Error
        }
        Expr::Alloc { span: _, expr } => {
            let inner_ty = synth_expr(checker, expr);
            ArType::Ptr(Box::new(inner_ty))
        }
        Expr::AsyncBlock { span: _, block } => super::super::check::check_block(checker, block),
        Expr::UnsafeBlock { span: _, block } => super::super::check::check_block(checker, block),
        Expr::If {
            span: _,
            condition,
            then_block,
            else_block,
        } => {
            super::super::check::check_condition(checker, condition);
            let then_ty = super::super::check::check_block(checker, then_block);
            let else_ty = super::super::check::check_block(checker, else_block);
            if !super::super::types::unify(&then_ty, &else_ty) {
                checker.add_constraint(
                    then_ty.clone(),
                    else_ty.clone(),
                    ConstraintOrigin::IfBranches {
                        then_span: then_block.span,
                        else_span: else_block.span,
                    },
                );
            }
            then_ty
        }
        Expr::Match { span, value, arms } => {
            let value_ty = synth_expr(checker, value);
            super::match_exhaust::check_match_exhaustiveness(checker, &value_ty, arms, *span);
            let mut expected_arm_ty = ArType::Error;
            let mut first_arm_span = *span;

            for (i, arm) in arms.iter().enumerate() {
                check_pattern(checker, &arm.pattern, &value_ty);
                let arm_ty = match &arm.body {
                    arandu_parser::MatchArmBody::Expr { expr, .. } => synth_expr(checker, expr),
                    arandu_parser::MatchArmBody::Block { block, .. } => {
                        super::super::check::check_block(checker, block)
                    }
                };

                if i == 0 {
                    expected_arm_ty = arm_ty;
                    first_arm_span = arm.span;
                } else if !super::super::types::unify(&expected_arm_ty, &arm_ty) {
                    checker.add_constraint(
                        expected_arm_ty.clone(),
                        arm_ty.clone(),
                        ConstraintOrigin::MatchArms {
                            first_span: first_arm_span,
                            mismatch_span: arm.span,
                            arm_index: i,
                        },
                    );
                }
            }
            expected_arm_ty
        }
        Expr::Catch {
            span,
            expr,
            handler,
        } => {
            let inner_ty = synth_expr(checker, expr);
            let handler_ty = match handler {
                CatchHandler::Expr {
                    expr: h,
                    span: h_span,
                } => {
                    let ty = synth_expr(checker, h);
                    (*h_span, ty)
                }
                CatchHandler::Block {
                    block,
                    span: h_span,
                    ..
                } => {
                    let ty = super::super::check::check_block(checker, block);
                    (*h_span, ty)
                }
            };
            if let Some((ok_ty, _)) = super::super::types::result_ok_err(&inner_ty) {
                if !super::super::types::unify(&ok_ty, &handler_ty.1) {
                    checker.add_constraint(
                        ok_ty.clone(),
                        handler_ty.1.clone(),
                        ConstraintOrigin::CatchHandler {
                            expr_span: expr.span(),
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
                            inner_ty.display(&checker.symbols)
                        ),
                        *span,
                    )
                    .with_label(expr.span(), "expression is not a Result"),
                );
                ArType::Error
            }
        }
        Expr::NullCoalesce { span, left, right } => {
            let left_ty = synth_expr(checker, left);
            let right_ty = synth_expr(checker, right);
            match &left_ty {
                ArType::Nullable(inner) => {
                    if !super::super::types::unify(inner, &right_ty) {
                        checker.add_constraint(
                            inner.as_ref().clone(),
                            right_ty.clone(),
                            ConstraintOrigin::NullCoalesce {
                                left_span: left.span(),
                                right_span: right.span(),
                            },
                        );
                    }
                    right_ty
                }
                ArType::Error => right_ty,
                other => {
                    checker.diagnostics.push(
                        crate::Diagnostic::error(
                            crate::DiagCode::T006NotNullable,
                            format!(
                                "operator `??` requires a nullable left-hand side, found '{}'",
                                other.display(&checker.symbols)
                            ),
                            *span,
                        )
                        .with_label(
                            left.span(),
                            format!("type is '{}'", other.display(&checker.symbols)),
                        )
                        .with_hint(
                            "use a nullable value on the left or make it nullable".to_string(),
                        ),
                    );
                    right_ty
                }
            }
        }
        Expr::Cast { span: _, expr, ty } => {
            let _found_ty = synth_expr(checker, expr);
            let target_ty = super::super::types::lower_type_expr(
                ty,
                &checker.symbols,
                checker.type_scope(),
                &checker.resolved,
            );
            // Basic cast validation logic goes here
            target_ty
        }
        Expr::Group { span: _, expr } => synth_expr(checker, expr),
        Expr::Unary { span, op, expr } => {
            let expr_ty = synth_expr(checker, expr);
            if expr_ty.is_error() {
                return ArType::Error;
            }
            match op {
                UnaryOp::Neg => {
                    if expr_ty.is_numeric() {
                        expr_ty
                    } else {
                        checker.add_constraint(
                            ArType::Primitive(Primitive::Int),
                            expr_ty.clone(),
                            ConstraintOrigin::UnaryOp {
                                op_span: *span,
                                operand_span: expr.span(),
                            },
                        );
                        ArType::Error
                    }
                }
                UnaryOp::Not => {
                    if super::super::types::unify(&expr_ty, &ArType::Primitive(Primitive::Bool)) {
                        ArType::Primitive(Primitive::Bool)
                    } else {
                        checker.add_constraint(
                            ArType::Primitive(Primitive::Bool),
                            expr_ty.clone(),
                            ConstraintOrigin::UnaryOp {
                                op_span: *span,
                                operand_span: expr.span(),
                            },
                        );
                        ArType::Error
                    }
                }
                UnaryOp::BitNot => {
                    if expr_ty.is_integer() {
                        expr_ty
                    } else {
                        checker.add_constraint(
                            ArType::Primitive(Primitive::Int),
                            expr_ty.clone(),
                            ConstraintOrigin::UnaryOp {
                                op_span: *span,
                                operand_span: expr.span(),
                            },
                        );
                        ArType::Error
                    }
                }
                UnaryOp::Await => {
                    report_unsupported(checker, *span, "await");
                    ArType::Error
                }
            }
        }
        Expr::Binary {
            span,
            op,
            left,
            right,
        } => {
            let left_ty = synth_expr(checker, left);
            let right_ty = synth_expr(checker, right);

            if left_ty.is_error() || right_ty.is_error() {
                return ArType::Error;
            }

            match op {
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                    if !super::super::types::unify(&left_ty, &right_ty)
                        || (!left_ty.is_numeric() && !right_ty.is_numeric())
                    {
                        checker.add_constraint(
                            left_ty.clone(),
                            right_ty.clone(),
                            ConstraintOrigin::BinaryOp {
                                op_span: *span,
                                left_span: left.span(),
                                right_span: right.span(),
                            },
                        );
                        return ArType::Error;
                    }
                    super::super::types::resolve_literal_pair(&left_ty, &right_ty)
                }
                BinaryOp::Equal
                | BinaryOp::NotEqual
                | BinaryOp::Lt
                | BinaryOp::Gt
                | BinaryOp::LtEqual
                | BinaryOp::GtEqual => {
                    if !super::super::types::unify(&left_ty, &right_ty) {
                        checker.add_constraint(
                            left_ty.clone(),
                            right_ty.clone(),
                            ConstraintOrigin::BinaryOp {
                                op_span: *span,
                                left_span: left.span(),
                                right_span: right.span(),
                            },
                        );
                    }
                    ArType::Primitive(Primitive::Bool)
                }
                _ => ArType::Error,
            }
        }
        Expr::Error(_) => ArType::Error,
    }
}
