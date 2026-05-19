use crate::{DiagCode, Diagnostic, SymbolTable};

use super::constraints::{Constraint, ConstraintOrigin};
use super::types::ArType;

// ── Flow-based error message generation ─────────────────────────────

/// Convert a failed constraint into a `Diagnostic` with flow-based
/// labels and active hints.
///
/// The key insight: instead of saying "expected X, found Y", we show
/// *where* X came from and *where* Y came from — the flow.
pub fn constraint_to_diagnostic(constraint: &Constraint, symbols: &SymbolTable) -> Diagnostic {
    let expected_str = constraint.expected.display(symbols);
    let found_str = constraint.found.display(symbols);

    match &constraint.origin {
        ConstraintOrigin::Assignment { lhs_span, rhs_span } => Diagnostic::error(
            DiagCode::T002IncompatibleAssignment,
            format!(
                "incompatible type in assignment: expected '{}', found '{}'",
                expected_str, found_str
            ),
            *rhs_span,
        )
        .with_label(*lhs_span, format!("type '{}' declared here", expected_str))
        .with_label(*rhs_span, format!("value has type '{}'", found_str))
        .with_note(format!(
            "flow: declaration ({}, {:?}) → value ({}, {:?})",
            expected_str, lhs_span, found_str, rhs_span
        ))
        .with_hint(format!(
            "convert with '{} as {}' or change the declaration type",
            found_str, expected_str
        )),

        ConstraintOrigin::CallArg {
            call_span,
            param_span,
            arg_span,
            arg_index,
        } => Diagnostic::error(
            DiagCode::T003IncompatibleCallArg,
            format!(
                "incompatible type in argument {}: expected '{}', found '{}'",
                arg_index + 1,
                expected_str,
                found_str,
            ),
            *call_span,
        )
        .with_label(
            *param_span,
            format!("parameter {} expects '{}'", arg_index + 1, expected_str),
        )
        .with_label(*arg_span, format!("argument has type '{}'", found_str))
        .with_note(format!(
            "flow: argument {} ({}) → parameter ({} expected)",
            arg_index + 1,
            found_str,
            expected_str,
        )),

        ConstraintOrigin::ReturnType {
            return_span,
            declared_span,
        } => Diagnostic::error(
            DiagCode::T004IncompatibleReturnType,
            format!(
                "incompatible return type: expected '{}', found '{}'",
                expected_str, found_str
            ),
            *return_span,
        )
        .with_label(
            *declared_span,
            format!("return type '{}' declared here", expected_str),
        )
        .with_label(*return_span, format!("returns '{}'", found_str))
        .with_hint(format!(
            "convert the return value with 'as {}' or change the function's return type",
            expected_str
        )),

        ConstraintOrigin::IfBranches {
            then_span,
            else_span,
        } => Diagnostic::error(
            DiagCode::T007IfBranchMismatch,
            format!(
                "if branches have incompatible types: '{}' and '{}'",
                expected_str, found_str
            ),
            *else_span,
        )
        .with_label(
            *then_span,
            format!("then branch has type '{}'", expected_str),
        )
        .with_label(*else_span, format!("else branch has type '{}'", found_str))
        .with_hint(
            "both branches must have the same type when if is used as an expression".to_string(),
        ),

        ConstraintOrigin::MatchArms {
            first_span,
            mismatch_span,
            arm_index,
        } => Diagnostic::error(
            DiagCode::T008MatchArmMismatch,
            format!(
                "match arm {} has type '{}', expected '{}'",
                arm_index + 1,
                found_str,
                expected_str,
            ),
            *mismatch_span,
        )
        .with_label(
            *first_span,
            format!("first arm has type '{}'", expected_str),
        )
        .with_label(
            *mismatch_span,
            format!("arm {} has type '{}'", arm_index + 1, found_str),
        )
        .with_hint("all match arms must have the same type".to_string()),

        ConstraintOrigin::BinaryOp {
            op_span,
            left_span,
            right_span,
        } => {
            let diag = Diagnostic::error(
                DiagCode::T005OperatorNotApplicable,
                format!(
                    "operator not applicable to '{}' and '{}'",
                    expected_str, found_str,
                ),
                *op_span,
            )
            .with_label(*left_span, format!("type '{}'", expected_str))
            .with_label(*right_span, format!("type '{}'", found_str));

            add_operator_hint(diag, &constraint.expected, &constraint.found, symbols)
        }

        ConstraintOrigin::UnaryOp {
            op_span,
            operand_span,
        } => Diagnostic::error(
            DiagCode::T005OperatorNotApplicable,
            format!("operator not applicable to '{}'", found_str),
            *op_span,
        )
        .with_label(*operand_span, format!("type '{}'", found_str)),

        ConstraintOrigin::Condition { span } => Diagnostic::error(
            DiagCode::T009ConditionNotBool,
            format!("condition must be 'bool', found '{}'", found_str,),
            *span,
        )
        .with_label(*span, format!("type '{}' is not bool", found_str))
        .with_hint(suggest_bool_conversion(&constraint.found)),

        ConstraintOrigin::FieldInit {
            struct_span: _,
            field_name,
            field_span,
            value_span,
        } => Diagnostic::error(
            DiagCode::T002IncompatibleAssignment,
            format!(
                "incompatible type for field '{}': expected '{}', found '{}'",
                field_name, expected_str, found_str,
            ),
            *value_span,
        )
        .with_label(
            *field_span,
            format!("field '{}' has type '{}'", field_name, expected_str),
        )
        .with_label(*value_span, format!("value has type '{}'", found_str)),

        ConstraintOrigin::SetTarget {
            place_span,
            value_span,
        } => Diagnostic::error(
            DiagCode::T002IncompatibleAssignment,
            format!(
                "incompatible type in set: expected '{}', found '{}'",
                expected_str, found_str,
            ),
            *value_span,
        )
        .with_label(*place_span, format!("place has type '{}'", expected_str))
        .with_label(*value_span, format!("value has type '{}'", found_str)),

        ConstraintOrigin::CastExpr {
            expr_span,
            target_span,
        } => Diagnostic::error(
            DiagCode::T010InvalidCast,
            format!("invalid cast from '{}' to '{}'", found_str, expected_str,),
            *target_span,
        )
        .with_label(*expr_span, format!("expression has type '{}'", found_str))
        .with_label(
            *target_span,
            format!("cast to '{}' is not possible", expected_str),
        ),

        ConstraintOrigin::ImplicitWidening {
            source_span,
            target_span,
        } => Diagnostic::error(
            DiagCode::T015ImplicitWidening,
            format!(
                "implicit widening from '{}' to '{}' is not allowed",
                found_str, expected_str,
            ),
            *source_span,
        )
        .with_label(*source_span, format!("type '{}' here", found_str))
        .with_label(*target_span, format!("expected '{}'", expected_str))
        .with_hint(format!(
            "use explicit conversion: 'value as {}'",
            expected_str
        )),

        ConstraintOrigin::TryInvalid { span } => Diagnostic::error(
            DiagCode::T016TryInvalid,
            format!(
                "the '?' operator can only be applied to a Result tuple, found '{}'",
                found_str
            ),
            *span,
        )
        .with_label(*span, format!("this has type '{}'", found_str))
        .with_hint(
            "ensure the expression returns a tuple where the second element is an error type"
                .to_string(),
        ),

        ConstraintOrigin::InvalidIndex {
            base_span,
            index_span,
            is_base_error,
        } => {
            if *is_base_error {
                Diagnostic::error(
                    DiagCode::T017InvalidIndex,
                    format!("type '{}' cannot be indexed", expected_str),
                    *base_span,
                )
                .with_label(
                    *base_span,
                    format!("this has type '{}', expected array or slice", expected_str),
                )
            } else {
                Diagnostic::error(
                    DiagCode::T017InvalidIndex,
                    format!("index must be of type 'int', found '{}'", found_str),
                    *index_span,
                )
                .with_label(*index_span, format!("this has type '{}'", found_str))
            }
        }

        ConstraintOrigin::UndefinedField {
            base_span,
            field_span,
            field_name,
        } => Diagnostic::error(
            DiagCode::T018UndefinedField,
            format!("no field '{}' on type '{}'", field_name, expected_str),
            *field_span,
        )
        .with_label(*base_span, format!("this has type '{}'", expected_str))
        .with_label(*field_span, "unknown field".to_string()),
    }
}

// ── Active hints ────────────────────────────────────────────────────

/// Add operator-specific hints (e.g., suggest interpolation for str+int).
fn add_operator_hint(
    diag: Diagnostic,
    left: &ArType,
    right: &ArType,
    symbols: &SymbolTable,
) -> Diagnostic {
    let left_str = left.display(symbols);
    let right_str = right.display(symbols);

    // str + something → suggest interpolation
    if is_string_type(left) || is_string_type(right) {
        return diag
            .with_hint("to concatenate with string, use interpolation: \"${value}\"".to_string());
    }

    // numeric type mismatch → suggest cast
    if left.is_numeric() && right.is_numeric() {
        return diag.with_hint(format!(
            "convert explicitly: 'value as {}'",
            if left.is_float() { left_str } else { right_str }
        ));
    }

    diag
}

/// Suggest how to convert a value to bool.
fn suggest_bool_conversion(ty: &ArType) -> String {
    match ty {
        ArType::Primitive(p) if p.is_numeric() => {
            "use explicit comparison: 'value != 0'".to_string()
        }
        ArType::Nullable(_) => "use explicit comparison: 'value != nil'".to_string(),
        ArType::Primitive(super::types::Primitive::Str) => {
            "strings are not automatically bool — use explicit comparison".to_string()
        }
        _ => "type is not convertible to bool".to_string(),
    }
}

fn is_string_type(ty: &ArType) -> bool {
    matches!(ty, ArType::Primitive(super::types::Primitive::Str))
}
