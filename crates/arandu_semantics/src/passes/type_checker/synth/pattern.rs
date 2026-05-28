use arandu_parser::Pattern;

use super::super::TypeChecker;
use super::super::constraints::ConstraintOrigin;
use super::super::types::ArType;
use super::expr::synth_expr;

pub fn check_pattern(checker: &mut TypeChecker<'_>, pattern: &Pattern, value_ty: &ArType) {
    match pattern {
        Pattern::Wildcard { .. } => {}
        Pattern::Bind { span, name: _ } => {
            let key = crate::NodeKey::from(*span);
            if let Some(symbol_id) = checker.resolved.definitions.get(&key) {
                checker.ctx.bind(*symbol_id, value_ty.clone());
                checker.record_decl_type(*symbol_id, value_ty.clone());
            }
        }
        Pattern::Literal { expr, .. } => {
            let expr_ty = synth_expr(checker, **expr);
            if !super::super::types::unify(value_ty, &expr_ty) {
                checker.add_constraint(
                    value_ty.clone(),
                    expr_ty,
                    ConstraintOrigin::Assignment {
                        lhs_span: pattern.span(),
                        rhs_span: checker.pool.expr_span(**expr),
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
                if !super::super::types::unify(value_ty, &expected_enum_ty) {
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
                            super::super::EnumPayloadShape::Unit => {
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
                            super::super::EnumPayloadShape::Tuple(tys) => {
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
                            super::super::EnumPayloadShape::Unit => {
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
                            super::super::EnumPayloadShape::Tuple(tys) => {
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
                        format!("variant '{name}' is not defined on enum '{enum_name}'"),
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
                if !super::super::types::unify(value_ty, &expected_struct_ty) {
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
                                checker.record_decl_type(symbol_id, field_ty.clone());
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
            let start_ty = synth_expr(checker, **start);
            let end_ty = synth_expr(checker, **end);
            if !super::super::types::unify(value_ty, &start_ty) {
                checker.add_constraint(
                    value_ty.clone(),
                    start_ty,
                    ConstraintOrigin::Assignment {
                        lhs_span: pattern.span(),
                        rhs_span: checker.pool.expr_span(**start),
                    },
                );
            }
            if !super::super::types::unify(value_ty, &end_ty) {
                checker.add_constraint(
                    value_ty.clone(),
                    end_ty,
                    ConstraintOrigin::Assignment {
                        lhs_span: pattern.span(),
                        rhs_span: checker.pool.expr_span(**end),
                    },
                );
            }
        }
    }
}
