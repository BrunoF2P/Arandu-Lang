pub(crate) fn ownership_to_receiver_kind(o: Ownership) -> ReceiverKind {
    match o {
        Ownership::Own => ReceiverKind::Own,
        Ownership::Mut => ReceiverKind::Mut,
        Ownership::Shared => ReceiverKind::Shared,
    }
}

use crate::TypeCheckResult;
use crate::diagnostics::Diagnostic;
use crate::hir::{
    HirBindingItem, HirBlock, HirCondition, HirForBinding, HirForClause, HirSimpleStmt, HirStmt,
    HirStmtKind, ReceiverKind,
};
use crate::ops::SetOp;
use crate::passes::lowering::require_def_symbol;
use crate::passes::type_checker::types::ArType;
use arandu_lexer::Span;
use arandu_parser::ast_pool::{AstPool, ExprKind, StmtId};
use arandu_parser::{Block, Condition, DeferBody, ForClause, Ownership, SimpleStmt, Stmt};

pub(crate) fn lower_block(
    type_check: &TypeCheckResult,
    pool: &AstPool,
    block: &Block,
) -> Result<HirBlock, Diagnostic> {
    let mut statements = Vec::new();
    for s in &block.statements {
        statements.push(lower_stmt(type_check, pool, *s)?);
    }
    Ok(HirBlock {
        statements,
        span: block.span,
    })
}

fn stmt_span(pool: &AstPool, stmt: StmtId) -> Span {
    match pool.stmt(stmt) {
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

fn lower_stmt(
    type_check: &TypeCheckResult,
    pool: &AstPool,
    stmt: StmtId,
) -> Result<HirStmt, Diagnostic> {
    let stmt_ref = pool.stmt(stmt);
    let kind = match stmt_ref {
        Stmt::VarDecl {
            bindings, value, ..
        } => {
            let value_ty = type_check.type_info.expr_type(*value).cloned();
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
                value: super::expr::lower_expr(type_check, pool, *value)?,
            }
        }
        Stmt::Set {
            places, op, value, ..
        } => {
            let hir_places: Result<Vec<_>, _> = places
                .iter()
                .map(|p| super::place::lower_place(type_check, pool, p))
                .collect();
            HirStmtKind::Set {
                places: hir_places?,
                op: SetOp::from(op.clone()),
                value: super::expr::lower_expr(type_check, pool, *value)?,
            }
        }
        Stmt::Return { values, .. } => {
            let hir_values: Result<Vec<_>, _> = values
                .iter()
                .map(|v| super::expr::lower_expr(type_check, pool, *v))
                .collect();
            HirStmtKind::Return {
                values: hir_values?,
            }
        }
        Stmt::Break { .. } => HirStmtKind::Break,
        Stmt::Continue { .. } => HirStmtKind::Continue,
        Stmt::Free { expr, .. } => {
            HirStmtKind::Free(super::expr::lower_expr(type_check, pool, *expr)?)
        }
        Stmt::Expr { expr, .. } => {
            HirStmtKind::Expr(super::expr::lower_expr(type_check, pool, **expr)?)
        }
        Stmt::If {
            condition,
            then_block,
            else_block,
            ..
        } => HirStmtKind::If {
            condition: lower_condition(type_check, pool, condition)?,
            then_block: Box::new(lower_block(type_check, pool, then_block)?),
            else_block: else_block
                .as_ref()
                .map(|b| lower_block(type_check, pool, b))
                .transpose()?
                .map(Box::new),
        },
        Stmt::For { clause, body, .. } => HirStmtKind::For {
            clause: Box::new(lower_for_clause(type_check, pool, clause)?),
            body: Box::new(lower_block(type_check, pool, body)?),
        },
        Stmt::While {
            condition, body, ..
        } => HirStmtKind::While {
            condition: lower_condition(type_check, pool, condition)?,
            body: Box::new(lower_block(type_check, pool, body)?),
        },
        Stmt::Match { expr, .. } => {
            let expr_id = *expr;
            match pool.expr(expr_id) {
                ExprKind::Match { value, arms } => {
                    let arm_ids = pool.match_arm_list(*arms).to_vec();
                    HirStmtKind::Match {
                        value: super::expr::lower_expr(type_check, pool, *value)?,
                        arms: super::pattern::lower_match_arms(type_check, pool, &arm_ids)?,
                    }
                }
                _ => HirStmtKind::Expr(super::expr::lower_expr(type_check, pool, expr_id)?),
            }
        }
        Stmt::Defer { body, .. } => {
            let block = match body {
                DeferBody::Expr { span, expr } => HirBlock {
                    statements: vec![HirStmt {
                        kind: HirStmtKind::Expr(super::expr::lower_expr(type_check, pool, **expr)?),
                        span: *span,
                    }],
                    span: *span,
                },
                DeferBody::Block { block, .. } => lower_block(type_check, pool, block)?,
            };
            HirStmtKind::Defer(Box::new(block))
        }
        Stmt::ErrDefer { body, .. } => {
            let block = match body {
                DeferBody::Expr { span, expr } => HirBlock {
                    statements: vec![HirStmt {
                        kind: HirStmtKind::Expr(super::expr::lower_expr(type_check, pool, **expr)?),
                        span: *span,
                    }],
                    span: *span,
                },
                DeferBody::Block { block, .. } => lower_block(type_check, pool, block)?,
            };
            HirStmtKind::ErrDefer(Box::new(block))
        }
        Stmt::Unsafe { block, .. } => {
            HirStmtKind::Unsafe(Box::new(lower_block(type_check, pool, block)?))
        }
        Stmt::Error(_) => unreachable!("syntax error in HIR lowering"),
    };
    Ok(HirStmt {
        kind,
        span: stmt_span(pool, stmt),
    })
}

pub(crate) fn lower_condition(
    type_check: &TypeCheckResult,
    pool: &AstPool,
    cond: &Condition,
) -> Result<HirCondition, Diagnostic> {
    match cond {
        Condition::Expr { expr, .. } => Ok(HirCondition::Expr(super::expr::lower_expr(
            type_check, pool, **expr,
        )?)),
        Condition::Is { expr, pattern, .. } => Ok(HirCondition::Is {
            expr: super::expr::lower_expr(type_check, pool, **expr)?,
            pattern: super::pattern::lower_pattern(type_check, pool, pattern)?,
        }),
    }
}

pub(crate) fn lower_for_clause(
    type_check: &TypeCheckResult,
    pool: &AstPool,
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
                iterable: Box::new(super::expr::lower_expr(type_check, pool, **iterable)?),
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
                .map(|s| lower_simple_stmt(type_check, pool, s))
                .transpose()?
                .map(Box::new),
            condition: condition
                .as_ref()
                .map(|e| super::expr::lower_expr(type_check, pool, **e))
                .transpose()?
                .map(Box::new),
            step: step
                .as_ref()
                .map(|s| lower_simple_stmt(type_check, pool, s))
                .transpose()?
                .map(Box::new),
        }),
    }
}

pub(crate) fn lower_simple_stmt(
    type_check: &TypeCheckResult,
    pool: &AstPool,
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
                value: super::expr::lower_expr(type_check, pool, *value)?,
            })
        }
        SimpleStmt::Set {
            places, op, value, ..
        } => {
            let hir_places: Result<Vec<_>, _> = places
                .iter()
                .map(|p| super::place::lower_place(type_check, pool, p))
                .collect();
            Ok(HirSimpleStmt::Set {
                places: hir_places?,
                op: SetOp::from(op.clone()),
                value: super::expr::lower_expr(type_check, pool, *value)?,
            })
        }
        SimpleStmt::Expr { expr, .. } => Ok(HirSimpleStmt::Expr(super::expr::lower_expr(
            type_check, pool, **expr,
        )?)),
    }
}
