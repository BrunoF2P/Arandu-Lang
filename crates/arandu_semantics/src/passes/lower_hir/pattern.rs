use crate::diagnostics::Diagnostic;
use crate::hir::{HirFieldPattern, HirMatchArm, HirMatchArmBody, HirPattern};
use crate::passes::lowering::{require_def_symbol, require_type_symbol};
use crate::{NodeKey, TypeCheckResult};
use arandu_parser::Pattern;
use arandu_parser::{MatchArm, MatchArmBody};

pub(crate) fn lower_pattern(
    type_check: &TypeCheckResult,
    pattern: &Pattern,
) -> Result<HirPattern, Diagnostic> {
    match pattern {
        Pattern::Wildcard { span } => Ok(HirPattern::Wildcard { span: *span }),
        Pattern::Bind { span, name } => {
            let symbol = require_def_symbol(&type_check.resolved, *span)?;
            Ok(HirPattern::Bind {
                span: *span,
                name: name.clone(),
                symbol,
            })
        }
        Pattern::Literal { span, expr } => Ok(HirPattern::Literal {
            span: *span,
            expr: Box::new(super::expr::lower_expr(type_check, expr)?),
        }),
        Pattern::Enum {
            span,
            type_name,
            variant,
            payload,
        } => {
            let type_symbol = require_type_symbol(&type_check.resolved, type_name.span)?;
            let variant_symbol = type_check
                .resolved
                .definitions
                .get(&NodeKey::from(*span))
                .copied();
            let mut hir_payload = Vec::new();
            for p in payload {
                hir_payload.push(lower_pattern(type_check, p)?);
            }
            Ok(HirPattern::Enum {
                span: *span,
                type_symbol,
                variant: variant.clone(),
                variant_symbol,
                payload: hir_payload,
            })
        }
        Pattern::TypeTuple {
            span,
            name,
            payload,
        } => {
            let mut hir_payload = Vec::new();
            for p in payload {
                hir_payload.push(lower_pattern(type_check, p)?);
            }
            Ok(HirPattern::TypeTuple {
                span: *span,
                name: name.clone(),
                payload: hir_payload,
            })
        }
        Pattern::Struct {
            span,
            type_name,
            fields,
        } => {
            let struct_symbol = require_type_symbol(&type_check.resolved, type_name.span)?;
            let mut hir_fields = Vec::new();
            for f in fields {
                hir_fields.push(HirFieldPattern {
                    span: f.span,
                    name: f.name.clone(),
                    pattern: match f.pattern.as_ref() {
                        Some(p) => Some(Box::new(lower_pattern(type_check, p)?)),
                        None => None,
                    },
                });
            }
            Ok(HirPattern::Struct {
                span: *span,
                struct_symbol,
                fields: hir_fields,
            })
        }
        Pattern::Tuple { span, items } => {
            let mut hir_items = Vec::new();
            for p in items {
                hir_items.push(lower_pattern(type_check, p)?);
            }
            Ok(HirPattern::Tuple {
                span: *span,
                items: hir_items,
            })
        }
        Pattern::Range {
            span,
            start,
            inclusive,
            end,
        } => Ok(HirPattern::Range {
            span: *span,
            start: Box::new(super::expr::lower_expr(type_check, start)?),
            inclusive: *inclusive,
            end: Box::new(super::expr::lower_expr(type_check, end)?),
        }),
    }
}

pub(crate) fn lower_match_arms(
    type_check: &TypeCheckResult,
    arms: &[MatchArm],
) -> Result<Vec<HirMatchArm>, Diagnostic> {
    let mut hir_arms = Vec::new();
    for arm in arms {
        let guard = arm
            .guard
            .as_ref()
            .map(|g| super::expr::lower_expr(type_check, g))
            .transpose()?;
        let body = match &arm.body {
            MatchArmBody::Expr { expr, .. } => {
                HirMatchArmBody::Expr(Box::new(super::expr::lower_expr(type_check, expr)?))
            }
            MatchArmBody::Block { block, .. } => {
                HirMatchArmBody::Block(super::stmt::lower_block(type_check, block)?)
            }
        };
        hir_arms.push(HirMatchArm {
            span: arm.span,
            pattern: lower_pattern(type_check, &arm.pattern)?,
            guard,
            body,
        });
    }
    Ok(hir_arms)
}
