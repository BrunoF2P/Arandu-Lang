use crate::diagnostics::Diagnostic;
use crate::hir::{
    HirCatchHandler, HirExpr, HirExprKind, HirFieldInit, HirLambdaBody, HirLambdaParam,
};
use crate::passes::lowering::{require_def_symbol, require_type_symbol, require_value_symbol};
use crate::passes::type_checker::types::ArType;
use crate::{NodeKey, TypeCheckResult};
use arandu_parser::CatchHandler;
use arandu_parser::ast_pool::{AstPool, ExprId, ExprKind};

fn get_resolved_value_ref(
    type_check: &TypeCheckResult,
    expr: ExprId,
) -> Option<crate::symbol_table::SymbolId> {
    type_check.resolved.expr_symbol(expr)
}

fn lookup_namespace_field(
    pool: &AstPool,
    base: ExprId,
    field: &str,
    type_check: &TypeCheckResult,
) -> Option<crate::symbol_table::SymbolId> {
    let ExprKind::Path { path, .. } = pool.expr(base) else {
        return None;
    };
    if path.len() != 1 {
        return None;
    }
    type_check.symbols.lookup_module_member(&path[0], field)
}

fn builtin_ctor_variant(pool: &AstPool, callee: ExprId) -> Option<crate::hir::ResultCtorVariant> {
    let ExprKind::TypePath {
        type_name, member, ..
    } = pool.expr(callee)
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

pub(crate) fn lower_expr_raw(
    type_check: &TypeCheckResult,
    pool: &AstPool,
    hir_pool: &mut crate::hir::HirPool,
    expr: ExprId,
) -> Result<HirExpr, Diagnostic> {
    let span = pool.expr_span(expr);
    let fallback_ty = type_check
        .type_info
        .expr_type(expr)
        .cloned()
        .unwrap_or(ArType::Error);

    let kind = match pool.expr(expr) {
        ExprKind::Path { .. } => {
            let symbol = require_value_symbol(&type_check.resolved, expr, span)?;
            HirExprKind::Path { symbol }
        }
        ExprKind::TypePath {
            type_name,
            member: _,
            ..
        } => {
            let type_symbol = require_type_symbol(&type_check.resolved, type_name.span)?;
            let member_symbol = require_value_symbol(&type_check.resolved, expr, span)?;
            HirExprKind::TypePath {
                type_symbol,
                member_symbol,
            }
        }
        ExprKind::Generic { callee, args, .. } => {
            let hir_callee = lower_expr_raw(type_check, pool, hir_pool, *callee)?;
            let mut hir_args = Vec::new();
            let arg_ids = pool.type_expr_list(*args).to_vec();
            for arg_id in arg_ids {
                hir_args.push(crate::passes::type_checker::types::lower_type_expr(
                    pool.type_expr(arg_id),
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
        ExprKind::Field { base, field, .. } => {
            let base_id = *base;
            if let Some(symbol) = lookup_namespace_field(pool, base_id, field, type_check)
                .or_else(|| get_resolved_value_ref(type_check, expr))
            {
                HirExprKind::Path { symbol }
            } else {
                HirExprKind::Field {
                    base: Box::new(lower_expr_raw(type_check, pool, hir_pool, base_id)?),
                    field: field.clone(),
                }
            }
        }
        ExprKind::SafeField { base, field, .. } => {
            let base_id = *base;
            if let Some(symbol) = lookup_namespace_field(pool, base_id, field, type_check)
                .or_else(|| get_resolved_value_ref(type_check, expr))
            {
                HirExprKind::Path { symbol }
            } else {
                HirExprKind::SafeField {
                    base: Box::new(lower_expr_raw(type_check, pool, hir_pool, base_id)?),
                    field: field.clone(),
                }
            }
        }
        ExprKind::Index { base, index, .. } => HirExprKind::Index {
            base: Box::new(lower_expr_raw(type_check, pool, hir_pool, *base)?),
            index: Box::new(lower_expr_raw(type_check, pool, hir_pool, *index)?),
        },
        ExprKind::SafeIndex { base, index, .. } => HirExprKind::SafeIndex {
            base: Box::new(lower_expr_raw(type_check, pool, hir_pool, *base)?),
            index: Box::new(lower_expr_raw(type_check, pool, hir_pool, *index)?),
        },
        ExprKind::Try {
            expr: inner_expr, ..
        } => HirExprKind::Try {
            expr: Box::new(lower_expr_raw(type_check, pool, hir_pool, *inner_expr)?),
        },
        ExprKind::Call {
            callee,
            args,
            trailing_block,
            ..
        } => {
            let callee_id = *callee;
            let arg_ids = pool.expr_list(*args).to_vec();
            if trailing_block.is_none()
                && let Some(variant) = builtin_ctor_variant(pool, callee_id)
                && arg_ids.len() == 1
            {
                let value = Box::new(lower_expr_raw(type_check, pool, hir_pool, arg_ids[0])?);
                let kind = HirExprKind::ResultCtor { variant, value };
                let ty = expr_type_for_kind(type_check, &kind, fallback_ty);
                return Ok(HirExpr { kind, ty, span });
            }
            let method_base = match pool.expr(callee_id) {
                ExprKind::Field { base, field, .. } | ExprKind::SafeField { base, field, .. } => {
                    if lookup_namespace_field(pool, *base, field, type_check).is_some() {
                        None
                    } else {
                        Some(*base)
                    }
                }
                _ => None,
            };
            let hir_callee = lower_expr_raw(type_check, pool, hir_pool, callee_id)?;
            let mut hir_args: Vec<HirExpr> = if let Some(base_id) = method_base {
                vec![lower_expr_raw(type_check, pool, hir_pool, base_id)?]
            } else {
                Vec::new()
            };
            for arg_id in arg_ids {
                hir_args.push(lower_expr_raw(type_check, pool, hir_pool, arg_id)?);
            }
            let hir_trailing = trailing_block
                .as_ref()
                .map(|b| super::stmt::lower_block(type_check, pool, hir_pool, pool.block(*b)))
                .transpose()?;
            HirExprKind::Call {
                callee: Box::new(hir_callee),
                args: hir_args,
                trailing_block: hir_trailing,
            }
        }
        ExprKind::StructLiteral { ty: _, fields, .. } => {
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
            let field_ids = pool.field_init_list(*fields).to_vec();
            let hir_fields: Result<Vec<_>, _> = field_ids
                .iter()
                .map(|fid| {
                    let f = pool.field_init(*fid);
                    Ok(HirFieldInit {
                        span: f.span,
                        name: f.name.clone(),
                        value: lower_expr_raw(type_check, pool, hir_pool, f.value)?,
                    })
                })
                .collect();
            HirExprKind::StructLiteral {
                struct_symbol,
                fields: hir_fields?,
            }
        }
        ExprKind::Array { items, .. } => {
            let item_ids = pool.expr_list(*items).to_vec();
            let hir_items: Result<Vec<_>, _> = item_ids
                .iter()
                .map(|i| lower_expr_raw(type_check, pool, hir_pool, *i))
                .collect();
            HirExprKind::Array { items: hir_items? }
        }
        ExprKind::Lambda { params, body, .. } => {
            let mut hir_params = Vec::new();
            let param_ids = pool.lambda_param_list(*params).to_vec();
            for pid in param_ids {
                let p = pool.lambda_param(pid);
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
                arandu_parser::LambdaBody::Expr {
                    expr: inner_expr, ..
                } => HirLambdaBody::Expr(Box::new(lower_expr_raw(
                    type_check,
                    pool,
                    hir_pool,
                    *inner_expr,
                )?)),
                arandu_parser::LambdaBody::Block { block, .. } => HirLambdaBody::Block(
                    super::stmt::lower_block(type_check, pool, hir_pool, block)?,
                ),
            };
            HirExprKind::Lambda {
                params: hir_params,
                body: hir_body,
            }
        }
        ExprKind::Alloc {
            expr: inner_expr, ..
        } => HirExprKind::Alloc {
            expr: Box::new(lower_expr_raw(type_check, pool, hir_pool, *inner_expr)?),
        },
        ExprKind::AsyncBlock { block, .. } => HirExprKind::AsyncBlock {
            block: super::stmt::lower_block(type_check, pool, hir_pool, pool.block(*block))?,
        },
        ExprKind::UnsafeBlock { block, .. } => HirExprKind::UnsafeBlock {
            block: super::stmt::lower_block(type_check, pool, hir_pool, pool.block(*block))?,
        },
        ExprKind::If {
            condition,
            then_block,
            else_block,
            ..
        } => HirExprKind::If {
            condition: Box::new(super::stmt::lower_condition(
                type_check, pool, hir_pool, condition,
            )?),
            then_block: super::stmt::lower_block(
                type_check,
                pool,
                hir_pool,
                pool.block(*then_block),
            )?,
            else_block: super::stmt::lower_block(
                type_check,
                pool,
                hir_pool,
                pool.block(*else_block),
            )?,
        },
        ExprKind::Match { value, arms, .. } => {
            let arm_ids = pool.match_arm_list(*arms).to_vec();
            HirExprKind::Match {
                value: Box::new(lower_expr_raw(type_check, pool, hir_pool, *value)?),
                arms: super::pattern::lower_match_arms(type_check, pool, hir_pool, &arm_ids)?,
            }
        }
        ExprKind::Catch {
            expr: inner_expr,
            handler,
            ..
        } => {
            let hir_expr = lower_expr_raw(type_check, pool, hir_pool, *inner_expr)?;
            let hir_handler = match pool.catch_handler(*handler) {
                CatchHandler::Expr { expr: h, .. } => {
                    let eid = lower_expr(type_check, pool, hir_pool, *h)?;
                    let e = hir_pool.expr(eid).clone();
                    HirCatchHandler::Expr(Box::new(e))
                }
                CatchHandler::Block { span, error, block } => {
                    let error_symbol = type_check
                        .resolved
                        .definitions
                        .get(&NodeKey::from(*span))
                        .copied();
                    let b = super::stmt::lower_block(type_check, pool, hir_pool, block)?;
                    HirCatchHandler::Block {
                        error_symbol,
                        error_name: Some(error.clone()),
                        block: b,
                    }
                }
            };
            HirExprKind::Catch {
                expr: Box::new(hir_expr),
                handler: hir_handler,
            }
        }
        ExprKind::NullCoalesce { left, right, .. } => HirExprKind::NullCoalesce {
            left: Box::new(lower_expr_raw(type_check, pool, hir_pool, *left)?),
            right: Box::new(lower_expr_raw(type_check, pool, hir_pool, *right)?),
        },
        ExprKind::Cast {
            expr: inner_expr,
            ty: cast_ty,
            ..
        } => {
            let target_ty = crate::passes::type_checker::types::lower_type_expr(
                pool.type_expr(*cast_ty),
                &type_check.symbols,
                crate::ScopeId(0),
                &type_check.resolved,
            );
            HirExprKind::Cast {
                expr: Box::new(lower_expr_raw(type_check, pool, hir_pool, *inner_expr)?),
                target_ty,
            }
        }
        ExprKind::Group {
            expr: inner_expr, ..
        } => {
            return lower_expr_raw(type_check, pool, hir_pool, *inner_expr);
        }
        ExprKind::Unary {
            op,
            expr: inner_expr,
            ..
        } => HirExprKind::Unary {
            op: (*op).into(),
            expr: Box::new(lower_expr_raw(type_check, pool, hir_pool, *inner_expr)?),
        },
        ExprKind::Binary {
            op, left, right, ..
        } => HirExprKind::Binary {
            op: (*op).into(),
            left: Box::new(lower_expr_raw(type_check, pool, hir_pool, *left)?),
            right: Box::new(lower_expr_raw(type_check, pool, hir_pool, *right)?),
        },
        ExprKind::Int { value } => HirExprKind::Int(value.clone()),
        ExprKind::Float { value } => HirExprKind::Float(value.clone()),
        ExprKind::Bool { value } => HirExprKind::Bool(*value),
        ExprKind::Char { value } => HirExprKind::Char(value.clone()),
        ExprKind::InterpolatedString { .. } => HirExprKind::Str("interpolated".to_string()),
        ExprKind::Nil => HirExprKind::Nil,
        ExprKind::Error => unreachable!("syntax error in HIR lowering"),
    };

    let ty = expr_type_for_kind(type_check, &kind, fallback_ty);
    Ok(HirExpr { kind, ty, span })
}

// (removed unused pool-allocation wrapper helper)

/// Lower expression and allocate into a `HirPool`, returning a `HirExprId`.
pub(crate) fn lower_expr(
    type_check: &TypeCheckResult,
    pool: &AstPool,
    hir_pool: &mut crate::hir::HirPool,
    expr: ExprId,
) -> Result<crate::hir::HirExprId, Diagnostic> {
    let hir = lower_expr_raw(type_check, pool, hir_pool, expr)?;
    Ok(hir_pool.alloc_expr(hir))
}
