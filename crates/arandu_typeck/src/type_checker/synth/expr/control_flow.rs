use arandu_lexer::Span;
use arandu_parser::ast_pool::{ExprId, ExprKind};

use super::synth_expr;
use crate::type_checker::TypeChecker;
use crate::type_checker::constraints::ConstraintOrigin;
use crate::type_checker::synth::check_pattern;
use crate::type_checker::types::{self, ArType};

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

use arandu_middle::types::type_interner::TypeId;

#[tracing::instrument(level = "trace", target = "arandu_typeck", skip(checker, _expr))]
pub(super) fn synth_control_flow_expr(
    checker: &mut TypeChecker<'_>,
    _expr: ExprId,
    kind: &ExprKind,
    span: Span,
) -> Option<TypeId> {
    match kind {
        ExprKind::Lambda { .. } => {
            report_unsupported(
                checker,
                span,
                "lambda/closure",
                "v0.3 LAMBDA: closure type checking and lowering",
            );
            Some(checker.intern(ArType::Error))
        }
        ExprKind::Alloc { expr: inner_expr } => {
            if !checker.ctx.is_in_unsafe() {
                checker.diagnostics.push(
                    crate::Diagnostic::error(
                        crate::DiagCode::O012AllocRequiresUnsafe,
                        "`alloc` requires an `unsafe` block",
                        span,
                    )
                    .with_label(span, "`alloc` is unsafe and must be inside an `unsafe` block"),
                );
            }
            let inner_id = *inner_expr;
            let inner_ty_id = synth_expr(checker, inner_id);
            Some(checker.intern(ArType::Ptr(inner_ty_id)))
        }
        ExprKind::AsyncBlock { block } => {
            let block_id = *block;
            let block_ty = crate::type_checker::check::check_block(
                checker,
                checker.pool,
                checker.pool.block(block_id),
            );
            let inner_id = checker.intern(block_ty);
            Some(checker.intern(ArType::Coroutine(inner_id)))
        }
        ExprKind::UnsafeBlock { block } => {
            checker.ctx.enter_unsafe();
            let block_id = *block;
            let block_ty = crate::type_checker::check::check_block(
                checker,
                checker.pool,
                checker.pool.block(block_id),
            );
            checker.ctx.exit_unsafe();
            Some(checker.intern(block_ty))
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
            Some(checker.intern(then_ty))
        }
        ExprKind::Match { value, arms } => {
            let value_id = *value;
            let arms_range = *arms;
            let value_ty_id = synth_expr(checker, value_id);
            let arm_ids = checker.pool.match_arm_list(arms_range).to_vec();

            let resolved_arms: Vec<arandu_parser::MatchArm> = arm_ids
                .iter()
                .map(|id| checker.pool.match_arm(*id).clone())
                .collect();
            crate::type_checker::synth::match_exhaust::check_match_exhaustiveness(
                checker,
                value_ty_id,
                &resolved_arms,
                span,
            );

            let mut expected_arm_ty_id = checker.intern(ArType::Error);
            let mut first_arm_span = span;

            for (i, arm_id) in arm_ids.iter().copied().enumerate() {
                let arm = checker.pool.match_arm(arm_id);
                check_pattern(checker, arm.pattern, value_ty_id);
                let arm_ty_id = match &arm.body {
                    arandu_parser::MatchArmBody::Expr {
                        expr: inner_expr, ..
                    } => synth_expr(checker, *inner_expr),
                    arandu_parser::MatchArmBody::Block { block, .. } => {
                        let block_ty =
                            crate::type_checker::check::check_block(checker, checker.pool, block);
                        checker.intern(block_ty)
                    }
                };

                if i == 0 {
                    expected_arm_ty_id = arm_ty_id;
                    first_arm_span = arm.span;
                } else if !checker.unify_ids(expected_arm_ty_id, arm_ty_id) {
                    let expected_arm_ty = checker.resolve(expected_arm_ty_id).clone();
                    let arm_ty = checker.resolve(arm_ty_id).clone();
                    checker.add_constraint(
                        expected_arm_ty,
                        arm_ty,
                        ConstraintOrigin::MatchArms {
                            first_span: first_arm_span,
                            mismatch_span: arm.span,
                            arm_index: i,
                        },
                    );
                }
            }
            Some(expected_arm_ty_id)
        }
        _ => None,
    }
}
