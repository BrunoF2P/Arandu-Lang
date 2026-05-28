//! Basic AMIR optimizer (O1).
//!
//! The pass is intentionally small and semantics-preserving for v0.1:
//! constant folding runs first, then DCE removes unused pure temp assignments.
//! Note: Constant folding is currently intra-block only. Constants defined in one
//! block are not propagated to other blocks (cross-block propagation is planned for v0.2+).
#![allow(clippy::collapsible_if)]

use crate::amir::{
    AmirConstant, AmirFunc, AmirOperand, AmirProgram, AmirRvalue, AmirStmt, AmirTerminator,
};
use crate::literal_pool::{AmirLiteralEntry, AmirLiteralPool};
use crate::ops::{BinaryOp, UnaryOp};

pub fn optimize_amir(program: &mut AmirProgram) {
    for func in &mut program.funcs {
        fold_constants(func, &mut program.literal_pool);
        eliminate_dead_assigns(func);
    }
}

/// Folds constants intra-block. Note: Constants are currently not propagated across basic blocks.
fn fold_constants(func: &mut AmirFunc, pool: &mut AmirLiteralPool) {
    for block in &mut func.blocks {
        let mut constants = vec![None; func.temps.len()];
        for stmt in &mut block.statements {
            let AmirStmt::Assign { lhs, rhs } = stmt else {
                continue;
            };

            if let Some(folded) = fold_rvalue(rhs, &constants, pool) {
                *rhs = AmirRvalue::Use(AmirOperand::Constant(folded));
            }

            constants[lhs.as_usize()] = match rhs {
                AmirRvalue::Use(AmirOperand::Constant(value)) => Some(*value),
                _ => None,
            };
        }
    }
}

fn fold_rvalue(
    rvalue: &AmirRvalue,
    constants: &[Option<AmirConstant>],
    pool: &mut AmirLiteralPool,
) -> Option<AmirConstant> {
    match rvalue {
        AmirRvalue::Unary { op, operand } => {
            let value = operand_const(operand, constants)?;
            fold_unary(*op, value, pool)
        }
        AmirRvalue::Binary { op, left, right } => {
            let left = operand_const(left, constants)?;
            let right = operand_const(right, constants)?;
            fold_binary(*op, left, right, pool)
        }
        _ => None,
    }
}

fn operand_const(op: &AmirOperand, constants: &[Option<AmirConstant>]) -> Option<AmirConstant> {
    match op {
        AmirOperand::Constant(value) => Some(*value),
        AmirOperand::Copy(temp) | AmirOperand::Move(temp) => {
            constants.get(temp.as_usize()).copied().flatten()
        }
        AmirOperand::FunctionRef(_) | AmirOperand::GlobalRef(_) => None,
    }
}

fn fold_unary(
    op: UnaryOp,
    value: AmirConstant,
    pool: &mut AmirLiteralPool,
) -> Option<AmirConstant> {
    match (op, value) {
        (UnaryOp::Not, AmirConstant::Bool(value)) => Some(AmirConstant::Bool(!value)),
        (UnaryOp::Neg, value) => {
            let value = int_value(value, pool)?;
            Some(int_constant(-value, pool))
        }
        (UnaryOp::BitNot, value) => {
            let value = int_value(value, pool)?;
            Some(int_constant(!value, pool))
        }
        (UnaryOp::Await, _) => None,
        _ => None,
    }
}

fn fold_binary(
    op: BinaryOp,
    left: AmirConstant,
    right: AmirConstant,
    pool: &mut AmirLiteralPool,
) -> Option<AmirConstant> {
    match (left, right) {
        (AmirConstant::Bool(left), AmirConstant::Bool(right)) => match op {
            BinaryOp::And => Some(AmirConstant::Bool(left && right)),
            BinaryOp::Or => Some(AmirConstant::Bool(left || right)),
            BinaryOp::Equal => Some(AmirConstant::Bool(left == right)),
            BinaryOp::NotEqual => Some(AmirConstant::Bool(left != right)),
            _ => None,
        },
        (left, right) => {
            let left = int_value(left, pool)?;
            let right = int_value(right, pool)?;
            match op {
                BinaryOp::Add => Some(int_constant(left.checked_add(right)?, pool)),
                BinaryOp::Sub => Some(int_constant(left.checked_sub(right)?, pool)),
                BinaryOp::Mul => Some(int_constant(left.checked_mul(right)?, pool)),
                BinaryOp::Div if right != 0 => Some(int_constant(left.checked_div(right)?, pool)),
                BinaryOp::Mod if right != 0 => Some(int_constant(left.checked_rem(right)?, pool)),
                BinaryOp::BitOr => Some(int_constant(left | right, pool)),
                BinaryOp::BitXor => Some(int_constant(left ^ right, pool)),
                BinaryOp::BitAnd => Some(int_constant(left & right, pool)),
                BinaryOp::ShiftLeft if (0..128).contains(&right) => {
                    Some(int_constant(left.checked_shl(right as u32)?, pool))
                }
                BinaryOp::ShiftRight if (0..128).contains(&right) => {
                    Some(int_constant(left.checked_shr(right as u32)?, pool))
                }
                BinaryOp::Equal => Some(AmirConstant::Bool(left == right)),
                BinaryOp::NotEqual => Some(AmirConstant::Bool(left != right)),
                BinaryOp::Lt => Some(AmirConstant::Bool(left < right)),
                BinaryOp::Gt => Some(AmirConstant::Bool(left > right)),
                BinaryOp::LtEqual => Some(AmirConstant::Bool(left <= right)),
                BinaryOp::GtEqual => Some(AmirConstant::Bool(left >= right)),
                _ => None,
            }
        }
    }
}

fn int_value(value: AmirConstant, pool: &AmirLiteralPool) -> Option<i128> {
    match value {
        AmirConstant::Pool(id) => match pool.get(id) {
            AmirLiteralEntry::Int(value) => value.parse().ok(),
            AmirLiteralEntry::Float(_) | AmirLiteralEntry::Str(_) | AmirLiteralEntry::Char(_) => {
                None
            }
        },
        AmirConstant::Bool(_) | AmirConstant::Nil => None,
    }
}

fn int_constant(value: i128, pool: &mut AmirLiteralPool) -> AmirConstant {
    AmirConstant::Pool(pool.intern(AmirLiteralEntry::Int(value.to_string())))
}

fn eliminate_dead_assigns(func: &mut AmirFunc) {
    loop {
        let used = used_temps(func);
        let mut changed = false;

        for block in &mut func.blocks {
            let before = block.statements.len();
            block.statements.retain(|stmt| match stmt {
                AmirStmt::Assign { lhs, rhs } => used[lhs.as_usize()] || !is_removable_rvalue(rhs),
                _ => true,
            });
            changed |= before != block.statements.len();
        }

        if !changed {
            break;
        }
    }
}

fn used_temps(func: &AmirFunc) -> Vec<bool> {
    let mut used = vec![false; func.temps.len()];
    if !used.is_empty() {
        // AMIR `return` reads the conventional return register `_0`.
        used[0] = true;
    }
    for block in &func.blocks {
        for stmt in &block.statements {
            match stmt {
                AmirStmt::Assign { rhs, .. } => collect_rvalue_temps(rhs, &mut used[..]),
                AmirStmt::Store { rhs, lhs } => {
                    collect_operand_temp(rhs, &mut used[..]);
                    for projection in &lhs.projections {
                        if let crate::amir::AmirProjection::Index(op) = projection {
                            collect_operand_temp(op, &mut used[..]);
                        }
                    }
                }
                AmirStmt::Call {
                    lhs: _,
                    callee,
                    args,
                } => {
                    collect_operand_temp(callee, &mut used[..]);
                    for arg in args {
                        collect_operand_temp(arg, &mut used[..]);
                    }
                }
                AmirStmt::Free(op) => collect_operand_temp(op, &mut used[..]),
                AmirStmt::StorageLive(_) | AmirStmt::StorageDead(_) | AmirStmt::Destroy(_) => {}
            }
        }
        match &block.terminator {
            AmirTerminator::Branch { condition, .. } => {
                collect_operand_temp(condition, &mut used[..])
            }
            AmirTerminator::SwitchInt { discriminant, .. } => {
                collect_operand_temp(discriminant, &mut used[..]);
            }
            AmirTerminator::Return | AmirTerminator::Goto(_) | AmirTerminator::Unreachable => {}
        }
    }
    used
}

fn collect_rvalue_temps(rvalue: &AmirRvalue, used: &mut [bool]) {
    match rvalue {
        AmirRvalue::Use(op)
        | AmirRvalue::Unary { operand: op, .. }
        | AmirRvalue::FieldAccess { base: op, .. }
        | AmirRvalue::Discriminant { value: op }
        | AmirRvalue::EnumPayload { value: op, .. }
        | AmirRvalue::Len(op)
        | AmirRvalue::Alloc(op) => collect_operand_temp(op, &mut used[..]),
        AmirRvalue::Binary { left, right, .. }
        | AmirRvalue::IndexAccess {
            base: left,
            index: right,
        } => {
            collect_operand_temp(left, &mut used[..]);
            collect_operand_temp(right, &mut used[..]);
        }
        AmirRvalue::StructLiteral { fields, .. } => {
            for (_, op) in fields {
                collect_operand_temp(op, &mut used[..]);
            }
        }
        AmirRvalue::Array { items } | AmirRvalue::Tuple { items } => {
            for op in items {
                collect_operand_temp(op, &mut used[..]);
            }
        }
        AmirRvalue::Load(place) | AmirRvalue::Borrow(place) | AmirRvalue::BorrowMut(place) => {
            for projection in &place.projections {
                if let crate::amir::AmirProjection::Index(op) = projection {
                    collect_operand_temp(op, &mut used[..]);
                }
            }
        }
    }
}

fn collect_operand_temp(op: &AmirOperand, used: &mut [bool]) {
    if let AmirOperand::Copy(temp) | AmirOperand::Move(temp) = op {
        if let Some(slot) = used.get_mut(temp.as_usize()) {
            *slot = true;
        }
    }
}

fn is_removable_rvalue(rvalue: &AmirRvalue) -> bool {
    !rvalue_contains_move(rvalue) && !matches!(rvalue, AmirRvalue::Alloc(_))
}

fn rvalue_contains_move(rvalue: &AmirRvalue) -> bool {
    match rvalue {
        AmirRvalue::Use(op)
        | AmirRvalue::Unary { operand: op, .. }
        | AmirRvalue::FieldAccess { base: op, .. }
        | AmirRvalue::Discriminant { value: op }
        | AmirRvalue::EnumPayload { value: op, .. }
        | AmirRvalue::Len(op)
        | AmirRvalue::Alloc(op) => matches!(op, AmirOperand::Move(_)),
        AmirRvalue::Binary { left, right, .. }
        | AmirRvalue::IndexAccess {
            base: left,
            index: right,
        } => matches!(left, AmirOperand::Move(_)) || matches!(right, AmirOperand::Move(_)),
        AmirRvalue::StructLiteral { fields, .. } => fields
            .iter()
            .any(|(_, op)| matches!(op, AmirOperand::Move(_))),
        AmirRvalue::Array { items } | AmirRvalue::Tuple { items } => {
            items.iter().any(|op| matches!(op, AmirOperand::Move(_)))
        }
        AmirRvalue::Load(_) | AmirRvalue::Borrow(_) | AmirRvalue::BorrowMut(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amir::{AmirBasicBlock, BlockId, TempId};
    use crate::passes::type_checker::types::{ArType, Primitive};

    fn int_temp(id: usize) -> crate::amir::AmirTemp {
        crate::amir::AmirTemp {
            id: TempId::from_usize(id),
            ty: ArType::Primitive(Primitive::Int),
            span: arandu_lexer::Span::new(0, 0, 0, 0, 0, 0),
        }
    }

    fn bool_temp(id: usize) -> crate::amir::AmirTemp {
        crate::amir::AmirTemp {
            id: TempId::from_usize(id),
            ty: ArType::Primitive(Primitive::Bool),
            span: arandu_lexer::Span::new(0, 0, 0, 0, 0, 0),
        }
    }

    fn func(statements: Vec<AmirStmt>, temps: Vec<crate::amir::AmirTemp>) -> AmirFunc {
        AmirFunc {
            symbol: crate::SymbolId(0),
            return_type: ArType::Void,
            receiver: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps,
            blocks: vec![AmirBasicBlock {
                id: BlockId::from_usize(0),
                statements,
                terminator: AmirTerminator::Return,
                successors: Vec::new(),
                predecessors: Vec::new(),
            }],
        }
    }

    #[test]
    fn folds_integer_binary_and_comparison() {
        let mut pool = AmirLiteralPool::default();
        let two = AmirConstant::Pool(pool.intern(AmirLiteralEntry::Int("2".to_string())));
        let three = AmirConstant::Pool(pool.intern(AmirLiteralEntry::Int("3".to_string())));
        let mut func = func(
            vec![
                AmirStmt::Assign {
                    lhs: TempId::from_usize(0),
                    rhs: AmirRvalue::Binary {
                        op: BinaryOp::Add,
                        left: AmirOperand::Constant(two),
                        right: AmirOperand::Constant(three),
                    },
                },
                AmirStmt::Assign {
                    lhs: TempId::from_usize(1),
                    rhs: AmirRvalue::Binary {
                        op: BinaryOp::Gt,
                        left: AmirOperand::Copy(TempId::from_usize(0)),
                        right: AmirOperand::Constant(three),
                    },
                },
            ],
            vec![int_temp(0), bool_temp(1)],
        );

        fold_constants(&mut func, &mut pool);

        assert!(matches!(
            &func.blocks[0].statements[1],
            AmirStmt::Assign {
                rhs: AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Bool(true))),
                ..
            }
        ));
    }

    #[test]
    fn dce_removes_unused_pure_assigns_and_keeps_alloc() {
        let mut func = func(
            vec![
                AmirStmt::Assign {
                    lhs: TempId::from_usize(1),
                    rhs: AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Bool(true))),
                },
                AmirStmt::Assign {
                    lhs: TempId::from_usize(2),
                    rhs: AmirRvalue::Alloc(AmirOperand::Constant(AmirConstant::Bool(true))),
                },
            ],
            vec![bool_temp(0), bool_temp(1), bool_temp(2)],
        );

        eliminate_dead_assigns(&mut func);

        assert_eq!(func.blocks[0].statements.len(), 1);
        assert!(matches!(
            func.blocks[0].statements[0],
            AmirStmt::Assign {
                rhs: AmirRvalue::Alloc(_),
                ..
            }
        ));
    }
}
