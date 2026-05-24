pub(crate) fn ownership_to_receiver_kind(o: Ownership) -> ReceiverKind {
    match o {
        Ownership::Own => ReceiverKind::Own,
        Ownership::Mut => ReceiverKind::Mut,
        Ownership::Shared => ReceiverKind::Shared,
    }
}

use crate::diagnostics::Diagnostic;
use crate::hir::{
    HirBindingItem, HirBlock, HirCondition, HirForBinding, HirForClause, HirSimpleStmt, HirStmt,
    HirStmtKind, ReceiverKind,
};
use crate::ops::SetOp;
use crate::passes::lowering::require_def_symbol;
use crate::passes::type_checker::types::ArType;
use crate::{NodeKey, TypeCheckResult};
use arandu_lexer::Span;
use arandu_parser::{Block, Condition, DeferBody, Expr, ForClause, Ownership, SimpleStmt, Stmt};

pub(crate) fn lower_block(
    type_check: &TypeCheckResult,
    block: &Block,
) -> Result<HirBlock, Diagnostic> {
    let mut statements = Vec::new();
    for s in &block.statements {
        statements.push(lower_stmt(type_check, s)?);
    }
    Ok(HirBlock {
        statements,
        span: block.span,
    })
}

fn stmt_span(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::VarDecl { span, .. }
        | Stmt::Set { span, .. }
        | Stmt::Return { span, .. }
        | Stmt::Break { span }
        | Stmt::Continue { span }
        | Stmt::Free { span, .. }
        | Stmt::Expr { span, .. }
        | Stmt::If { span, .. }
        | Stmt::For { span, .. }
        | Stmt::While { span, .. }
        | Stmt::Match { span, .. }
        | Stmt::Defer { span, .. }
        | Stmt::ErrDefer { span, .. }
        | Stmt::Unsafe { span, .. }
        | Stmt::Error(span) => *span,
    }
}

fn lower_stmt(type_check: &TypeCheckResult, stmt: &Stmt) -> Result<HirStmt, Diagnostic> {
    let kind = match stmt {
        Stmt::VarDecl {
            bindings, value, ..
        } => {
            let value_ty = type_check
                .type_info
                .expr_type(NodeKey::from(value.span()))
                .cloned();
            let mut hir_bindings = Vec::new();
            for (i, b) in bindings.iter().enumerate() {
                let symbol = require_def_symbol(&type_check.resolved, b.span)?;
                let ty = type_check
                    .type_info
                    .decl_type(symbol)
                    .cloned()
                    .or_else(|| {
                        value_ty.as_ref().and_then(|val_ty| match val_ty {
                            ArType::Tuple(elems) => elems.get(i).cloned(),
                            _ if bindings.len() == 1 => Some(val_ty.clone()),
                            _ => None,
                        })
                    })
                    .unwrap_or(ArType::Error);
                hir_bindings.push(HirBindingItem {
                    symbol,
                    ty,
                    span: b.span,
                });
            }
            HirStmtKind::VarDecl {
                bindings: hir_bindings,
                value: super::expr::lower_expr(type_check, value)?,
            }
        }
        Stmt::Set {
            places, op, value, ..
        } => {
            let hir_places: Result<Vec<_>, _> = places
                .iter()
                .map(|p| super::place::lower_place(type_check, p))
                .collect();
            HirStmtKind::Set {
                places: hir_places?,
                op: SetOp::from(op.clone()),
                value: super::expr::lower_expr(type_check, value)?,
            }
        }
        Stmt::Return { values, .. } => {
            let hir_values: Result<Vec<_>, _> = values
                .iter()
                .map(|v| super::expr::lower_expr(type_check, v))
                .collect();
            HirStmtKind::Return {
                values: hir_values?,
            }
        }
        Stmt::Break { .. } => HirStmtKind::Break,
        Stmt::Continue { .. } => HirStmtKind::Continue,
        Stmt::Free { expr, .. } => HirStmtKind::Free(super::expr::lower_expr(type_check, expr)?),
        Stmt::Expr { expr, .. } => HirStmtKind::Expr(super::expr::lower_expr(type_check, expr)?),
        Stmt::If {
            condition,
            then_block,
            else_block,
            ..
        } => HirStmtKind::If {
            condition: lower_condition(type_check, condition)?,
            then_block: lower_block(type_check, then_block)?,
            else_block: else_block
                .as_ref()
                .map(|b| lower_block(type_check, b))
                .transpose()?,
        },
        Stmt::For { clause, body, .. } => HirStmtKind::For {
            clause: lower_for_clause(type_check, clause)?,
            body: lower_block(type_check, body)?,
        },
        Stmt::While {
            condition, body, ..
        } => HirStmtKind::While {
            condition: lower_condition(type_check, condition)?,
            body: lower_block(type_check, body)?,
        },
        Stmt::Match { expr, .. } => match expr {
            Expr::Match { value, arms, .. } => HirStmtKind::Match {
                value: super::expr::lower_expr(type_check, value)?,
                arms: super::pattern::lower_match_arms(type_check, arms)?,
            },
            other => HirStmtKind::Expr(super::expr::lower_expr(type_check, other)?),
        },
        Stmt::Defer { body, .. } => {
            let block = match body {
                DeferBody::Expr { span, expr } => HirBlock {
                    statements: vec![HirStmt {
                        kind: HirStmtKind::Expr(super::expr::lower_expr(type_check, expr)?),
                        span: *span,
                    }],
                    span: *span,
                },
                DeferBody::Block { block, .. } => lower_block(type_check, block)?,
            };
            HirStmtKind::Defer(block)
        }
        Stmt::ErrDefer { body, .. } => {
            let block = match body {
                DeferBody::Expr { span, expr } => HirBlock {
                    statements: vec![HirStmt {
                        kind: HirStmtKind::Expr(super::expr::lower_expr(type_check, expr)?),
                        span: *span,
                    }],
                    span: *span,
                },
                DeferBody::Block { block, .. } => lower_block(type_check, block)?,
            };
            HirStmtKind::ErrDefer(block)
        }
        Stmt::Unsafe { block, .. } => HirStmtKind::Unsafe(lower_block(type_check, block)?),
        Stmt::Error(_) => unreachable!("syntax error in HIR lowering"),
    };
    Ok(HirStmt {
        kind,
        span: stmt_span(stmt),
    })
}

pub(crate) fn lower_condition(
    type_check: &TypeCheckResult,
    cond: &Condition,
) -> Result<HirCondition, Diagnostic> {
    match cond {
        Condition::Expr { expr, .. } => Ok(HirCondition::Expr(super::expr::lower_expr(
            type_check, expr,
        )?)),
        Condition::Is { expr, pattern, .. } => Ok(HirCondition::Is {
            expr: super::expr::lower_expr(type_check, expr)?,
            pattern: super::pattern::lower_pattern(type_check, pattern)?,
        }),
    }
}

pub(crate) fn lower_for_clause(
    type_check: &TypeCheckResult,
    clause: &ForClause,
) -> Result<HirForClause, Diagnostic> {
    match clause {
        ForClause::In {
            span,
            bindings,
            iterable,
        } => {
            let mut hir_bindings = Vec::new();
            for b in bindings {
                let symbol = require_def_symbol(&type_check.resolved, b.span)?;
                let ty = type_check
                    .type_info
                    .decl_type(symbol)
                    .cloned()
                    .unwrap_or(ArType::Error);
                hir_bindings.push(HirForBinding {
                    symbol,
                    ty,
                    span: b.span,
                });
            }
            Ok(HirForClause::In {
                span: *span,
                bindings: hir_bindings,
                iterable: super::expr::lower_expr(type_check, iterable)?,
            })
        }
        ForClause::CStyle {
            span,
            init,
            condition,
            step,
        } => Ok(HirForClause::CStyle {
            span: *span,
            init: init
                .as_ref()
                .map(|s| lower_simple_stmt(type_check, s))
                .transpose()?,
            condition: condition
                .as_ref()
                .map(|e| super::expr::lower_expr(type_check, e))
                .transpose()?,
            step: step
                .as_ref()
                .map(|s| lower_simple_stmt(type_check, s))
                .transpose()?,
        }),
    }
}

pub(crate) fn lower_simple_stmt(
    type_check: &TypeCheckResult,
    stmt: &SimpleStmt,
) -> Result<HirSimpleStmt, Diagnostic> {
    match stmt {
        SimpleStmt::VarDecl {
            bindings, value, ..
        } => {
            let mut hir_bindings = Vec::new();
            for b in bindings {
                let symbol = require_def_symbol(&type_check.resolved, b.span)?;
                let ty = type_check
                    .type_info
                    .decl_type(symbol)
                    .cloned()
                    .unwrap_or(ArType::Error);
                hir_bindings.push(HirBindingItem {
                    symbol,
                    ty,
                    span: b.span,
                });
            }
            Ok(HirSimpleStmt::VarDecl {
                bindings: hir_bindings,
                value: super::expr::lower_expr(type_check, value)?,
            })
        }
        SimpleStmt::Set {
            places, op, value, ..
        } => {
            let hir_places: Result<Vec<_>, _> = places
                .iter()
                .map(|p| super::place::lower_place(type_check, p))
                .collect();
            Ok(HirSimpleStmt::Set {
                places: hir_places?,
                op: SetOp::from(op.clone()),
                value: super::expr::lower_expr(type_check, value)?,
            })
        }
        SimpleStmt::Expr { expr, .. } => Ok(HirSimpleStmt::Expr(super::expr::lower_expr(
            type_check, expr,
        )?)),
    }
}
