//! Shared AMIR rvalue/operand visitors (RC-ANALYSIS-LOAD).
//!
//! All analyses (liveness, move, definite-init, DCE) and backends should walk
//! operands through these helpers so new `AmirRvalue` variants cannot silently
//! skip a pass.

use super::value::{AmirOperand, AmirPlace, AmirProjection, AmirRvalue};

/// Invoke `f` for every operand nested in `place` projections (e.g. index).
pub fn for_each_place_operand(place: &AmirPlace, mut f: impl FnMut(&AmirOperand)) {
    for proj in &place.projections {
        if let AmirProjection::Index(op) = proj {
            f(op);
        }
    }
}

/// Invoke `f` for every operand used by `rvalue` (not places themselves).
pub fn for_each_rvalue_operand(rvalue: &AmirRvalue, mut f: impl FnMut(&AmirOperand)) {
    match rvalue {
        AmirRvalue::Use(op)
        | AmirRvalue::Unary { operand: op, .. }
        | AmirRvalue::Len(op)
        | AmirRvalue::Alloc(op)
        | AmirRvalue::Discriminant { value: op }
        | AmirRvalue::EnumPayload { value: op, .. }
        | AmirRvalue::FieldAccess { base: op, .. }
        | AmirRvalue::ToStr { value: op, .. }
        | AmirRvalue::CoroutineReady { value: op, .. }
        | AmirRvalue::GenInsert { value: op }
        | AmirRvalue::GenGet { gen_ref: op }
        | AmirRvalue::GenRemove { gen_ref: op } => f(op),

        AmirRvalue::Binary { left, right, .. }
        | AmirRvalue::IndexAccess {
            base: left,
            index: right,
        } => {
            f(left);
            f(right);
        }

        AmirRvalue::EnumConstruct { payload, .. } => {
            if let Some(op) = payload {
                f(op);
            }
        }

        AmirRvalue::StructLiteral { fields, .. } => {
            for (_, op) in fields {
                f(op);
            }
        }

        AmirRvalue::Array { items }
        | AmirRvalue::Tuple { items }
        | AmirRvalue::StringInterp { parts: items } => {
            for op in items {
                f(op);
            }
        }

        AmirRvalue::Load(place) | AmirRvalue::Borrow(place) | AmirRvalue::BorrowMut(place) => {
            for_each_place_operand(place, &mut f);
        }
    }
}

/// Invoke `f` for every place nested in `rvalue` (Load/Borrow/BorrowMut).
pub fn for_each_rvalue_place(rvalue: &AmirRvalue, mut f: impl FnMut(&AmirPlace)) {
    match rvalue {
        AmirRvalue::Load(place) | AmirRvalue::Borrow(place) | AmirRvalue::BorrowMut(place) => {
            f(place);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amir::local::TempId;
    use crate::ops::BinaryOp;

    #[test]
    fn visits_binary_operands() {
        let rv = AmirRvalue::Binary {
            op: BinaryOp::Add,
            left: AmirOperand::Copy(TempId::from_usize(1)),
            right: AmirOperand::Copy(TempId::from_usize(2)),
        };
        let mut temps = Vec::new();
        for_each_rvalue_operand(&rv, |op| {
            if let AmirOperand::Copy(t) = op {
                temps.push(t.as_usize());
            }
        });
        assert_eq!(temps, vec![1, 2]);
    }

    #[test]
    fn visits_string_interp_parts() {
        let rv = AmirRvalue::StringInterp {
            parts: vec![
                AmirOperand::Copy(TempId::from_usize(0)),
                AmirOperand::Copy(TempId::from_usize(3)),
            ],
        };
        let mut n = 0;
        for_each_rvalue_operand(&rv, |_| n += 1);
        assert_eq!(n, 2);
    }
}
