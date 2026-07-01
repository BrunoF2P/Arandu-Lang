use crate::amir::{
    AmirFunc, AmirOperand, AmirProjection, AmirRvalue, AmirStmt, AmirTerminator, BlockId, TempId,
};
use crate::layout::DenseRange;
use smallvec::SmallVec;
use std::collections::VecDeque;

/// Mark-Sweep Dead Code Elimination (Cooper & Torczon, Engineering a Compiler, ch. 10).
///
/// Phase 1 — Mark: seeds are side-effecting instructions (Call, Store, Free, Destroy,
/// StorageLive/Dead, Alloc).  Transitively marks the defining instruction of every
/// temp operand used by a live instruction.  Branch conditions and switch discriminants
/// are also seeds.
///
/// Phase 2 — Sweep: removes every unmarked statement by rebuilding the stmt table.
///
/// Returns `true` if any instruction was removed.
pub fn mark_sweep_dce(func: &mut AmirFunc) -> bool {
    let n_stmts = func.stmts.len();
    let n_temps = func.temps.len();
    if n_stmts == 0 || n_temps == 0 {
        return false;
    }

    // --- def map: TempId → InstrId -----------------------------------------
    let mut def_of = vec![None; n_temps];
    for bi in 0..func.blocks.len() {
        let bid = BlockId::from_usize(bi);
        for stmt_id in func.block_stmt_ids(bid) {
            if let AmirStmt::Assign { lhs, .. } = func.stmt(stmt_id) {
                def_of[lhs.as_usize()] = Some(stmt_id);
            }
        }
    }

    // --- Mark phase ---------------------------------------------------------
    //
    // Principle: the roots of a DCE mark-phase are everything observable from
    // outside the function:
    //   - Return value (`_0` in AMIR convention)
    //   - Side effects (Call, Store, Free, Destroy, Alloc, StorageLive/Dead)
    //   - Terminator operand uses (branch conditions, switch discriminants)
    //
    // Future roots to add when the language grows:
    //   - Mutable reference parameters (writes visible to caller)
    //   - Closure captures by reference
    //
    let mut live = vec![false; n_stmts];
    let mut queue: VecDeque<usize> = VecDeque::new();

    // Seed: return register _0 is always live (AMIR convention).
    if let Some(def) = def_of[0] {
        let idx = def.as_usize();
        live[idx] = true;
        queue.push_back(idx);
    }

    // Seeds: side-effecting stmts + terminator operand defs
    for bi in 0..func.blocks.len() {
        let bid = BlockId::from_usize(bi);

        for temp in collect_terminator_temps(&func.block(bid).terminator) {
            if let Some(def) = def_of[temp.as_usize()] {
                let idx = def.as_usize();
                if !live[idx] {
                    live[idx] = true;
                    queue.push_back(idx);
                }
            }
        }

        for stmt_id in func.block_stmt_ids(bid) {
            if has_side_effect(func.stmt(stmt_id)) {
                let idx = stmt_id.as_usize();
                if !live[idx] {
                    live[idx] = true;
                    queue.push_back(idx);
                }
            }
        }
    }

    // Propagate: for each live instruction, its operand defs become live
    while let Some(idx) = queue.pop_front() {
        let stmt = func.stmt(crate::amir::InstrId::from_usize(idx));
        for temp in collect_stmt_temps(stmt) {
            if let Some(def) = def_of[temp.as_usize()] {
                let didx = def.as_usize();
                if !live[didx] {
                    live[didx] = true;
                    queue.push_back(didx);
                }
            }
        }
    }

    // --- Sweep phase: rebuild stmt table with only live stmts --------------
    let mut new_stmts = crate::amir::AmirStmtTable::new();
    let mut new_ranges: Vec<DenseRange> = Vec::with_capacity(func.blocks.len());
    let mut any_removed = false;

    for block in &func.blocks {
        let start = new_stmts.len();
        let mut kept = 0usize;
        for stmt_id in func.block_stmt_ids(block.id) {
            if live[stmt_id.as_usize()] {
                new_stmts.push(func.stmt(stmt_id).clone());
                kept += 1;
            } else {
                any_removed = true;
            }
        }
        new_ranges.push(DenseRange::new(start, kept));
    }

    if any_removed {
        func.stmts = new_stmts;
        for (block, range) in func.blocks.iter_mut().zip(new_ranges) {
            block.statements = range;
        }
    }

    any_removed
}

// ---------------------------------------------------------------------------
// Side-effect / temp helpers
// ---------------------------------------------------------------------------

fn has_side_effect(stmt: &AmirStmt) -> bool {
    match stmt {
        AmirStmt::Call { .. }
        | AmirStmt::Store { .. }
        | AmirStmt::Free(_)
        | AmirStmt::Destroy(_)
        | AmirStmt::StorageLive(_)
        | AmirStmt::StorageDead(_) => true,
        AmirStmt::Assign { rhs, .. } => matches!(rhs, AmirRvalue::Alloc(_)),
        AmirStmt::Nop => false,
    }
}

fn collect_terminator_temps(term: &AmirTerminator) -> SmallVec<[TempId; 4]> {
    match term {
        AmirTerminator::Branch { condition, .. } => collect_operand_temps(condition),
        AmirTerminator::SwitchInt { discriminant, .. } => collect_operand_temps(discriminant),
        _ => SmallVec::new(),
    }
}

fn collect_stmt_temps(stmt: &AmirStmt) -> SmallVec<[TempId; 4]> {
    match stmt {
        AmirStmt::Assign { rhs, .. } => collect_rvalue_temps(rhs),
        AmirStmt::Store { lhs, rhs } => {
            let mut temps = collect_operand_temps(rhs);
            for proj in &lhs.projections {
                if let AmirProjection::Index(op) = proj {
                    temps.extend(collect_operand_temps(op));
                }
            }
            temps
        }
        AmirStmt::Call { callee, args, .. } => {
            let mut temps = collect_operand_temps(callee);
            for arg in args {
                temps.extend(collect_operand_temps(arg));
            }
            temps
        }
        AmirStmt::Free(op) => collect_operand_temps(op),
        AmirStmt::Destroy(place) => {
            let mut temps = SmallVec::new();
            for proj in &place.projections {
                if let AmirProjection::Index(op) = proj {
                    temps.extend(collect_operand_temps(op));
                }
            }
            temps
        }
        AmirStmt::StorageLive(_) | AmirStmt::StorageDead(_) | AmirStmt::Nop => SmallVec::new(),
    }
}

fn collect_rvalue_temps(rvalue: &AmirRvalue) -> SmallVec<[TempId; 4]> {
    match rvalue {
        AmirRvalue::Use(op)
        | AmirRvalue::Unary { operand: op, .. }
        | AmirRvalue::Len(op)
        | AmirRvalue::Alloc(op) => collect_operand_temps(op),
        AmirRvalue::Binary { left, right, .. } => {
            let mut t = collect_operand_temps(left);
            t.extend(collect_operand_temps(right));
            t
        }
        AmirRvalue::FieldAccess { base, .. }
        | AmirRvalue::Discriminant { value: base }
        | AmirRvalue::EnumPayload { value: base, .. } => collect_operand_temps(base),
        AmirRvalue::IndexAccess { base, index } => {
            let mut t = collect_operand_temps(base);
            t.extend(collect_operand_temps(index));
            t
        }
        AmirRvalue::StructLiteral { fields, .. } => {
            let mut t = SmallVec::new();
            for (_, op) in fields {
                t.extend(collect_operand_temps(op));
            }
            t
        }
        AmirRvalue::Array { items } | AmirRvalue::Tuple { items } => {
            let mut t = SmallVec::new();
            for op in items {
                t.extend(collect_operand_temps(op));
            }
            t
        }
        AmirRvalue::Load(place) | AmirRvalue::Borrow(place) | AmirRvalue::BorrowMut(place) => {
            let mut t = SmallVec::new();
            for proj in &place.projections {
                if let AmirProjection::Index(op) = proj {
                    t.extend(collect_operand_temps(op));
                }
            }
            t
        }
    }
}

fn collect_operand_temps(op: &AmirOperand) -> SmallVec<[TempId; 4]> {
    match op {
        AmirOperand::Copy(t) | AmirOperand::Move(t) => {
            let mut v = SmallVec::new();
            v.push(*t);
            v
        }
        _ => SmallVec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amir::program::extend_block_range;
    use crate::amir::{
        AmirBasicBlock, AmirConstant, AmirPlace, AmirStmtTable, AmirTemp, BlockId, LocalId, TempId,
    };
    use crate::layout::DenseRange;
    use crate::ops::BinaryOp;
    use crate::passes::type_checker::types::{ArType, Primitive};

    fn int_temp(id: usize) -> AmirTemp {
        AmirTemp {
            id: TempId::from_usize(id),
            ty: ArType::Primitive(Primitive::Int),
            span: arandu_lexer::Span::new(0, 0, 0),
        }
    }

    fn bool_temp(id: usize) -> AmirTemp {
        AmirTemp {
            id: TempId::from_usize(id),
            ty: ArType::Primitive(Primitive::Bool),
            span: arandu_lexer::Span::new(0, 0, 0),
        }
    }

    fn func(statements: Vec<AmirStmt>, temps: Vec<AmirTemp>) -> AmirFunc {
        let mut stmts = AmirStmtTable::new();
        let mut range = DenseRange::empty();
        for stmt in statements {
            let instr = stmts.push(stmt);
            extend_block_range(&mut range, instr);
        }
        let blocks = vec![AmirBasicBlock {
            id: BlockId::from_usize(0),
            statements: range,
            terminator: AmirTerminator::Return,
        }];
        let cfg = crate::cfg::compute_cfg_edges(&blocks);
        AmirFunc {
            symbol: crate::SymbolId(0),
            return_type: ArType::Void,
            receiver: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps,
            blocks,
            stmts,
            cfg,
        }
    }

    #[test]
    fn keeps_call_side_effect() {
        let mut f = func(
            vec![AmirStmt::Call {
                lhs: Some(TempId::from_usize(0)),
                callee: AmirOperand::FunctionRef(crate::SymbolId(1)),
                args: smallvec::smallvec![],
            }],
            vec![bool_temp(0)],
        );
        assert!(!mark_sweep_dce(&mut f));
        assert_eq!(f.blocks[0].statements.len, 1);
    }

    #[test]
    fn removes_unused_assign() {
        let mut f = func(
            vec![AmirStmt::Assign {
                lhs: TempId::from_usize(1),
                rhs: AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Bool(true))),
            }],
            vec![bool_temp(0), bool_temp(1)],
        );
        assert!(mark_sweep_dce(&mut f));
        assert_eq!(f.blocks[0].statements.len, 0);
    }

    #[test]
    fn keeps_binary_when_result_is_used() {
        let mut f = func(
            vec![
                AmirStmt::Assign {
                    lhs: TempId::from_usize(1),
                    rhs: AmirRvalue::Binary {
                        op: BinaryOp::Add,
                        left: AmirOperand::Constant(AmirConstant::Pool(
                            crate::literal_pool::LiteralId(0),
                        )),
                        right: AmirOperand::Constant(AmirConstant::Pool(
                            crate::literal_pool::LiteralId(1),
                        )),
                    },
                },
                AmirStmt::Store {
                    lhs: AmirPlace {
                        local: LocalId::from_usize(0),
                        projections: smallvec::smallvec![],
                    },
                    rhs: AmirOperand::Copy(TempId::from_usize(1)),
                },
            ],
            vec![int_temp(0), int_temp(1)],
        );
        assert!(!mark_sweep_dce(&mut f));
        assert_eq!(f.blocks[0].statements.len, 2);
    }

    #[test]
    fn removes_unreachable_chain() {
        // a = b + 1; b = c * 2; c = 5; (all unused) → all removed in one pass
        let mut f = func(
            vec![
                AmirStmt::Assign {
                    lhs: TempId::from_usize(2),
                    rhs: AmirRvalue::Binary {
                        op: BinaryOp::Add,
                        left: AmirOperand::Copy(TempId::from_usize(1)),
                        right: AmirOperand::Constant(AmirConstant::Bool(true)),
                    },
                },
                AmirStmt::Assign {
                    lhs: TempId::from_usize(1),
                    rhs: AmirRvalue::Binary {
                        op: BinaryOp::Mul,
                        left: AmirOperand::Copy(TempId::from_usize(0)),
                        right: AmirOperand::Constant(AmirConstant::Bool(true)),
                    },
                },
                AmirStmt::Assign {
                    lhs: TempId::from_usize(0),
                    rhs: AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Bool(true))),
                },
            ],
            vec![bool_temp(0), bool_temp(1), bool_temp(2)],
        );
        assert!(mark_sweep_dce(&mut f));
        // _0 is always live, so the t0 = Use(true) stmt survives.
        // t2 = t1 + true and t1 = t0 * true are correctly removed.
        assert_eq!(f.blocks[0].statements.len, 1);
    }

    #[test]
    fn noop_when_nothing_to_remove() {
        let mut f = func(
            vec![AmirStmt::Store {
                lhs: AmirPlace {
                    local: LocalId::from_usize(0),
                    projections: smallvec::smallvec![],
                },
                rhs: AmirOperand::Constant(AmirConstant::Bool(true)),
            }],
            vec![],
        );
        assert!(!mark_sweep_dce(&mut f));
        assert_eq!(f.blocks[0].statements.len, 1);
    }
}
