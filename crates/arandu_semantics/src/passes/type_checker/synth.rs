use arandu_parser::{BinaryOp, Expr, Pattern, UnaryOp};

use super::TypeChecker;
use super::constraints::ConstraintOrigin;
use super::types::{ArType, Primitive};

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
        Expr::Nil { .. } => ArType::Nullable(Box::new(ArType::Error)), // nil needs context
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
                        super::EnumPayloadShape::Unit => {
                            return enum_ty;
                        }
                        super::EnumPayloadShape::Tuple(tys) => {
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
            span: _,
        } => {
            let _callee_ty = synth_expr(checker, callee);
            let mut arg_tys = Vec::new();
            for arg in args {
                arg_tys.push(super::types::lower_type_expr(
                    arg,
                    &checker.symbols,
                    checker.symbols.global_scope(), // Simplification for now
                    &checker.resolved,
                ));
            }
            // For now, generics return error since we lack full generic instantiation
            ArType::Error
        }
        Expr::Field { span, base, field } => {
            if let Some(ty) = resolve_namespace_member_type(checker, *span) {
                ty
            } else {
                resolve_field(checker, base, field, *span, false)
            }
        }
        Expr::SafeField { span, base, field } => {
            if let Some(ty) = resolve_namespace_member_type(checker, *span) {
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
            match &inner_ty {
                ArType::Tuple(types) if types.len() == 2 => {
                    let err_ty = &types[1];
                    let is_err = match err_ty {
                        ArType::Err => true,
                        ArType::Nullable(inner) => matches!(**inner, ArType::Err),
                        _ => false,
                    };
                    if is_err {
                        types[0].clone()
                    } else {
                        checker.add_constraint(
                            ArType::Error,
                            inner_ty,
                            ConstraintOrigin::TryInvalid { span: *span },
                        );
                        ArType::Error
                    }
                }
                ArType::Error => ArType::Error,
                _ => {
                    checker.add_constraint(
                        ArType::Error,
                        inner_ty,
                        ConstraintOrigin::TryInvalid { span: *span },
                    );
                    ArType::Error
                }
            }
        }
        Expr::Call {
            span,
            callee,
            args,
            trailing_block: _,
        } => {
            let callee_ty = synth_expr(checker, callee);
            match callee_ty {
                ArType::Func(params, ret) => {
                    if params.len() != args.len() {
                        checker.diagnostics.push(crate::Diagnostic::error(
                            crate::DiagCode::T012WrongArgCount,
                            format!("expected {} arguments, found {}", params.len(), args.len()),
                            *span,
                        ));
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let arg_ty = synth_expr(checker, arg);
                        if i < params.len() {
                            if !super::types::unify(&params[i], &arg_ty) {
                                checker.add_constraint(
                                    params[i].clone(),
                                    arg_ty,
                                    ConstraintOrigin::CallArg {
                                        call_span: *span,
                                        param_span: *span,
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
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T003IncompatibleCallArg,
                        format!(
                            "cannot call non-function type '{}'",
                            other.display(&checker.symbols)
                        ),
                        *span,
                    ));
                    ArType::Error
                }
            }
        }
        Expr::StructLiteral { span, ty, fields } => {
            let struct_ty = super::types::lower_type_expr(
                ty,
                &checker.symbols,
                checker.symbols.global_scope(),
                &checker.resolved,
            );
            if let ArType::Named(symbol_id, _) = &struct_ty {
                let has_struct_def = checker.type_info.struct_fields.contains_key(symbol_id);
                if has_struct_def {
                    for field in fields {
                        let field_val_ty = synth_expr(checker, &field.value);
                        let defined_field_ty = checker
                            .type_info
                            .struct_fields
                            .get(symbol_id)
                            .and_then(|df| df.get(&field.name).cloned());
                        if let Some(defined_field_ty) = defined_field_ty {
                            if !super::types::unify(&defined_field_ty, &field_val_ty) {
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
        Expr::Array { span: _, items } => {
            let mut elem_ty = ArType::Error;
            for item in items {
                let item_ty = synth_expr(checker, item);
                if elem_ty.is_error() {
                    elem_ty = item_ty;
                } else if !super::types::unify(&elem_ty, &item_ty) {
                    // element type mismatch
                }
            }
            ArType::Array(items.len() as u64, Box::new(elem_ty))
        }
        Expr::Lambda {
            span: _,
            params: _,
            body: _,
        } => ArType::Error,
        Expr::Alloc { span: _, expr } => {
            let inner_ty = synth_expr(checker, expr);
            ArType::Ptr(Box::new(inner_ty))
        }
        Expr::AsyncBlock { span: _, block } => super::check::check_block(checker, block),
        Expr::UnsafeBlock { span: _, block } => super::check::check_block(checker, block),
        Expr::If {
            span: _,
            condition,
            then_block,
            else_block,
        } => {
            super::check::check_condition(checker, condition);
            let then_ty = super::check::check_block(checker, then_block);
            let else_ty = super::check::check_block(checker, else_block);
            if !super::types::unify(&then_ty, &else_ty) {
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
            let mut expected_arm_ty = ArType::Error;
            let mut first_arm_span = *span;

            for (i, arm) in arms.iter().enumerate() {
                check_pattern(checker, &arm.pattern, &value_ty);
                let arm_ty = match &arm.body {
                    arandu_parser::MatchArmBody::Expr { expr, .. } => synth_expr(checker, expr),
                    arandu_parser::MatchArmBody::Block { block, .. } => {
                        super::check::check_block(checker, block)
                    }
                };

                if i == 0 {
                    expected_arm_ty = arm_ty;
                    first_arm_span = arm.span;
                } else if !super::types::unify(&expected_arm_ty, &arm_ty) {
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
            span: _,
            expr,
            handler: _,
        } => {
            let _inner_ty = synth_expr(checker, expr);
            ArType::Error
        }
        Expr::NullCoalesce {
            span: _,
            left,
            right,
        } => {
            let _left_ty = synth_expr(checker, left);
            synth_expr(checker, right)
        }
        Expr::Cast { span: _, expr, ty } => {
            let _found_ty = synth_expr(checker, expr);
            let target_ty = super::types::lower_type_expr(
                ty,
                &checker.symbols,
                checker.symbols.global_scope(),
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
                    if !expr_ty.is_numeric() {
                        checker.add_constraint(
                            ArType::Primitive(Primitive::Int),
                            expr_ty.clone(),
                            ConstraintOrigin::UnaryOp {
                                op_span: *span,
                                operand_span: expr.span(),
                            },
                        );
                        ArType::Error
                    } else {
                        expr_ty
                    }
                }
                UnaryOp::Not => {
                    if !super::types::unify(&expr_ty, &ArType::Primitive(Primitive::Bool)) {
                        checker.add_constraint(
                            ArType::Primitive(Primitive::Bool),
                            expr_ty.clone(),
                            ConstraintOrigin::UnaryOp {
                                op_span: *span,
                                operand_span: expr.span(),
                            },
                        );
                        ArType::Error
                    } else {
                        ArType::Primitive(Primitive::Bool)
                    }
                }
                UnaryOp::BitNot => {
                    if !expr_ty.is_integer() {
                        checker.add_constraint(
                            ArType::Primitive(Primitive::Int),
                            expr_ty.clone(),
                            ConstraintOrigin::UnaryOp {
                                op_span: *span,
                                operand_span: expr.span(),
                            },
                        );
                        ArType::Error
                    } else {
                        expr_ty
                    }
                }
                UnaryOp::Await => ArType::Error,
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
                    if !super::types::unify(&left_ty, &right_ty)
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
                    super::types::resolve_literal_pair(&left_ty, &right_ty)
                }
                BinaryOp::Equal
                | BinaryOp::NotEqual
                | BinaryOp::Lt
                | BinaryOp::Gt
                | BinaryOp::LtEqual
                | BinaryOp::GtEqual => {
                    if !super::types::unify(&left_ty, &right_ty) {
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

fn resolve_namespace_member_type(checker: &TypeChecker, span: arandu_lexer::Span) -> Option<ArType> {
    let key = crate::NodeKey::from(span);
    if let Some(symbol_id) = checker.resolved.value_refs.get(&key) {
        if let Some(ty) = checker.ctx.lookup(*symbol_id) {
            return Some(ty.clone());
        }
        if let Some(ty) = checker.type_info.decl_types.get(symbol_id) {
            return Some(ty.clone());
        }
    }
    None
}

fn resolve_field(
    checker: &mut TypeChecker,
    base: &Expr,
    field: &String,
    field_span: arandu_lexer::Span,
    safe: bool,
) -> ArType {
    let base_ty = synth_expr(checker, base);
    if base_ty.is_error() {
        return ArType::Error;
    }

    let (actual_base_ty, was_nullable) = match &base_ty {
        ArType::Nullable(inner) => (inner.as_ref().clone(), true),
        other => (other.clone(), false),
    };

    if was_nullable && !safe {
        checker.diagnostics.push(crate::Diagnostic::error(
            crate::DiagCode::T006NotNullable,
            format!(
                "cannot access field '{}' on nullable type '{}'",
                field,
                base_ty.display(&checker.symbols)
            ),
            field_span,
        ));
        return ArType::Error;
    }

    let struct_id_opt = match &actual_base_ty {
        ArType::Named(id, _) => Some(*id),
        ArType::Ptr(inner) => match &**inner {
            ArType::Named(id, _) => Some(*id),
            _ => None,
        },
        _ => None,
    };

    let field_ty = if let Some(struct_id) = struct_id_opt
        && let Some(fields) = checker.type_info.struct_fields.get(&struct_id)
        && let Some(field_ty) = fields.get(field)
    {
        field_ty.clone()
    } else {
        checker.add_constraint(
            actual_base_ty.clone(),
            ArType::Error,
            ConstraintOrigin::UndefinedField {
                base_span: base.span(),
                field_span,
                field_name: field.clone(),
            },
        );
        return ArType::Error;
    };

    if safe || was_nullable {
        ArType::Nullable(Box::new(field_ty))
    } else {
        field_ty
    }
}

fn resolve_index(checker: &mut TypeChecker, base: &Expr, index: &Expr, safe: bool) -> ArType {
    let base_ty = synth_expr(checker, base);
    let index_ty = synth_expr(checker, index);

    if base_ty.is_error() {
        return ArType::Error;
    }

    let (actual_base_ty, was_nullable) = match &base_ty {
        ArType::Nullable(inner) => (inner.as_ref().clone(), true),
        other => (other.clone(), false),
    };

    if was_nullable && !safe {
        checker.diagnostics.push(crate::Diagnostic::error(
            crate::DiagCode::T006NotNullable,
            format!(
                "cannot index nullable type '{}'",
                base_ty.display(&checker.symbols)
            ),
            index.span(),
        ));
        return ArType::Error;
    }

    let elem_ty = match &actual_base_ty {
        ArType::Array(_, inner) | ArType::Slice(inner) => inner.as_ref().clone(),
        _ => {
            checker.add_constraint(
                actual_base_ty.clone(),
                ArType::Error,
                ConstraintOrigin::InvalidIndex {
                    base_span: base.span(),
                    index_span: index.span(),
                    is_base_error: true,
                },
            );
            return ArType::Error;
        }
    };

    if !index_ty.is_error() && !index_ty.is_integer() {
        checker.add_constraint(
            ArType::Primitive(Primitive::Int),
            index_ty,
            ConstraintOrigin::InvalidIndex {
                base_span: base.span(),
                index_span: index.span(),
                is_base_error: false,
            },
        );
    }

    if safe || was_nullable {
        ArType::Nullable(Box::new(elem_ty))
    } else {
        elem_ty
    }
}

pub fn check_pattern(checker: &mut TypeChecker, pattern: &Pattern, value_ty: &ArType) {
    match pattern {
        Pattern::Wildcard { .. } => {}
        Pattern::Bind { span, name: _ } => {
            let key = crate::NodeKey::from(*span);
            if let Some(symbol_id) = checker.resolved.definitions.get(&key) {
                checker.ctx.bind(*symbol_id, value_ty.clone());
                checker
                    .type_info
                    .decl_types
                    .insert(*symbol_id, value_ty.clone());
            }
        }
        Pattern::Literal { expr, .. } => {
            let expr_ty = synth_expr(checker, expr);
            if !super::types::unify(value_ty, &expr_ty) {
                checker.add_constraint(
                    value_ty.clone(),
                    expr_ty,
                    ConstraintOrigin::Assignment {
                        lhs_span: pattern.span(),
                        rhs_span: expr.span(),
                    },
                );
            }
        }
        Pattern::Enum {
            span,
            type_name,
            variant,
            payload,
        } => {
            let type_key = crate::NodeKey::from(type_name.span);
            if let Some(enum_symbol_id) = checker.resolved.type_refs.get(&type_key).copied() {
                let expected_enum_ty = ArType::Named(enum_symbol_id, vec![]);
                if !super::types::unify(value_ty, &expected_enum_ty) {
                    checker.add_constraint(
                        expected_enum_ty.clone(),
                        value_ty.clone(),
                        ConstraintOrigin::Assignment {
                            lhs_span: *span,
                            rhs_span: type_name.span,
                        },
                    );
                }

                let variant_symbol_opt = checker
                    .symbols
                    .lookup_associated_member(&type_name.path.join("."), variant);
                if let Some(variant_symbol_id) = variant_symbol_opt {
                    let shape_opt = checker
                        .type_info
                        .enum_variants
                        .get(&variant_symbol_id)
                        .cloned();
                    if let Some((_, shape)) = shape_opt {
                        match shape {
                            super::EnumPayloadShape::Unit => {
                                if !payload.is_empty() {
                                    checker.diagnostics.push(crate::Diagnostic::error(
                                        crate::DiagCode::T012WrongArgCount,
                                        format!(
                                            "enum variant '{}' expects 0 payload items, found {}",
                                            variant,
                                            payload.len()
                                        ),
                                        *span,
                                    ));
                                }
                            }
                            super::EnumPayloadShape::Tuple(tys) => {
                                if tys.len() != payload.len() {
                                    checker.diagnostics.push(crate::Diagnostic::error(
                                        crate::DiagCode::T012WrongArgCount,
                                        format!(
                                            "enum variant '{}' expects {} payload items, found {}",
                                            variant,
                                            tys.len(),
                                            payload.len()
                                        ),
                                        *span,
                                    ));
                                }
                                for (i, pat) in payload.iter().enumerate() {
                                    let expected_pat_ty =
                                        tys.get(i).cloned().unwrap_or(ArType::Error);
                                    check_pattern(checker, pat, &expected_pat_ty);
                                }
                            }
                        }
                    }
                } else {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T018UndefinedField,
                        format!(
                            "variant '{}' is not defined on enum '{}'",
                            variant,
                            type_name.path.join(".")
                        ),
                        *span,
                    ));
                }
            }
        }
        Pattern::TypeTuple {
            span,
            name,
            payload,
        } => {
            if let ArType::Named(enum_symbol_id, _) = value_ty {
                let enum_name = &checker.symbols.get(*enum_symbol_id).name.clone();
                let variant_symbol_opt = checker.symbols.lookup_associated_member(enum_name, name);
                if let Some(variant_symbol_id) = variant_symbol_opt {
                    let shape_opt = checker
                        .type_info
                        .enum_variants
                        .get(&variant_symbol_id)
                        .cloned();
                    if let Some((_, shape)) = shape_opt {
                        match shape {
                            super::EnumPayloadShape::Unit => {
                                if !payload.is_empty() {
                                    checker.diagnostics.push(crate::Diagnostic::error(
                                        crate::DiagCode::T012WrongArgCount,
                                        format!(
                                            "enum variant '{}' expects 0 payload items, found {}",
                                            name,
                                            payload.len()
                                        ),
                                        *span,
                                    ));
                                }
                            }
                            super::EnumPayloadShape::Tuple(tys) => {
                                if tys.len() != payload.len() {
                                    checker.diagnostics.push(crate::Diagnostic::error(
                                        crate::DiagCode::T012WrongArgCount,
                                        format!(
                                            "enum variant '{}' expects {} payload items, found {}",
                                            name,
                                            tys.len(),
                                            payload.len()
                                        ),
                                        *span,
                                    ));
                                }
                                for (i, pat) in payload.iter().enumerate() {
                                    let expected_pat_ty =
                                        tys.get(i).cloned().unwrap_or(ArType::Error);
                                    check_pattern(checker, pat, &expected_pat_ty);
                                }
                            }
                        }
                    }
                } else {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T018UndefinedField,
                        format!("variant '{}' is not defined on enum '{}'", name, enum_name),
                        *span,
                    ));
                }
            } else {
                checker.diagnostics.push(crate::Diagnostic::error(
                    crate::DiagCode::T002IncompatibleAssignment,
                    format!(
                        "cannot match type tuple pattern against non-enum type '{}'",
                        value_ty.display(&checker.symbols)
                    ),
                    *span,
                ));
            }
        }
        Pattern::Tuple { items, span: _ } => {
            if let ArType::Tuple(tys) = value_ty {
                for (i, item) in items.iter().enumerate() {
                    let item_ty = tys.get(i).cloned().unwrap_or(ArType::Error);
                    check_pattern(checker, item, &item_ty);
                }
            } else {
                checker.diagnostics.push(crate::Diagnostic::error(
                    crate::DiagCode::T002IncompatibleAssignment,
                    format!(
                        "cannot match tuple pattern against non-tuple type '{}'",
                        value_ty.display(&checker.symbols)
                    ),
                    pattern.span(),
                ));
            }
        }
        Pattern::Struct {
            type_name,
            fields,
            span: _,
        } => {
            let type_key = crate::NodeKey::from(type_name.span);
            if let Some(struct_symbol_id) = checker.resolved.type_refs.get(&type_key).copied() {
                let expected_struct_ty = ArType::Named(struct_symbol_id, vec![]);
                if !super::types::unify(value_ty, &expected_struct_ty) {
                    checker.add_constraint(
                        expected_struct_ty.clone(),
                        value_ty.clone(),
                        ConstraintOrigin::Assignment {
                            lhs_span: pattern.span(),
                            rhs_span: type_name.span,
                        },
                    );
                }
                for field in fields {
                    let field_ty_opt = checker
                        .type_info
                        .struct_fields
                        .get(&struct_symbol_id)
                        .and_then(|df| df.get(&field.name).cloned());
                    if let Some(field_ty) = field_ty_opt {
                        if let Some(pat) = &field.pattern {
                            check_pattern(checker, pat, &field_ty);
                        } else {
                            let key = crate::NodeKey::from(field.span);
                            if let Some(symbol_id) = checker.resolved.definitions.get(&key).copied()
                            {
                                checker.ctx.bind(symbol_id, field_ty.clone());
                                checker
                                    .type_info
                                    .decl_types
                                    .insert(symbol_id, field_ty.clone());
                            }
                        }
                    } else {
                        checker.diagnostics.push(crate::Diagnostic::error(
                            crate::DiagCode::T018UndefinedField,
                            format!(
                                "field '{}' is not defined on struct '{}'",
                                field.name,
                                type_name.path.join(".")
                            ),
                            field.span,
                        ));
                    }
                }
            }
        }
        Pattern::Range {
            start,
            end,
            span: _,
            ..
        } => {
            let start_ty = synth_expr(checker, start);
            let end_ty = synth_expr(checker, end);
            if !super::types::unify(value_ty, &start_ty) {
                checker.add_constraint(
                    value_ty.clone(),
                    start_ty,
                    ConstraintOrigin::Assignment {
                        lhs_span: pattern.span(),
                        rhs_span: start.span(),
                    },
                );
            }
            if !super::types::unify(value_ty, &end_ty) {
                checker.add_constraint(
                    value_ty.clone(),
                    end_ty,
                    ConstraintOrigin::Assignment {
                        lhs_span: pattern.span(),
                        rhs_span: end.span(),
                    },
                );
            }
        }
    }
}
