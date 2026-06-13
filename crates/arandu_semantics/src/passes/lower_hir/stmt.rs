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
    HirBindingItem, HirBlock, HirBlockId, HirCondition, HirForBinding, HirForClause, HirSimpleStmt,
    HirStmt, HirStmtId, HirStmtKind, ReceiverKind,
};
use crate::ops::SetOp;
use crate::passes::lowering::require_def_symbol;
use crate::passes::type_checker::types::ArType;
use arandu_lexer::Span;
use arandu_parser::ast_pool::{AstPool, ExprKind, StmtId};
use arandu_parser::{Block, Condition, DeferBody, ForClause, Ownership, SimpleStmt, Stmt};

pub(crate) fn lower_block_raw(
    type_check: &TypeCheckResult,
    pool: &AstPool,
    hir_pool: &mut crate::hir::HirPool,
    block: &Block,
) -> Result<HirBlock, Diagnostic> {
    let mut statements = Vec::new();
    for s in &block.statements {
        statements.push(lower_stmt(type_check, pool, hir_pool, *s)?);
    }
    Ok(HirBlock {
        statements,
        span: block.span,
    })
}

pub(crate) fn lower_block(
    type_check: &TypeCheckResult,
    pool: &AstPool,
    hir_pool: &mut crate::hir::HirPool,
    block: &Block,
) -> Result<HirBlockId, Diagnostic> {
    let hir = lower_block_raw(type_check, pool, hir_pool, block)?;
    Ok(hir_pool.alloc_block(hir))
}

// (removed unused pool-allocation wrapper helpers)

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

fn lower_stmt_raw(
    type_check: &TypeCheckResult,
    pool: &AstPool,
    hir_pool: &mut crate::hir::HirPool,
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
            let value_vid = super::expr::lower_expr(type_check, pool, hir_pool, *value)?;
            let value_hir = hir_pool.expr(value_vid).clone();
            HirStmtKind::VarDecl {
                bindings: hir_bindings,
                value: value_hir,
            }
        }
        Stmt::Set {
            places, op, value, ..
        } => {
            let hir_places: Result<Vec<_>, Diagnostic> = places
                .iter()
                .map(|p| super::place::lower_place(type_check, pool, hir_pool, p))
                .collect();
            let value_vid = super::expr::lower_expr(type_check, pool, hir_pool, *value)?;
            let value_hir = hir_pool.expr(value_vid).clone();
            HirStmtKind::Set {
                places: hir_places?,
                op: SetOp::from(op.clone()),
                value: value_hir,
            }
        }
        Stmt::Return { values, .. } => {
            let hir_values: Result<Vec<_>, Diagnostic> = values
                .iter()
                .map(|v| {
                    let vid = super::expr::lower_expr(type_check, pool, hir_pool, *v)?;
                    Ok(hir_pool.expr(vid).clone())
                })
                .collect();
            HirStmtKind::Return {
                values: hir_values?,
            }
        }
        Stmt::Break { .. } => HirStmtKind::Break,
        Stmt::Continue { .. } => HirStmtKind::Continue,
        Stmt::Free { expr, .. } => {
            let eid = super::expr::lower_expr(type_check, pool, hir_pool, *expr)?;
            let e = hir_pool.expr(eid).clone();
            HirStmtKind::Free(e)
        }
        Stmt::Expr { expr, .. } => {
            let eid = super::expr::lower_expr(type_check, pool, hir_pool, **expr)?;
            let e = hir_pool.expr(eid).clone();
            HirStmtKind::Expr(e)
        }
        Stmt::If {
            condition,
            then_block,
            else_block,
            ..
        } => HirStmtKind::If {
            condition: lower_condition(type_check, pool, hir_pool, condition)?,
            then_block: super::stmt::lower_block(type_check, pool, hir_pool, then_block)?,
            else_block: else_block
                .as_ref()
                .map(|b| super::stmt::lower_block(type_check, pool, hir_pool, b))
                .transpose()?,
        },
        Stmt::For { clause, body, .. } => HirStmtKind::For {
            clause: Box::new(lower_for_clause(type_check, pool, hir_pool, clause)?),
            body: super::stmt::lower_block(type_check, pool, hir_pool, body)?,
        },
        Stmt::While {
            condition, body, ..
        } => HirStmtKind::While {
            condition: lower_condition(type_check, pool, hir_pool, condition)?,
            body: super::stmt::lower_block(type_check, pool, hir_pool, body)?,
        },
        Stmt::Match { expr, .. } => {
            let expr_id = *expr;
            match pool.expr(expr_id) {
                ExprKind::Match { value, arms } => {
                    let arm_ids = pool.match_arm_list(*arms).to_vec();
                    let vid = super::expr::lower_expr(type_check, pool, hir_pool, *value)?;
                    let value_hir = hir_pool.expr(vid).clone();
                    HirStmtKind::Match {
                        value: value_hir,
                        arms: super::pattern::lower_match_arms(
                            type_check, pool, hir_pool, &arm_ids,
                        )?,
                    }
                }
                _ => {
                    let eid = super::expr::lower_expr(type_check, pool, hir_pool, expr_id)?;
                    HirStmtKind::Expr(hir_pool.expr(eid).clone())
                }
            }
        }
        Stmt::Defer { body, .. } => {
            let block = match body {
                DeferBody::Expr { span, expr } => {
                    let eid = super::expr::lower_expr(type_check, pool, hir_pool, **expr)?;
                    let e = hir_pool.expr(eid).clone();
                    let stmt_id = hir_pool.alloc_stmt(HirStmt {
                        kind: HirStmtKind::Expr(e),
                        span: *span,
                    });
                    hir_pool.alloc_block(HirBlock {
                        statements: vec![stmt_id],
                        span: *span,
                    })
                }
                DeferBody::Block { block, .. } => {
                    super::stmt::lower_block(type_check, pool, hir_pool, block)?
                }
            };
            HirStmtKind::Defer(block)
        }
        Stmt::ErrDefer { body, .. } => {
            let block = match body {
                DeferBody::Expr { span, expr } => {
                    let eid = super::expr::lower_expr(type_check, pool, hir_pool, **expr)?;
                    let e = hir_pool.expr(eid).clone();
                    let stmt_id = hir_pool.alloc_stmt(HirStmt {
                        kind: HirStmtKind::Expr(e),
                        span: *span,
                    });
                    hir_pool.alloc_block(HirBlock {
                        statements: vec![stmt_id],
                        span: *span,
                    })
                }
                DeferBody::Block { block, .. } => {
                    super::stmt::lower_block(type_check, pool, hir_pool, block)?
                }
            };
            HirStmtKind::ErrDefer(block)
        }
        Stmt::Unsafe { block, .. } => {
            HirStmtKind::Unsafe(super::stmt::lower_block(type_check, pool, hir_pool, block)?)
        }
        Stmt::Error(_) => unreachable!("syntax error in HIR lowering"),
    };
    Ok(HirStmt {
        kind,
        span: stmt_span(pool, stmt),
    })
}

pub(crate) fn lower_stmt(
    type_check: &TypeCheckResult,
    pool: &AstPool,
    hir_pool: &mut crate::hir::HirPool,
    stmt: StmtId,
) -> Result<HirStmtId, Diagnostic> {
    let hir = lower_stmt_raw(type_check, pool, hir_pool, stmt)?;
    Ok(hir_pool.alloc_stmt(hir))
}

pub(crate) fn lower_condition(
    type_check: &TypeCheckResult,
    pool: &AstPool,
    hir_pool: &mut crate::hir::HirPool,
    cond: &Condition,
) -> Result<HirCondition, Diagnostic> {
    match cond {
        Condition::Expr { expr, .. } => {
            let eid = super::expr::lower_expr(type_check, pool, hir_pool, **expr)?;
            Ok(HirCondition::Expr(hir_pool.expr(eid).clone()))
        }
        Condition::Is { expr, pattern, .. } => {
            let eid = super::expr::lower_expr(type_check, pool, hir_pool, **expr)?;
            Ok(HirCondition::Is {
                expr: hir_pool.expr(eid).clone(),
                pattern: super::pattern::lower_pattern(type_check, pool, hir_pool, pattern)?,
            })
        }
    }
}

pub(crate) fn lower_for_clause(
    type_check: &TypeCheckResult,
    pool: &AstPool,
    hir_pool: &mut crate::hir::HirPool,
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
                iterable: Box::new({
                    let eid = super::expr::lower_expr(type_check, pool, hir_pool, **iterable)?;
                    hir_pool.expr(eid).clone()
                }),
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
                .map(|s| lower_simple_stmt(type_check, pool, hir_pool, s))
                .transpose()?
                .map(Box::new),
            condition: condition
                .as_ref()
                .map(|e| -> Result<_, Diagnostic> {
                    let eid = super::expr::lower_expr(type_check, pool, hir_pool, **e)?;
                    Ok(hir_pool.expr(eid).clone())
                })
                .transpose()?
                .map(Box::new),
            step: step
                .as_ref()
                .map(|s| lower_simple_stmt(type_check, pool, hir_pool, s))
                .transpose()?
                .map(Box::new),
        }),
    }
}

pub(crate) fn lower_simple_stmt(
    type_check: &TypeCheckResult,
    pool: &AstPool,
    hir_pool: &mut crate::hir::HirPool,
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
            let vid = super::expr::lower_expr(type_check, pool, hir_pool, *value)?;
            Ok(HirSimpleStmt::VarDecl {
                bindings: hir_bindings,
                value: hir_pool.expr(vid).clone(),
            })
        }
        SimpleStmt::Set {
            places, op, value, ..
        } => {
            let hir_places: Result<Vec<_>, Diagnostic> = places
                .iter()
                .map(|p| super::place::lower_place(type_check, pool, hir_pool, p))
                .collect();
            let vid = super::expr::lower_expr(type_check, pool, hir_pool, *value)?;
            let value_hir = hir_pool.expr(vid).clone();
            Ok(HirSimpleStmt::Set {
                places: hir_places?,
                op: SetOp::from(op.clone()),
                value: value_hir,
            })
        }
        SimpleStmt::Expr { expr, .. } => {
            let eid = super::expr::lower_expr(type_check, pool, hir_pool, **expr)?;
            let e = hir_pool.expr(eid).clone();
            Ok(HirSimpleStmt::Expr(e))
        }
    }
}
