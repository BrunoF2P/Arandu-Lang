use crate::diagnostics::Diagnostic;
use crate::hir::{HirPlace, HirPlaceSuffix};
use crate::passes::type_checker::types::ArType;
use crate::{NodeKey, TypeCheckResult};
use arandu_parser::{Place, PlaceSuffix};

pub(crate) fn lower_place(
    type_check: &TypeCheckResult,
    place: &Place,
) -> Result<HirPlace, Diagnostic> {
    let root_key = NodeKey::from(place.span);
    let root_symbol = type_check
        .resolved
        .value_refs
        .get(&root_key)
        .copied()
        .or_else(|| type_check.resolved.definitions.get(&root_key).copied())
        .ok_or_else(|| {
            Diagnostic::error(
                crate::diagnostics::DiagCode::L001LoweringUnresolvedSymbol,
                "cannot lower place: root symbol not resolved",
                place.span,
            )
        })?;

    let mut current_ty = if let Some(ty) = type_check.type_info.decl_type(root_symbol) {
        ty.clone()
    } else {
        ArType::Error
    };

    let mut suffixes = Vec::new();
    for suffix in &place.suffixes {
        if current_ty.is_error() {
            match suffix {
                PlaceSuffix::Field { span, name } => {
                    suffixes.push(HirPlaceSuffix::Field {
                        span: *span,
                        name: name.clone(),
                        field_symbol: None,
                        ty: ArType::Error,
                    });
                }
                PlaceSuffix::Index { span, expr } => {
                    suffixes.push(HirPlaceSuffix::Index {
                        span: *span,
                        expr: super::expr::lower_expr(type_check, expr)?,
                        ty: ArType::Error,
                    });
                }
            }
            continue;
        }

        match suffix {
            PlaceSuffix::Field { span, name } => {
                let actual_base_ty = match &current_ty {
                    ArType::Nullable(inner) => inner.as_ref().clone(),
                    other => other.clone(),
                };
                let struct_id_opt = match &actual_base_ty {
                    ArType::Named(id, _) => Some(*id),
                    ArType::Ptr(inner) => match &**inner {
                        ArType::Named(id, _) => Some(*id),
                        _ => None,
                    },
                    _ => None,
                };
                let (field_ty, field_symbol) = if let Some(struct_id) = struct_id_opt
                    && let Some(fields) = type_check.type_info.struct_fields.get(&struct_id)
                    && let Some(ty) = fields.get(name)
                {
                    let symbol = type_check
                        .type_info
                        .struct_field_symbols
                        .get(&struct_id)
                        .and_then(|fields| fields.get(name))
                        .copied();
                    (ty.clone(), symbol)
                } else {
                    (ArType::Error, None)
                };
                current_ty = field_ty.clone();
                suffixes.push(HirPlaceSuffix::Field {
                    span: *span,
                    name: name.clone(),
                    field_symbol,
                    ty: field_ty,
                });
            }
            PlaceSuffix::Index { span, expr } => {
                let actual_base_ty = match &current_ty {
                    ArType::Nullable(inner) => inner.as_ref().clone(),
                    other => other.clone(),
                };
                let elem_ty = match &actual_base_ty {
                    ArType::Array(_, inner) | ArType::Slice(inner) => inner.as_ref().clone(),
                    _ => ArType::Error,
                };
                current_ty = elem_ty.clone();
                suffixes.push(HirPlaceSuffix::Index {
                    span: *span,
                    expr: super::expr::lower_expr(type_check, expr)?,
                    ty: elem_ty,
                });
            }
        }
    }

    Ok(HirPlace {
        root_symbol,
        suffixes: suffixes.into(),
        ty: current_ty,
        span: place.span,
    })
}
