//! Pure scalar operations and bounded AMIR compile-time execution.
//!
//! This module does not execute queries, inspect the host, or interpret AST.
//! The interpreter consumes typed function units with explicit budgets and a
//! cancellation hook. Public `comptime` syntax/materialization remain separate
//! steps. Operations accept typed values; coercions belong to type checking.

use arandu_middle::ctfe::{ConstInt, ConstValue, IntegerType};
use arandu_middle::ops::{BinaryOp, UnaryOp};

mod vm;
pub use vm::{Budget, CtfeFunction, EvalError, EvalErrorKind, FunctionProvider, evaluate};

/// Internal arithmetic failure; the evaluation boundary adds source locations
/// and chooses the existing or new public diagnostic for the specific cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarEvalError {
    TypeMismatch,
    Overflow(IntegerType),
    DivisionByZero,
    InvalidShift { amount: i128, bit_width: u8 },
    UnsupportedBinary(BinaryOp),
    UnsupportedUnary(UnaryOp),
}

/// Evaluate one admitted unary operation without silent wrapping.
pub fn eval_unary(op: UnaryOp, value: ConstValue) -> Result<ConstValue, ScalarEvalError> {
    match (op, value) {
        (UnaryOp::Not, ConstValue::Bool(value)) => Ok(ConstValue::Bool(!value)),
        (UnaryOp::Neg, ConstValue::Integer(value)) => {
            integer_result(value.ty(), value.value().checked_neg())
        }
        (UnaryOp::BitNot, ConstValue::Integer(value)) => {
            let complemented = if value.ty().is_signed() {
                !value.value()
            } else {
                // An unsigned complement is confined to the declared width,
                // rather than the 128-bit representation used by the compiler.
                value.value() ^ value.ty().max()
            };
            integer_result(value.ty(), Some(complemented))
        }
        (UnaryOp::Not | UnaryOp::Neg | UnaryOp::BitNot, _) => Err(ScalarEvalError::TypeMismatch),
        (UnaryOp::Await | UnaryOp::Ref | UnaryOp::RefMut | UnaryOp::Deref, _) => {
            Err(ScalarEvalError::UnsupportedUnary(op))
        }
        // UnaryOp is non_exhaustive across crates. A future operator must fail
        // closed until CTFE explicitly implements and tests its semantics.
        _ => Err(ScalarEvalError::UnsupportedUnary(op)),
    }
}

/// Evaluate a typed AMIR scalar operation. Both operands have already been
/// evaluated; short-circuit control flow is the interpreter's responsibility.
pub fn eval_binary(
    op: BinaryOp,
    left: ConstValue,
    right: ConstValue,
) -> Result<ConstValue, ScalarEvalError> {
    if matches!(
        op,
        BinaryOp::NullCoalesce | BinaryOp::RangeExclusive | BinaryOp::RangeInclusive
    ) {
        return Err(ScalarEvalError::UnsupportedBinary(op));
    }
    match (left, right) {
        (ConstValue::Bool(left), ConstValue::Bool(right)) => match op {
            BinaryOp::And => Ok(ConstValue::Bool(left && right)),
            BinaryOp::Or => Ok(ConstValue::Bool(left || right)),
            BinaryOp::Equal => Ok(ConstValue::Bool(left == right)),
            BinaryOp::NotEqual => Ok(ConstValue::Bool(left != right)),
            BinaryOp::Lt
            | BinaryOp::Gt
            | BinaryOp::LtEqual
            | BinaryOp::GtEqual
            | BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::BitAnd
            | BinaryOp::ShiftLeft
            | BinaryOp::ShiftRight => Err(ScalarEvalError::TypeMismatch),
            BinaryOp::NullCoalesce | BinaryOp::RangeExclusive | BinaryOp::RangeInclusive => {
                Err(ScalarEvalError::UnsupportedBinary(op))
            }
            _ => Err(ScalarEvalError::UnsupportedBinary(op)),
        },
        (ConstValue::Integer(left), ConstValue::Integer(right)) => eval_integers(op, left, right),
        _ => Err(ScalarEvalError::TypeMismatch),
    }
}

fn eval_integers(
    op: BinaryOp,
    left: ConstInt,
    right: ConstInt,
) -> Result<ConstValue, ScalarEvalError> {
    let ty = left.ty();
    let a = left.value();
    let b = right.value();
    if matches!(op, BinaryOp::ShiftLeft | BinaryOp::ShiftRight) {
        let amount = u32::try_from(b)
            .ok()
            .filter(|amount| *amount < u32::from(ty.bit_width()))
            .ok_or(ScalarEvalError::InvalidShift {
                amount: b,
                bit_width: ty.bit_width(),
            })?;
        let result = if matches!(op, BinaryOp::ShiftLeft) {
            // Checked mathematical shift: refuse any discarded significant bit.
            a.checked_mul(1_i128 << amount)
        } else {
            // Negative mathematical integers use arithmetic shift; unsigned
            // values are nonnegative, so the same operation is a logical shift.
            a.checked_shr(amount)
        };
        return integer_result(ty, result);
    }
    if ty != right.ty() {
        return Err(ScalarEvalError::TypeMismatch);
    }
    let result = match op {
        BinaryOp::Equal => return Ok(ConstValue::Bool(a == b)),
        BinaryOp::NotEqual => return Ok(ConstValue::Bool(a != b)),
        BinaryOp::Lt => return Ok(ConstValue::Bool(a < b)),
        BinaryOp::Gt => return Ok(ConstValue::Bool(a > b)),
        BinaryOp::LtEqual => return Ok(ConstValue::Bool(a <= b)),
        BinaryOp::GtEqual => return Ok(ConstValue::Bool(a >= b)),
        BinaryOp::Add => a.checked_add(b),
        BinaryOp::Sub => a.checked_sub(b),
        BinaryOp::Mul => a.checked_mul(b),
        BinaryOp::Div | BinaryOp::Mod => {
            if b == 0 {
                return Err(ScalarEvalError::DivisionByZero);
            }
            // Both division and remainder overflow for signed MIN / -1.
            if ty.is_signed() && a == ty.min() && b == -1 {
                return Err(ScalarEvalError::Overflow(ty));
            }
            if matches!(op, BinaryOp::Div) {
                a.checked_div(b)
            } else {
                a.checked_rem(b)
            }
        }
        BinaryOp::BitOr => Some(a | b),
        BinaryOp::BitXor => Some(a ^ b),
        BinaryOp::BitAnd => Some(a & b),
        BinaryOp::And | BinaryOp::Or => return Err(ScalarEvalError::TypeMismatch),
        BinaryOp::ShiftLeft
        | BinaryOp::ShiftRight
        | BinaryOp::NullCoalesce
        | BinaryOp::RangeExclusive
        | BinaryOp::RangeInclusive => return Err(ScalarEvalError::UnsupportedBinary(op)),
        // BinaryOp is non_exhaustive; new operators are rejected explicitly.
        _ => return Err(ScalarEvalError::UnsupportedBinary(op)),
    };
    integer_result(ty, result)
}

fn integer_result(ty: IntegerType, result: Option<i128>) -> Result<ConstValue, ScalarEvalError> {
    let value = result.ok_or(ScalarEvalError::Overflow(ty))?;
    ConstInt::new(ty, value)
        .map(ConstValue::Integer)
        .map_err(|_| ScalarEvalError::Overflow(ty))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod vm_tests;
