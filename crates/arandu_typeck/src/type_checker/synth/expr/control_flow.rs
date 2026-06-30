use arandu_lexer::Span;
use arandu_parser::ast_pool::{ExprId, ExprKind};

use crate::type_checker::TypeChecker;
use crate::type_checker::constraints::ConstraintOrigin;
use crate::type_checker::synth::check_pattern;
use crate::type_checker::types::{self, ArType};
use super::synth_expr;

#[cold]
#[inline(never)]
pub(super) fn report_unsupported(
    checker: &mut TypeChecker<'_>,
    span: Span,
    feature: &str,
    roadmap: &str,
) {
    checker.diagnostics.push(
        crate::Diagnostic::error(
            crate::DiagCode::U001FeatureNotSupported,
            format!("{feature} is not supported yet ({roadmap})"),
            span,
        )
        .with_hint("see docs/arandu-compiler-roadmap-v0.1.md for the planned milestone"),
    );
}

pub(super) fn synth_control_flow_expr(
    checker: &mut TypeChecker<'_>,
    _expr: ExprId,
    kind: &ExprKind,
    span: Span,
) -> Option<ArType> {
    match kind {
        ExprKind::Lambda { .. } => {
            report_unsupported(
                checker,
                span,
                "lambda/closure",
                "v0.3 LAMBDA: closure type checking and lowering",
            );
            Some(ArType::Error)
        }
        ExprKind::Alloc { expr: inner_expr } => {
            let inner_id = *inner_expr;
            let inner_ty = synth_expr(checker, inner_id);
            let inner_id = checker.intern(inner_ty);
            Some(ArType::Ptr(inner_id))
        }
        ExprKind::AsyncBlock { block } => {
            let block_id = *block;
            let block_ty = crate::type_checker::check::check_block(
                checker,
                checker.pool,
                checker.pool.block(block_id),
            );
            let inner_id = checker.intern(block_ty);
            Some(ArType::Coroutine(inner_id))
        }
        ExprKind::UnsafeBlock { block } => {
            let block_id = *block;
            Some(crate::type_checker::check::check_block(
                checker,
                checker.pool,
                checker.pool.block(block_id),
            ))
        }
        ExprKind::If {
            condition,
            then_block,
            else_block,
        } => {
            let cond = condition.clone();
            let then_id = *then_block;
            let else_id = *else_block;
            crate::type_checker::check::check_condition(checker, &cond);
            let then_ty = crate::type_checker::check::check_block(
                checker,
                checker.pool,
                checker.pool.block(then_id),
            );
            let else_ty = crate::type_checker::check::check_block(
                checker,
                checker.pool,
                checker.pool.block(else_id),
            );
            if !types::unify(&then_ty, &else_ty, &checker.type_info.type_interner) {
                checker.add_constraint(
                    then_ty.clone(),
                    else_ty.clone(),
                    ConstraintOrigin::IfBranches {
                        then_span: checker.pool.block(then_id).span,
                        else_span: checker.pool.block(else_id).span,
                    },
                );
            }
            Some(then_ty)
        }
        ExprKind::Match { value, arms } => {
            let value_id = *value;
            let arms_range = *arms;
            let value_ty = synth_expr(checker, value_id);
            let arm_ids = checker.pool.match_arm_list(arms_range).to_vec();

            let resolved_arms: Vec<arandu_parser::MatchArm> = arm_ids
                .iter()
                .map(|id| checker.pool.match_arm(*id).clone())
                .collect();
            let value_ty_id = checker.type_info.type_interner.intern(value_ty.clone());
            crate::type_checker::synth::match_exhaust::check_match_exhaustiveness(
                checker,
                value_ty_id,
                &resolved_arms,
                span,
            );

            let mut expected_arm_ty = ArType::Error;
            let mut first_arm_span = span;

            for (i, arm_id) in arm_ids.iter().copied().enumerate() {
                let arm = checker.pool.match_arm(arm_id);
                check_pattern(checker, arm.pattern, value_ty_id);
                let arm_ty = match &arm.body {
                    arandu_parser::MatchArmBody::Expr {
                        expr: inner_expr, ..
                    } => synth_expr(checker, *inner_expr),
                    arandu_parser::MatchArmBody::Block { block, .. } => {
                        crate::type_checker::check::check_block(
                            checker,
                            checker.pool,
                            block,
                        )
                    }
                };

                if i == 0 {
                    expected_arm_ty = arm_ty;
                    first_arm_span = arm.span;
                } else if !types::unify(&expected_arm_ty, &arm_ty, &checker.type_info.type_interner) {
                    checker.add_constraint(
                        expected_arm_ty.clone(),
                        arm_ty.clone(),
                        ConstraintOrigin::MatchArms {
                            first_span: first_arm_span,
                            mismatch_span: arm.span,
                            arm_index: i,
                        },
                    );
                }
            }
            Some(expected_arm_ty)
        }
        _ => None,
    }
}
