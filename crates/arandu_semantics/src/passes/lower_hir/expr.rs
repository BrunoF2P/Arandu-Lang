use crate::diagnostics::Diagnostic;
use crate::hir::{
    HirCatchHandler, HirExpr, HirExprKind, HirFieldInit, HirLambdaBody, HirLambdaParam,
};
use crate::passes::lowering::{require_def_symbol, require_type_symbol, require_value_symbol};
use crate::passes::type_checker::types::ArType;
use crate::{NodeKey, TypeCheckResult};
use arandu_parser::{CatchHandler, Expr, LambdaBody};

fn get_resolved_value_ref(
    type_check: &TypeCheckResult,
    span: arandu_lexer::Span,
) -> Option<crate::symbol_table::SymbolId> {
    type_check
        .resolved
        .value_refs
        .get(&NodeKey::from(span))
        .copied()
}

fn lookup_namespace_field(
    base: &Expr,
    field: &str,
    type_check: &TypeCheckResult,
) -> Option<crate::symbol_table::SymbolId> {
    let Expr::Path { path, .. } = base else {
        return None;
    };
    if path.len() != 1 {
        return None;
    }
    type_check.symbols.lookup_module_member(&path[0], field)
}

fn builtin_ctor_variant(callee: &Expr) -> Option<crate::hir::ResultCtorVariant> {
    let Expr::TypePath {
        type_name, member, ..
    } = callee
    else {
        return None;
    };
    let base = type_name
        .path
        .last()
        .map_or("", std::string::String::as_str);
    match (base, member.as_str()) {
        ("Result", "Ok") => Some(crate::hir::ResultCtorVariant::Ok),
        ("Result", "Err") => Some(crate::hir::ResultCtorVariant::Err),
        ("Option", "Some") => Some(crate::hir::ResultCtorVariant::Some),
        _ => None,
    }
}

fn expr_type_for_kind(
    type_check: &TypeCheckResult,
    kind: &HirExprKind,
    fallback: ArType,
) -> ArType {
    use crate::passes::type_checker::types::Primitive;

    match kind {
        HirExprKind::Str(_) => ArType::Primitive(Primitive::Str),
        HirExprKind::Int(_) => ArType::IntLiteral,
        HirExprKind::Float(_) => ArType::FloatLiteral,
        HirExprKind::Bool(_) => ArType::Primitive(Primitive::Bool),
        HirExprKind::Char(_) => ArType::Primitive(Primitive::Char),
        HirExprKind::Nil => {
            if fallback.is_error() {
                ArType::Nullable(Box::new(ArType::Error))
            } else {
                fallback
            }
        }
        HirExprKind::Path { symbol } => type_check
            .type_info
            .decl_type(*symbol)
            .cloned()
            .filter(|ty| !ty.is_error())
            .unwrap_or(fallback),
        HirExprKind::Call { callee, .. } => {
            if !callee.ty.is_error() {
                match &callee.ty {
                    ArType::Func(_, ret) => return ret.as_ref().clone(),
                    other => return other.clone(),
                }
            }
            match &callee.kind {
                HirExprKind::Path { symbol } => type_check
                    .type_info
                    .decl_type(*symbol)
                    .and_then(|ty| match ty {
                        ArType::Func(_, ret) => Some(ret.as_ref().clone()),
                        _ => None,
                    })
                    .unwrap_or(fallback),
                _ => fallback,
            }
        }
        HirExprKind::ResultCtor { .. } => fallback,
        _ => fallback,
    }
}

pub(crate) fn lower_expr(type_check: &TypeCheckResult, expr: &Expr) -> Result<HirExpr, Diagnostic> {
    let span = expr.span();
    let key = NodeKey::from(span);
    let fallback_ty = type_check
        .type_info
        .expr_type(key)
        .cloned()
        .unwrap_or(ArType::Error);

    let kind = match expr {
        Expr::Path { .. } => {
            let symbol = require_value_symbol(&type_check.resolved, span)?;
            HirExprKind::Path { symbol }
        }
        Expr::TypePath {
            type_name,
            member: _,
            ..
        } => {
            let type_symbol = require_type_symbol(&type_check.resolved, type_name.span)?;
            let member_symbol = require_value_symbol(&type_check.resolved, span)?;
            HirExprKind::TypePath {
                type_symbol,
                member_symbol,
            }
        }
        Expr::Generic { callee, args, .. } => {
            let hir_callee = lower_expr(type_check, callee)?;
            let mut hir_args = Vec::new();
            for arg in args {
                hir_args.push(crate::passes::type_checker::types::lower_type_expr(
                    arg,
                    &type_check.symbols,
                    crate::ScopeId(0),
                    &type_check.resolved,
                ));
            }
            HirExprKind::Generic {
                callee: Box::new(hir_callee),
                args: hir_args,
            }
        }
        Expr::Field { base, field, .. } => {
            if let Some(symbol) = lookup_namespace_field(base, field, type_check)
                .or_else(|| get_resolved_value_ref(type_check, span))
            {
                HirExprKind::Path { symbol }
            } else {
                HirExprKind::Field {
                    base: Box::new(lower_expr(type_check, base)?),
                    field: field.clone(),
                }
            }
        }
        Expr::SafeField { base, field, .. } => {
            if let Some(symbol) = lookup_namespace_field(base, field, type_check)
                .or_else(|| get_resolved_value_ref(type_check, span))
            {
                HirExprKind::Path { symbol }
            } else {
                HirExprKind::SafeField {
                    base: Box::new(lower_expr(type_check, base)?),
                    field: field.clone(),
                }
            }
        }
        Expr::Index { base, index, .. } => HirExprKind::Index {
            base: Box::new(lower_expr(type_check, base)?),
            index: Box::new(lower_expr(type_check, index)?),
        },
        Expr::SafeIndex { base, index, .. } => HirExprKind::SafeIndex {
            base: Box::new(lower_expr(type_check, base)?),
            index: Box::new(lower_expr(type_check, index)?),
        },
        Expr::Try { expr, .. } => HirExprKind::Try {
            expr: Box::new(lower_expr(type_check, expr)?),
        },
        Expr::Call {
            callee,
            args,
            trailing_block,
            ..
        } => {
            if trailing_block.is_none()
                && let Some(variant) = builtin_ctor_variant(callee)
                && args.len() == 1
            {
                let value = Box::new(lower_expr(type_check, &args[0])?);
                let kind = HirExprKind::ResultCtor { variant, value };
                let ty = expr_type_for_kind(type_check, &kind, fallback_ty);
                return Ok(HirExpr { kind, ty, span });
            }
            let method_base = match &**callee {
                Expr::Field { base, field, .. } | Expr::SafeField { base, field, .. } => {
                    if lookup_namespace_field(base, field, type_check).is_some() {
                        None
                    } else {
                        Some(&**base)
                    }
                }
                _ => None,
            };
            let hir_callee = lower_expr(type_check, callee)?;
            let mut hir_args: Vec<HirExpr> = if let Some(base) = method_base {
                vec![lower_expr(type_check, base)?]
            } else {
                Vec::new()
            };
            hir_args.extend(
                args.iter()
                    .map(|a| lower_expr(type_check, a))
                    .collect::<Result<Vec<_>, _>>()?,
            );
            let hir_trailing = trailing_block
                .as_ref()
                .map(|b| super::stmt::lower_block(type_check, b))
                .transpose()?;
            HirExprKind::Call {
                callee: Box::new(hir_callee),
                args: hir_args,
                trailing_block: hir_trailing,
            }
        }
        Expr::StructLiteral { fields, .. } => {
            let struct_symbol = match &fallback_ty {
                ArType::Named(id, _) => *id,
                _ => {
                    return Err(Diagnostic::error(
                        crate::diagnostics::DiagCode::L001LoweringUnresolvedSymbol,
                        "cannot lower struct literal: type is not a named struct",
                        span,
                    ));
                }
            };
            let hir_fields: Result<Vec<_>, _> = fields
                .iter()
                .map(|f| {
                    Ok(HirFieldInit {
                        span: f.span,
                        name: f.name.clone(),
                        value: lower_expr(type_check, &f.value)?,
                    })
                })
                .collect();
            HirExprKind::StructLiteral {
                struct_symbol,
                fields: hir_fields?,
            }
        }
        Expr::Array { items, .. } => {
            let hir_items: Result<Vec<_>, _> =
                items.iter().map(|i| lower_expr(type_check, i)).collect();
            HirExprKind::Array { items: hir_items? }
        }
        Expr::Lambda { params, body, .. } => {
            let mut hir_params = Vec::new();
            for p in params {
                let symbol = require_def_symbol(&type_check.resolved, p.span)?;
                let p_ty = type_check
                    .type_info
                    .decl_type(symbol)
                    .cloned()
                    .unwrap_or(ArType::Error);
                hir_params.push(HirLambdaParam {
                    span: p.span,
                    symbol,
                    ty: p_ty,
                });
            }
            let hir_body = match body {
                LambdaBody::Expr { expr, .. } => {
                    HirLambdaBody::Expr(Box::new(lower_expr(type_check, expr)?))
                }
                LambdaBody::Block { block, .. } => {
                    HirLambdaBody::Block(super::stmt::lower_block(type_check, block)?)
                }
            };
            HirExprKind::Lambda {
                params: hir_params,
                body: hir_body,
            }
        }
        Expr::Alloc { expr, .. } => HirExprKind::Alloc {
            expr: Box::new(lower_expr(type_check, expr)?),
        },
        Expr::AsyncBlock { block, .. } => HirExprKind::AsyncBlock {
            block: super::stmt::lower_block(type_check, block)?,
        },
        Expr::UnsafeBlock { block, .. } => HirExprKind::UnsafeBlock {
            block: super::stmt::lower_block(type_check, block)?,
        },
        Expr::If {
            condition,
            then_block,
            else_block,
            ..
        } => HirExprKind::If {
            condition: Box::new(super::stmt::lower_condition(type_check, condition)?),
            then_block: super::stmt::lower_block(type_check, then_block)?,
            else_block: super::stmt::lower_block(type_check, else_block)?,
        },
        Expr::Match { value, arms, .. } => HirExprKind::Match {
            value: Box::new(lower_expr(type_check, value)?),
            arms: super::pattern::lower_match_arms(type_check, arms)?,
        },
        Expr::Catch { expr, handler, .. } => {
            let hir_expr = lower_expr(type_check, expr)?;
            let hir_handler = match handler {
                CatchHandler::Expr { expr, .. } => {
                    HirCatchHandler::Expr(Box::new(lower_expr(type_check, expr)?))
                }
                CatchHandler::Block { span, error, block } => {
                    let error_symbol = type_check
                        .resolved
                        .definitions
                        .get(&NodeKey::from(*span))
                        .copied();
                    HirCatchHandler::Block {
                        error_symbol,
                        error_name: Some(error.clone()),
                        block: super::stmt::lower_block(type_check, block)?,
                    }
                }
            };
            HirExprKind::Catch {
                expr: Box::new(hir_expr),
                handler: hir_handler,
            }
        }
        Expr::NullCoalesce { left, right, .. } => HirExprKind::NullCoalesce {
            left: Box::new(lower_expr(type_check, left)?),
            right: Box::new(lower_expr(type_check, right)?),
        },
        Expr::Cast {
            expr, ty: cast_ty, ..
        } => {
            let target_ty = crate::passes::type_checker::types::lower_type_expr(
                cast_ty,
                &type_check.symbols,
                crate::ScopeId(0),
                &type_check.resolved,
            );
            HirExprKind::Cast {
                expr: Box::new(lower_expr(type_check, expr)?),
                target_ty,
            }
        }
        Expr::Group { expr, .. } => {
            return lower_expr(type_check, expr);
        }
        Expr::Unary { op, expr, .. } => HirExprKind::Unary {
            op: (*op).into(),
            expr: Box::new(lower_expr(type_check, expr)?),
        },
        Expr::Binary {
            op, left, right, ..
        } => HirExprKind::Binary {
            op: (*op).into(),
            left: Box::new(lower_expr(type_check, left)?),
            right: Box::new(lower_expr(type_check, right)?),
        },
        Expr::Int { value, .. } => HirExprKind::Int(value.clone()),
        Expr::Float { value, .. } => HirExprKind::Float(value.clone()),
        Expr::Bool { value, .. } => HirExprKind::Bool(*value),
        Expr::Char { value, .. } => HirExprKind::Char(value.clone()),
        Expr::InterpolatedString { .. } => HirExprKind::Str("interpolated".to_string()),
        Expr::Nil { .. } => HirExprKind::Nil,
        Expr::Error(_) => unreachable!("syntax error in HIR lowering"),
    };

    let ty = expr_type_for_kind(type_check, &kind, fallback_ty);
    Ok(HirExpr { kind, ty, span })
}
