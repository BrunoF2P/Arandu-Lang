mod binary;
mod call;
mod control_flow;
mod literal;

use arandu_parser::ast_pool::{ExprId, ExprKind};

use super::super::TypeChecker;
use super::super::types::ArType;

use binary::synth_binary_unary_expr;
use call::synth_call_expr;
use control_flow::synth_control_flow_expr;
use literal::synth_literal_expr;

use arandu_middle::types::type_interner::TypeId;

pub fn synth_expr(checker: &mut TypeChecker<'_>, expr: ExprId) -> TypeId {
    let id = synth_expr_inner(checker, expr);
    checker.record_expr_type(expr, id);
    id
}

fn synth_expr_inner(checker: &mut TypeChecker<'_>, expr: ExprId) -> TypeId {
    let span = checker.pool.expr_span(expr);
    let kind = checker.pool.expr(expr).clone();

    if let Some(id) = synth_literal_expr(checker, expr, &kind, span) {
        return id;
    }
    if let Some(id) = synth_call_expr(checker, expr, &kind, span) {
        return id;
    }
    if let Some(id) = synth_binary_unary_expr(checker, expr, &kind, span) {
        return id;
    }
    if let Some(id) = synth_control_flow_expr(checker, expr, &kind, span) {
        return id;
    }

    match kind {
        ExprKind::Group { expr: inner_expr } => synth_expr(checker, inner_expr),
        ExprKind::Error => checker.intern(ArType::Error),
        _ => checker.intern(ArType::Error),
    }
}
