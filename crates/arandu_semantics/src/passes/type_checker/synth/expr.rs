use arandu_lexer::Span;
use arandu_parser::ast_pool::{ExprId, ExprKind};
use arandu_parser::{BinaryOp, CatchHandler, UnaryOp};

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

fn cast_types_compatible(found: &ArType, target: &ArType) -> bool {
    if found.is_error() || target.is_error() {
        return true;
    }
    if super::super::types::unify(found, target) {
        return true;
    }
    found.is_numeric() && target.is_numeric()
}

fn report_unsupported(checker: &mut TypeChecker<'_>, span: Span, feature: &str, roadmap: &str) {
    checker.diagnostics.push(
        crate::Diagnostic::error(
            crate::DiagCode::L002AmirUnsupportedFeature,
            format!("{feature} is not supported yet ({roadmap})"),
            span,
        )
        .with_hint("see docs/arandu-compiler-roadmap-v0.1.md for the planned milestone"),
    );
}

pub fn synth_expr(checker: &mut TypeChecker<'_>, expr: ExprId) -> ArType {
    let ty = synth_expr_inner(checker, expr);
    checker.record_expr_type(expr, ty.clone());
    ty
}

fn synth_expr_inner(checker: &mut TypeChecker<'_>, expr: ExprId) -> ArType {
    let span = checker.pool.expr_span(expr);
    match checker.pool.expr(expr) {
        ExprKind::Int { .. } => ArType::IntLiteral,
        ExprKind::Float { .. } => ArType::FloatLiteral,
        ExprKind::Bool { .. } => ArType::Primitive(Primitive::Bool),
        ExprKind::Char { .. } => ArType::Primitive(Primitive::Char),
        ExprKind::InterpolatedString { parts } => {
            let part_ids = checker.pool.string_part_list(*parts).to_vec();
            for part_id in part_ids {
                if let arandu_parser::StringPart::Expr {
                    expr: inner_expr, ..
                } = checker.pool.string_part(part_id)
                {
                    let _ = synth_expr(checker, *inner_expr);
                }
            }
            ArType::Primitive(Primitive::Str)
        }
        ExprKind::Nil => {
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
        ExprKind::Path { path: _ } => {
            if let Some(symbol_id) = checker.resolved.expr_symbol(expr) {
                if let Some(ty) = checker.ctx.lookup(symbol_id) {
                    return ty.clone();
                }
                if let Some(ty) = checker.decl_type(symbol_id) {
                    return ty;
                }
            }
            ArType::Error
        }
        ExprKind::TypePath { type_name, member } => {
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

            if let Some(symbol_id) = checker.resolved.expr_symbol(expr)
                && let Some(ty) = checker.decl_type(symbol_id)
            {
                return ty;
            }
            ArType::Error
        }
        ExprKind::Generic { callee, args } => {
            let callee_id = *callee;
            let args_range = *args;
            super::super::types::synth_generic_instantiation(checker, callee_id, args_range, span)
        }
        ExprKind::Field { base, field } => {
            let base_id = *base;
            let field_str = field.clone();
            if let Some(ty) = resolve_namespace_field(checker, base_id, expr, &field_str, span) {
                ty
            } else if let Some(ty) = resolve_namespace_member_type(checker, expr) {
                ty
            } else {
                resolve_field(checker, base_id, &field_str, span, false)
            }
        }
        ExprKind::SafeField { base, field } => {
            let base_id = *base;
            let field_str = field.clone();
            if let Some(ty) = resolve_namespace_field(checker, base_id, expr, &field_str, span) {
                ty
            } else if let Some(ty) = resolve_namespace_member_type(checker, expr) {
                ty
            } else {
                resolve_field(checker, base_id, &field_str, span, true)
            }
        }
        ExprKind::Index { base, index } => {
            let base_id = *base;
            let index_id = *index;
            resolve_index(checker, base_id, index_id, false)
        }
        ExprKind::SafeIndex { base, index } => {
            let base_id = *base;
            let index_id = *index;
            resolve_index(checker, base_id, index_id, true)
        }
        ExprKind::Try { expr: inner_expr } => {
            let inner_id = *inner_expr;
            let inner_ty = synth_expr(checker, inner_id);
            if let Some(ok_ty) = super::super::types::try_ok_type(&inner_ty) {
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
            }
        }
        ExprKind::Call {
            callee,
            args,
            trailing_block: _,
        } => {
            let callee_id = *callee;
            let args_range = *args;
            if let Some(result_ty) = synth_result_ctor(checker, callee_id, args_range, span) {
                return result_ty;
            }
            if let Some(option_ty) = synth_option_ctor(checker, callee_id, args_range, span) {
                return option_ty;
            }
            if let ExprKind::Field { base, field } = checker.pool.expr(callee_id) {
                let base_id = *base;
                let field_str = field.clone();
                let field_span = checker.pool.expr_span(callee_id);
                if let Some(ret) = synth_method_call(
                    checker, base_id, callee_id, &field_str, field_span, args_range, span,
                ) {
                    return ret;
                }
            }

            let callee_ty = synth_expr(checker, callee_id);
            let arg_ids = checker.pool.expr_list(args_range).to_vec();
            match callee_ty {
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
                            if !super::super::types::unify(&params[i], &arg_ty) {
                                checker.add_constraint(
                                    params[i].clone(),
                                    arg_ty,
                                    ConstraintOrigin::CallArg {
                                        call_span: span,
                                        param_span: checker.pool.expr_span(callee_id),
                                        arg_span: checker.pool.expr_span(arg_id),
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
                                        source_span: checker.pool.expr_span(arg_id),
                                        target_span: span,
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
                        span,
                    )
                    .with_label(
                        checker.pool.expr_span(callee_id),
                        format!("type is '{}'", other.display(&checker.symbols)),
                    )
                    .with_label(span, "call site");
                    checker.diagnostics.push(diag);
                    ArType::Error
                }
            }
        }
        ExprKind::StructLiteral { ty, fields } => {
            let ty_id = *ty;
            let fields_range = *fields;
            let struct_ty = super::super::types::lower_type_expr(
                checker.pool.type_expr(ty_id),
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
                .or_else(|| checker.type_info.struct_fields.get(symbol_id).cloned());
                let field_ids = checker.pool.field_init_list(fields_range).to_vec();
                if let Some(fields_def) = field_map {
                    for fid in field_ids {
                        let field = checker.pool.field_init(fid);
                        let field_val_ty = synth_expr(checker, field.value);
                        let defined_field_ty = fields_def.get(&field.name).cloned();
                        if let Some(defined_field_ty) = defined_field_ty {
                            if !super::super::types::unify(&defined_field_ty, &field_val_ty) {
                                checker.add_constraint(
                                    defined_field_ty,
                                    field_val_ty,
                                    ConstraintOrigin::FieldInit {
                                        struct_span: span,
                                        field_name: field.name.clone(),
                                        field_span: field.span,
                                        value_span: checker.pool.expr_span(field.value),
                                    },
                                );
                            }
                        } else {
                            checker.add_constraint(
                                struct_ty.clone(),
                                ArType::Error,
                                ConstraintOrigin::UndefinedField {
                                    base_span: checker.pool.type_expr_span(ty_id),
                                    field_span: field.span,
                                    field_name: field.name.clone(),
                                },
                            );
                        }
                    }
                } else {
                    for fid in field_ids {
                        let field = checker.pool.field_init(fid);
                        let _ = synth_expr(checker, field.value);
                    }
                }
            } else {
                let field_ids = checker.pool.field_init_list(fields_range).to_vec();
                for fid in field_ids {
                    let field = checker.pool.field_init(fid);
                    let _ = synth_expr(checker, field.value);
                }
            }
            struct_ty
        }
        ExprKind::Array { items } => {
            let items_range = *items;
            let mut elem_ty = ArType::Error;
            let item_ids = checker.pool.expr_list(items_range).to_vec();
            for (i, item_id) in item_ids.iter().copied().enumerate() {
                let item_ty = synth_expr(checker, item_id);
                if elem_ty.is_error() {
                    elem_ty = item_ty;
                } else if !array_element_types_compatible(&elem_ty, &item_ty) {
                    checker.add_constraint(
                        elem_ty.clone(),
                        item_ty,
                        ConstraintOrigin::ArrayLiteral {
                            array_span: span,
                            item_span: checker.pool.expr_span(item_id),
                            item_index: i,
                        },
                    );
                    elem_ty = ArType::Error;
                }
            }
            ArType::Array(items_range.len as u64, Box::new(elem_ty))
        }
        ExprKind::Lambda { .. } => {
            report_unsupported(
                checker,
                span,
                "lambda/closure",
                "v0.3 LAMBDA: closure type checking and lowering",
            );
            ArType::Error
        }
        ExprKind::Alloc { expr: inner_expr } => {
            let inner_id = *inner_expr;
            let inner_ty = synth_expr(checker, inner_id);
            ArType::Ptr(Box::new(inner_ty))
        }
        ExprKind::AsyncBlock { block } => {
            let block_id = *block;
            super::super::check::check_block(checker, checker.pool, checker.pool.block(block_id))
        }
        ExprKind::UnsafeBlock { block } => {
            let block_id = *block;
            super::super::check::check_block(checker, checker.pool, checker.pool.block(block_id))
        }
        ExprKind::If {
            condition,
            then_block,
            else_block,
        } => {
            let cond = condition.clone();
            let then_id = *then_block;
            let else_id = *else_block;
            super::super::check::check_condition(checker, &cond);
            let then_ty = super::super::check::check_block(
                checker,
                checker.pool,
                checker.pool.block(then_id),
            );
            let else_ty = super::super::check::check_block(
                checker,
                checker.pool,
                checker.pool.block(else_id),
            );
            if !super::super::types::unify(&then_ty, &else_ty) {
                checker.add_constraint(
                    then_ty.clone(),
                    else_ty.clone(),
                    ConstraintOrigin::IfBranches {
                        then_span: checker.pool.block(then_id).span,
                        else_span: checker.pool.block(else_id).span,
                    },
                );
            }
            then_ty
        }
        ExprKind::Match { value, arms } => {
            let value_id = *value;
            let arms_range = *arms;
            let value_ty = synth_expr(checker, value_id);
            let arm_ids = checker.pool.match_arm_list(arms_range).to_vec();

            let resolved_arms: Vec<arandu_parser::MatchArm> = arm_ids
                .iter()
                .map(|id| checker.pool.match_arm(*id).clone())
                .collect();
            super::match_exhaust::check_match_exhaustiveness(
                checker,
                &value_ty,
                &resolved_arms,
                span,
            );

            let mut expected_arm_ty = ArType::Error;
            let mut first_arm_span = span;

            for (i, arm_id) in arm_ids.iter().copied().enumerate() {
                let arm = checker.pool.match_arm(arm_id);
                check_pattern(checker, &arm.pattern, &value_ty);
                let arm_ty = match &arm.body {
                    arandu_parser::MatchArmBody::Expr {
                        expr: inner_expr, ..
                    } => synth_expr(checker, **inner_expr),
                    arandu_parser::MatchArmBody::Block { block, .. } => {
                        super::super::check::check_block(checker, checker.pool, block)
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
                    let ty = super::super::check::check_block(checker, checker.pool, block);
                    (*h_span, ty)
                }
            };
            if let Some((ok_ty, _)) = super::super::types::result_ok_err(&inner_ty) {
                if !super::super::types::unify(&ok_ty, &handler_ty.1) {
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
                            inner_ty.display(&checker.symbols)
                        ),
                        span,
                    )
                    .with_label(
                        checker.pool.expr_span(inner_id),
                        "expression is not a Result",
                    ),
                );
                ArType::Error
            }
        }
        ExprKind::NullCoalesce { left, right } => {
            let left_id = *left;
            let right_id = *right;
            let left_ty = synth_expr(checker, left_id);
            let right_ty = synth_expr(checker, right_id);
            match &left_ty {
                ArType::Nullable(inner) => {
                    if !super::super::types::unify(inner, &right_ty) {
                        checker.add_constraint(
                            inner.as_ref().clone(),
                            right_ty.clone(),
                            ConstraintOrigin::NullCoalesce {
                                left_span: checker.pool.expr_span(left_id),
                                right_span: checker.pool.expr_span(right_id),
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
                            span,
                        )
                        .with_label(
                            checker.pool.expr_span(left_id),
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
        ExprKind::Cast {
            expr: inner_expr,
            ty,
        } => {
            let inner_id = *inner_expr;
            let ty_id = *ty;
            let found_ty = synth_expr(checker, inner_id);
            let target_ty = super::super::types::lower_type_expr(
                checker.pool.type_expr(ty_id),
                &checker.symbols,
                checker.type_scope(),
                &checker.resolved,
            );
            if !cast_types_compatible(&found_ty, &target_ty) {
                checker.add_constraint(
                    target_ty.clone(),
                    found_ty,
                    ConstraintOrigin::CastExpr {
                        expr_span: checker.pool.expr_span(inner_id),
                        target_span: checker.pool.type_expr_span(ty_id),
                    },
                );
            }
            target_ty
        }
        ExprKind::Group { expr: inner_expr } => synth_expr(checker, *inner_expr),
        ExprKind::Unary {
            op,
            expr: inner_expr,
        } => {
            let inner_id = *inner_expr;
            let expr_ty = synth_expr(checker, inner_id);
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
                                op_span: span,
                                operand_span: checker.pool.expr_span(inner_id),
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
                                op_span: span,
                                operand_span: checker.pool.expr_span(inner_id),
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
                                op_span: span,
                                operand_span: checker.pool.expr_span(inner_id),
                            },
                        );
                        ArType::Error
                    }
                }
                UnaryOp::Await => {
                    report_unsupported(
                        checker,
                        span,
                        "await",
                        "v0.3 ASYNC: effect flags and async lowering",
                    );
                    ArType::Error
                }
            }
        }
        ExprKind::Binary { op, left, right } => {
            let left_id = *left;
            let right_id = *right;
            let left_ty = synth_expr(checker, left_id);
            let right_ty = synth_expr(checker, right_id);

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
                                op_span: span,
                                left_span: checker.pool.expr_span(left_id),
                                right_span: checker.pool.expr_span(right_id),
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
                                op_span: span,
                                left_span: checker.pool.expr_span(left_id),
                                right_span: checker.pool.expr_span(right_id),
                            },
                        );
                    }
                    ArType::Primitive(Primitive::Bool)
                }
                _ => ArType::Error,
            }
        }
        ExprKind::Error => ArType::Error,
    }
}
