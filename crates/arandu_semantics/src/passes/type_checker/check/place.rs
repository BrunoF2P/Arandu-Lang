use super::super::TypeChecker;
use super::super::constraints::ConstraintOrigin;
use super::super::types::{ArType, Primitive};

pub(crate) fn synth_place(checker: &mut TypeChecker, place: &arandu_parser::Place) -> ArType {
    let root_key = crate::NodeKey::from(place.span);
    let mut current_ty = if let Some(symbol_id) = checker.resolved.value_refs.get(&root_key) {
        if let Some(ty) = checker.ctx.lookup(*symbol_id) {
            ty.clone()
        } else if let Some(ty) = checker.decl_type(*symbol_id) {
            ty
        } else {
            ArType::Error
        }
    } else {
        ArType::Error
    };

    // 2. Traverse suffixes
    for suffix in &place.suffixes {
        if current_ty.is_error() {
            break;
        }
        match suffix {
            arandu_parser::PlaceSuffix::Field { span, name } => {
                let (actual_base_ty, was_nullable) = match &current_ty {
                    ArType::Nullable(inner) => (inner.as_ref().clone(), true),
                    other => (other.clone(), false),
                };
                if was_nullable {
                    let diag = crate::Diagnostic::error(
                        crate::DiagCode::T006NotNullable,
                        format!(
                            "cannot access field '{}' on nullable type '{}'",
                            name,
                            current_ty.display(&checker.symbols)
                        ),
                        *span,
                    )
                    .with_label(
                        place.span,
                        format!("this has type '{}'", current_ty.display(&checker.symbols)),
                    )
                    .with_hint("use safe access `?.` or make the value non-nullable".to_string());
                    checker.diagnostics.push(diag);
                    current_ty = ArType::Error;
                    break;
                }
                let struct_id_opt = match &actual_base_ty {
                    ArType::Named(id, _) => Some(*id),
                    ArType::Ptr(inner) => match &**inner {
                        ArType::Named(id, _) => Some(*id),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(struct_id) = struct_id_opt
                    && let Some(fields) = checker.type_info.struct_fields.get(&struct_id)
                    && let Some(field_ty) = fields.get(name)
                {
                    current_ty = field_ty.clone();
                } else {
                    checker.add_constraint(
                        actual_base_ty.clone(),
                        ArType::Error,
                        ConstraintOrigin::UndefinedField {
                            base_span: place.span,
                            field_span: *span,
                            field_name: name.clone(),
                        },
                    );
                    current_ty = ArType::Error;
                }
            }
            arandu_parser::PlaceSuffix::Index { span, expr } => {
                let index_ty = super::super::synth::synth_expr(checker, expr);
                let (actual_base_ty, was_nullable) = match &current_ty {
                    ArType::Nullable(inner) => (inner.as_ref().clone(), true),
                    other => (other.clone(), false),
                };
                if was_nullable {
                    let diag = crate::Diagnostic::error(
                        crate::DiagCode::T006NotNullable,
                        format!(
                            "cannot index nullable type '{}'",
                            current_ty.display(&checker.symbols)
                        ),
                        *span,
                    )
                    .with_label(
                        place.span,
                        format!("this has type '{}'", current_ty.display(&checker.symbols)),
                    )
                    .with_hint(
                        "use safe index `?[...]` or make the value non-nullable".to_string(),
                    );
                    checker.diagnostics.push(diag);
                    current_ty = ArType::Error;
                    break;
                }
                match &actual_base_ty {
                    ArType::Array(_, inner) | ArType::Slice(inner) => {
                        current_ty = inner.as_ref().clone();
                    }
                    _ => {
                        checker.add_constraint(
                            actual_base_ty.clone(),
                            ArType::Error,
                            ConstraintOrigin::InvalidIndex {
                                base_span: place.span,
                                index_span: expr.span(),
                                is_base_error: true,
                            },
                        );
                        current_ty = ArType::Error;
                    }
                }
                if !index_ty.is_error() && !index_ty.is_integer() {
                    checker.add_constraint(
                        ArType::Primitive(Primitive::Int),
                        index_ty,
                        ConstraintOrigin::InvalidIndex {
                            base_span: place.span,
                            index_span: expr.span(),
                            is_base_error: false,
                        },
                    );
                }
            }
        }
    }

    current_ty
}
