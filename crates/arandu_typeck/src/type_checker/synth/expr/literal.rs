use arandu_lexer::Span;
use arandu_parser::ast_pool::{ExprId, ExprKind};
use smallvec::SmallVec;

use super::synth_expr;
use crate::type_checker::TypeChecker;
use crate::type_checker::constraints::ConstraintOrigin;
use crate::type_checker::types::{self, ArType, Primitive};
use arandu_middle::types::type_interner::TypeId;

/// Stricter than `unify` for array literals: int and float literals must not mix.
pub(super) fn array_element_types_compatible(
    a: &ArType,
    b: &ArType,
    interner: &arandu_middle::types::TypeInterner,
) -> bool {
    if matches!(
        (a, b),
        (ArType::IntLiteral, ArType::FloatLiteral) | (ArType::FloatLiteral, ArType::IntLiteral)
    ) {
        return false;
    }
    types::unify(a, b, interner)
}

#[tracing::instrument(level = "trace", target = "arandu_typeck", skip(checker, _expr))]
pub(super) fn synth_literal_expr(
    checker: &mut TypeChecker<'_>,
    _expr: ExprId,
    kind: &ExprKind,
    span: Span,
) -> Option<TypeId> {
    match kind {
        ExprKind::Int { .. } => Some(checker.intern(ArType::IntLiteral)),
        ExprKind::Float { .. } => Some(checker.intern(ArType::FloatLiteral)),
        ExprKind::Bool { .. } => Some(checker.intern(ArType::Primitive(Primitive::Bool))),
        ExprKind::Char { .. } => Some(checker.intern(ArType::Primitive(Primitive::Char))),
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
            Some(checker.intern(ArType::Primitive(Primitive::Str)))
        }
        ExprKind::Nil => {
            if let Some(ret_id) = checker.ctx.current_return() {
                // If it is nil, it can fallback to return type or nullable/option
                let ret = checker.resolve(ret_id).clone();
                if types::is_err_type(&ret, &checker.type_info.type_interner) {
                    let err_id = checker.intern(ArType::Error);
                    Some(checker.intern(ArType::Nullable(err_id)))
                } else if types::is_tryable_type(&ret, &checker.type_info.type_interner) {
                    Some(ret_id)
                } else {
                    Some(checker.intern(ArType::Nullable(ret_id)))
                }
            } else {
                let err_id = checker.intern(ArType::Error);
                Some(checker.intern(ArType::Nullable(err_id)))
            }
        }
        ExprKind::StructLiteral { ty, fields } => {
            let ty_id = *ty;
            let fields_range = *fields;
            let struct_ty = checker.lower_type_expr(ty_id, checker.type_scope());
            let struct_ty_id = checker.intern(struct_ty);
            let struct_info = match checker.resolve(struct_ty_id) {
                ArType::Named(symbol_id, generic_args) => Some((*symbol_id, generic_args.clone())),
                _ => None,
            };
            if let Some((symbol_id, generic_args)) = struct_info {
                let resolved_args: Vec<ArType> = generic_args
                    .iter()
                    .map(|&arg_id| checker.resolve(arg_id).clone())
                    .collect();
                let field_map =
                    types::struct_fields_instantiated(checker, symbol_id, &resolved_args)
                        .or_else(|| checker.type_info.struct_fields.get(&symbol_id).cloned());
                let field_ids = checker.pool.field_init_list(fields_range).to_vec();

                let mut seen_fields = SmallVec::<[(&str, Span); 8]>::new();
                for &fid in &field_ids {
                    let field = checker.pool.field_init(fid);
                    if let Some((_, prev_span)) =
                        seen_fields.iter().find(|(name, _)| *name == field.name)
                    {
                        let diag = crate::Diagnostic::error(
                            crate::DiagCode::T028DuplicateFieldInit,
                            format!("field '{}' initialized more than once", field.name),
                            field.span,
                        )
                        .with_label(*prev_span, "first initialization here")
                        .with_label(field.span, "duplicate initialization");
                        checker.diagnostics.push(diag);
                    } else {
                        seen_fields.push((&field.name, field.span));
                    }
                }

                if let Some(fields_def) = field_map {
                    for fid in &field_ids {
                        let field = checker.pool.field_init(*fid);
                        let field_val_ty_id = synth_expr(checker, field.value);
                        let defined_field_ty_opt = fields_def.get(&field.name).cloned();
                        if let Some(defined_field_ty) = defined_field_ty_opt {
                            let field_val_ty = checker.resolve(field_val_ty_id);
                            if !types::unify(
                                &defined_field_ty,
                                field_val_ty,
                                &checker.type_info.type_interner,
                            ) {
                                checker.add_constraint(
                                    defined_field_ty,
                                    field_val_ty_id,
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
                                struct_ty_id,
                                ArType::Error,
                                ConstraintOrigin::UndefinedField {
                                    base_span: checker.pool.type_expr_span(ty_id),
                                    field_span: field.span,
                                    field_name: field.name.clone(),
                                },
                            );
                        }
                    }

                    let mut missing_fields = Vec::new();
                    for def_name in fields_def.keys() {
                        if !seen_fields.iter().any(|(name, _)| name == def_name) {
                            missing_fields.push(format!("`{def_name}`"));
                        }
                    }
                    if !missing_fields.is_empty() {
                        missing_fields.sort();
                        let missing_str = missing_fields.join(", ");
                        let struct_name = checker.symbols.get(symbol_id).name.clone();
                        let diag = crate::Diagnostic::error(
                            crate::DiagCode::T027MissingStructFields,
                            format!("missing fields {missing_str} in struct initializer"),
                            span,
                        )
                        .with_label(span, format!("instantiating struct '{struct_name}' here"));
                        checker.diagnostics.push(diag);
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
            Some(struct_ty_id)
        }
        ExprKind::Array { items } => {
            let items_range = *items;
            let error_id = checker.intern(ArType::Error);
            let mut elem_ty_id = error_id;
            let item_ids = checker.pool.expr_list(items_range).to_vec();
            for (i, item_id) in item_ids.iter().copied().enumerate() {
                let item_ty_id = synth_expr(checker, item_id);
                if checker.resolve(elem_ty_id).is_error() {
                    elem_ty_id = item_ty_id;
                } else {
                    let elem_ty = checker.resolve(elem_ty_id);
                    let item_ty = checker.resolve(item_ty_id);
                    if !array_element_types_compatible(
                        elem_ty,
                        item_ty,
                        &checker.type_info.type_interner,
                    ) {
                        checker.add_constraint(
                            elem_ty_id,
                            item_ty_id,
                            ConstraintOrigin::ArrayLiteral {
                                array_span: span,
                                item_span: checker.pool.expr_span(item_id),
                                item_index: i,
                            },
                        );
                        elem_ty_id = error_id;
                    }
                }
            }
            Some(checker.intern(ArType::Array(items_range.len as u64, elem_ty_id)))
        }
        _ => None,
    }
}
