use super::super::TypeChecker;
use super::super::constraints::ConstraintOrigin;
use super::super::types::{ArType, Primitive};

pub(crate) fn synth_place(checker: &mut TypeChecker<'_>, place: &arandu_parser::Place) -> ArType {
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
                let interner = &checker.type_info.type_interner;
                let (actual_base_ty, was_nullable) = match &current_ty {
                    ArType::Nullable(inner) => (
                        interner.resolve(*inner).clone(),
                        true,
                    ),
                    other => (other.clone(), false),
                };
                if was_nullable {
                    let diag = crate::Diagnostic::error(
                        crate::DiagCode::T006NotNullable,
                        format!(
                            "cannot access field '{}' on nullable type '{}'",
                            name,
                            current_ty.display(&checker.symbols, interner)
                        ),
                        *span,
                    )
                    .with_label(
                        place.span,
                        format!("this has type '{}'", current_ty.display(&checker.symbols, interner)),
                    )
                    .with_hint("use safe access `?.` or make the value non-nullable".to_string());
                    checker.diagnostics.push(diag);
                    current_ty = ArType::Error;
                    break;
                }
                let struct_info_opt = match &actual_base_ty {
                    ArType::Named(id, args) => Some((*id, args.clone())),
                    ArType::Ptr(inner) => match interner.resolve(*inner) {
                        ArType::Named(id, args) => Some((*id, args.clone())),
                        _ => None,
                    },
                    _ => None,
                };
                let field_from_struct = if let Some((struct_id, args)) = struct_info_opt {
                    let resolved_args: Vec<ArType> = args
                        .iter()
                        .map(|&a| interner.resolve(a).clone())
                        .collect();
                    if let Some(fields_map) = super::super::types::struct_fields_instantiated(checker, struct_id, &resolved_args) {
                        fields_map.get(name).cloned()
                    } else {
                        checker.type_info.struct_fields.get(&struct_id).and_then(|fields| fields.get(name).cloned())
                    }
                } else {
                    None
                };

                if let Some(field_ty) = field_from_struct {
                    current_ty = field_ty;
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
                let index_ty = super::super::synth::synth_expr(checker, *expr);
                let interner = &checker.type_info.type_interner;
                let (actual_base_ty, was_nullable) = match &current_ty {
                    ArType::Nullable(inner) => (
                        interner.resolve(*inner).clone(),
                        true,
                    ),
                    other => (other.clone(), false),
                };
                if was_nullable {
                    let diag = crate::Diagnostic::error(
                        crate::DiagCode::T006NotNullable,
                        format!(
                            "cannot index nullable type '{}'",
                            current_ty.display(&checker.symbols, interner)
                        ),
                        *span,
                    )
                    .with_label(
                        place.span,
                        format!("this has type '{}'", current_ty.display(&checker.symbols, interner)),
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
                        current_ty = interner.resolve(*inner).clone();
                    }
                    _ => {
                        checker.add_constraint(
                            actual_base_ty.clone(),
                            ArType::Error,
                            ConstraintOrigin::InvalidIndex {
                                base_span: place.span,
                                index_span: checker.pool.expr_span(*expr),
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
                            index_span: checker.pool.expr_span(*expr),
                            is_base_error: false,
                        },
                    );
                }
            }
        }
    }

    current_ty
}
