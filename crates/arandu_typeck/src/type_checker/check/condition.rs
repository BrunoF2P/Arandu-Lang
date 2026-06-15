use arandu_parser::Condition;

use super::super::TypeChecker;
use super::super::constraints::ConstraintOrigin;
use super::super::types::{ArType, Primitive};

pub fn check_condition(checker: &mut TypeChecker<'_>, condition: &Condition) {
    match condition {
        arandu_parser::Condition::Expr { expr, span } => {
            let cond_ty = super::super::synth::synth_expr(checker, *expr);
            if !cond_ty.is_error()
                && !super::super::types::unify(&cond_ty, &ArType::Primitive(Primitive::Bool))
            {
                checker.add_constraint(
                    ArType::Primitive(Primitive::Bool),
                    cond_ty,
                    ConstraintOrigin::Condition { span: *span },
                );
            }
        }
        arandu_parser::Condition::Is {
            expr,
            pattern,
            span: _,
        } => {
            let cond_ty = super::super::synth::synth_expr(checker, *expr);
            super::super::synth::check_pattern(checker, *pattern, &cond_ty);
        }
    }
}
