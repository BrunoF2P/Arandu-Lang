use arandu_parser::ast_pool::{ExprId, ExprKind};

use super::super::TypeChecker;
use super::super::constraints::ConstraintOrigin;
use super::super::types::{ArType, Primitive};
use super::expr::synth_expr;

pub(crate) fn resolve_namespace_field(
    checker: &mut TypeChecker<'_>,
    base: ExprId,
    expr: ExprId,
    field: &str,
    _span: arandu_lexer::Span,
) -> Option<ArType> {
    let ExprKind::Path { path } = checker.pool.expr(base) else {
        return None;
    };
    if path.len() != 1 {
        return None;
    }
    let symbol_id = checker.symbols.lookup_module_member(&path[0], field)?;
    checker.resolved.expr_ref(expr, symbol_id);
    if let Some(ty) = checker.ctx.lookup(symbol_id) {
        return Some(ty.clone());
    }
    checker.decl_type(symbol_id)
}

pub(crate) fn resolve_namespace_member_type(
    checker: &TypeChecker<'_>,
    expr: ExprId,
) -> Option<ArType> {
    if let Some(symbol_id) = checker.resolved.expr_symbol(expr) {
        if let Some(ty) = checker.ctx.lookup(symbol_id) {
            return Some(ty.clone());
        }
        if let Some(ty) = checker.decl_type(symbol_id) {
            return Some(ty);
        }
    }
    None
}

pub(crate) fn resolve_field(
    checker: &mut TypeChecker<'_>,
    base: ExprId,
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
        let diag = crate::Diagnostic::error(
            crate::DiagCode::T006NotNullable,
            format!(
                "cannot access field '{}' on nullable type '{}'",
                field,
                base_ty.display(&checker.symbols)
            ),
            field_span,
        )
        .with_label(
            checker.pool.expr_span(base),
            format!("this has type '{}'", base_ty.display(&checker.symbols)),
        )
        .with_hint("use safe access `?.` or make the value non-nullable".to_string());
        checker.diagnostics.push(diag);
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

    let field_ty = if let Some(struct_id) = struct_id_opt {
        if let Some(fields) = checker.type_info.struct_fields.get(&struct_id)
            && let Some(field_ty) = fields.get(field)
        {
            field_ty.clone()
        } else {
            let struct_name = checker.symbols.get(struct_id).name.clone();
            if let Some(method_sym) = checker
                .symbols
                .lookup_associated_member(&struct_name, field)
                && let Some(ArType::Func(params, ret)) = checker.decl_type(method_sym)
            {
                ArType::Func(params, ret)
            } else {
                checker.add_constraint(
                    actual_base_ty.clone(),
                    ArType::Error,
                    ConstraintOrigin::UndefinedField {
                        base_span: checker.pool.expr_span(base),
                        field_span,
                        field_name: field.clone(),
                    },
                );
                return ArType::Error;
            }
        }
    } else {
        checker.add_constraint(
            actual_base_ty.clone(),
            ArType::Error,
            ConstraintOrigin::UndefinedField {
                base_span: checker.pool.expr_span(base),
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

pub(crate) fn resolve_index(
    checker: &mut TypeChecker<'_>,
    base: ExprId,
    index: ExprId,
    safe: bool,
) -> ArType {
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
        let diag = crate::Diagnostic::error(
            crate::DiagCode::T006NotNullable,
            format!(
                "cannot index nullable type '{}'",
                base_ty.display(&checker.symbols)
            ),
            checker.pool.expr_span(index),
        )
        .with_label(
            checker.pool.expr_span(base),
            format!("this has type '{}'", base_ty.display(&checker.symbols)),
        )
        .with_hint("use safe index `?[...]` or make the value non-nullable".to_string());
        checker.diagnostics.push(diag);
        return ArType::Error;
    }

    let elem_ty = match &actual_base_ty {
        ArType::Array(_, inner) | ArType::Slice(inner) => inner.as_ref().clone(),
        _ => {
            checker.add_constraint(
                actual_base_ty.clone(),
                ArType::Error,
                ConstraintOrigin::InvalidIndex {
                    base_span: checker.pool.expr_span(base),
                    index_span: checker.pool.expr_span(index),
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
                base_span: checker.pool.expr_span(base),
                index_span: checker.pool.expr_span(index),
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
