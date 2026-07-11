//! Intraprocedural liveness analysis (locals + SSA temps).
//!
//! - [`analyze_local_liveness`]: stack locals (register allocation / OSSA).
//! - [`analyze_temp_liveness`]: SSA temps — **F2.2** reuses this so a loan's
//!   window equals the live range of the reference value that holds it.

use crate::amir::reachability::terminator_targets;
use crate::amir::{
    AmirFunc, AmirOperand, AmirPlace, AmirProjection, AmirRvalue, AmirStmt, AmirTerminator,
    BlockId, LocalId, TempId, for_each_rvalue_operand, for_each_rvalue_place,
};
use crate::{BitMatrix, BitSet};

/// Liveness query results for all local variables within a single function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalLiveness {
    live_in: Vec<BitSet<LocalId>>,
    live_out: Vec<BitSet<LocalId>>,
}

impl LocalLiveness {
    /// Returns the set of local variables that are live at the entry of the given block.
    #[must_use]
    pub fn live_in(&self, block: BlockId) -> &BitSet<LocalId> {
        &self.live_in[block.as_usize()]
    }

    /// Returns the set of local variables that are live at the exit of the given block.
    #[must_use]
    pub fn live_out(&self, block: BlockId) -> &BitSet<LocalId> {
        &self.live_out[block.as_usize()]
    }
}

/// Liveness of SSA temps (per-block live-in / live-out).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempLiveness {
    live_in: Vec<BitSet<TempId>>,
    live_out: Vec<BitSet<TempId>>,
}

impl TempLiveness {
    #[must_use]
    pub fn live_in(&self, block: BlockId) -> &BitSet<TempId> {
        &self.live_in[block.as_usize()]
    }

    #[must_use]
    pub fn live_out(&self, block: BlockId) -> &BitSet<TempId> {
        &self.live_out[block.as_usize()]
    }
}

/// Runs intraprocedural liveness analysis for all local variables in the function.
///
/// Uses a backward dataflow analysis over the CFG.
#[must_use]
pub fn analyze_local_liveness(func: &AmirFunc) -> LocalLiveness {
    let num_blocks = func.blocks.len();
    let num_locals = func.locals.len();
    let mut block_uses = BitMatrix::<BlockId, LocalId>::new(num_blocks, num_locals);
    let mut block_defs = BitMatrix::<BlockId, LocalId>::new(num_blocks, num_locals);

    for block in &func.blocks {
        let mut defined = BitSet::<LocalId>::with_capacity(num_locals);
        for stmt in func.block_stmts(block.id) {
            collect_stmt_uses(stmt, &defined, &mut block_uses, block.id);
            collect_stmt_defs(stmt, &mut defined, &mut block_defs, block.id);
        }
        collect_terminator_uses(&block.terminator, &defined, &mut block_uses, block.id);
    }

    let mut live_in = vec![BitSet::<LocalId>::with_capacity(num_locals); num_blocks];
    let mut live_out = vec![BitSet::<LocalId>::with_capacity(num_locals); num_blocks];
    let mut changed = true;

    let rpo = crate::amir::reverse_post_order(func);

    let mut new_out = BitSet::<LocalId>::with_capacity(num_locals);
    let mut new_in = BitSet::<LocalId>::with_capacity(num_locals);

    // Bound iterations: monotone lattice converges in ≤ |blocks| in theory;
    // hard cap guards host freeze if CFG metadata is corrupt.
    let max_iters = (num_blocks.saturating_mul(2)).max(8);
    let mut iters = 0usize;
    while changed && iters < max_iters {
        iters += 1;
        changed = false;
        for &block_id in rpo.iter().rev() {
            let block = &func.blocks[block_id.as_usize()];

            new_out.clear();
            for successor in terminator_targets(&block.terminator) {
                new_out.union_with(&live_in[successor.as_usize()]);
            }

            new_in.clone_from(&new_out);
            new_in.difference_with(&block_defs.row_set(block_id));
            new_in.union_with(&block_uses.row_set(block_id));

            let index = block_id.as_usize();
            if new_in != live_in[index] || new_out != live_out[index] {
                live_in[index].clone_from(&new_in);
                live_out[index].clone_from(&new_out);
                changed = true;
            }
        }
    }

    LocalLiveness { live_in, live_out }
}

/// Backward dataflow: which SSA temps are live-in / live-out per block (F2.2).
#[must_use]
pub fn analyze_temp_liveness(func: &AmirFunc) -> TempLiveness {
    let num_blocks = func.blocks.len();
    let num_temps = func.temps.len();
    let mut block_uses = BitMatrix::<BlockId, TempId>::new(num_blocks, num_temps);
    let mut block_defs = BitMatrix::<BlockId, TempId>::new(num_blocks, num_temps);

    for block in &func.blocks {
        let mut defined = BitSet::<TempId>::with_capacity(num_temps);
        // Block params are defs at entry (before body uses).
        for param in &block.params {
            defined.insert(param.id);
            block_defs.insert(block.id, param.id);
        }
        for stmt in func.block_stmts(block.id) {
            collect_stmt_temp_uses(stmt, &defined, &mut block_uses, block.id);
            collect_stmt_temp_defs(stmt, &mut defined, &mut block_defs, block.id);
        }
        collect_terminator_temp_uses(&block.terminator, &defined, &mut block_uses, block.id);
    }

    let mut live_in = vec![BitSet::<TempId>::with_capacity(num_temps); num_blocks];
    let mut live_out = vec![BitSet::<TempId>::with_capacity(num_temps); num_blocks];
    let mut changed = true;
    let rpo = crate::amir::reverse_post_order(func);
    let mut new_out = BitSet::<TempId>::with_capacity(num_temps);
    let mut new_in = BitSet::<TempId>::with_capacity(num_temps);

    let max_iters = (num_blocks.saturating_mul(2)).max(8);
    let mut iters = 0usize;
    while changed && iters < max_iters {
        iters += 1;
        changed = false;
        for &block_id in rpo.iter().rev() {
            let block = &func.blocks[block_id.as_usize()];
            new_out.clear();
            for successor in terminator_targets(&block.terminator) {
                new_out.union_with(&live_in[successor.as_usize()]);
            }
            new_in.clone_from(&new_out);
            new_in.difference_with(&block_defs.row_set(block_id));
            new_in.union_with(&block_uses.row_set(block_id));
            let index = block_id.as_usize();
            if new_in != live_in[index] || new_out != live_out[index] {
                live_in[index].clone_from(&new_in);
                live_out[index].clone_from(&new_out);
                changed = true;
            }
        }
    }

    TempLiveness { live_in, live_out }
}

fn collect_stmt_temp_uses(
    stmt: &AmirStmt,
    defined: &BitSet<TempId>,
    uses: &mut BitMatrix<BlockId, TempId>,
    block: BlockId,
) {
    match stmt {
        AmirStmt::Assign { rhs, .. } => {
            for_each_rvalue_operand(rhs, |op| mark_temp_use(op, defined, uses, block));
            for_each_rvalue_place(rhs, |place| {
                for proj in &place.projections {
                    if let AmirProjection::Index(op) = proj {
                        mark_temp_use(op, defined, uses, block);
                    }
                }
            });
        }
        AmirStmt::Store { lhs, rhs } => {
            mark_temp_use(rhs, defined, uses, block);
            for proj in &lhs.projections {
                if let AmirProjection::Index(op) = proj {
                    mark_temp_use(op, defined, uses, block);
                }
            }
        }
        AmirStmt::Call { callee, args, .. } => {
            mark_temp_use(callee, defined, uses, block);
            for arg in args {
                mark_temp_use(arg, defined, uses, block);
            }
        }
        AmirStmt::Free(op) => mark_temp_use(op, defined, uses, block),
        AmirStmt::Destroy(place) => {
            for proj in &place.projections {
                if let AmirProjection::Index(op) = proj {
                    mark_temp_use(op, defined, uses, block);
                }
            }
        }
        AmirStmt::StorageLive(_) | AmirStmt::StorageDead(_) | AmirStmt::Nop => {}
    }
}

fn collect_stmt_temp_defs(
    stmt: &AmirStmt,
    defined: &mut BitSet<TempId>,
    defs: &mut BitMatrix<BlockId, TempId>,
    block: BlockId,
) {
    match stmt {
        AmirStmt::Assign { lhs, .. } => {
            defined.insert(*lhs);
            defs.insert(block, *lhs);
        }
        AmirStmt::Call { lhs: Some(t), .. } => {
            defined.insert(*t);
            defs.insert(block, *t);
        }
        _ => {}
    }
}

fn collect_terminator_temp_uses(
    term: &AmirTerminator,
    defined: &BitSet<TempId>,
    uses: &mut BitMatrix<BlockId, TempId>,
    block: BlockId,
) {
    match term {
        AmirTerminator::Branch {
            condition,
            true_args,
            false_args,
            ..
        } => {
            mark_temp_use(condition, defined, uses, block);
            for a in true_args {
                mark_temp_use(a, defined, uses, block);
            }
            for a in false_args {
                mark_temp_use(a, defined, uses, block);
            }
        }
        AmirTerminator::SwitchInt {
            discriminant,
            targets,
            otherwise,
            ..
        } => {
            mark_temp_use(discriminant, defined, uses, block);
            for (_, _, args) in targets {
                for a in args {
                    mark_temp_use(a, defined, uses, block);
                }
            }
            for a in &otherwise.1 {
                mark_temp_use(a, defined, uses, block);
            }
        }
        AmirTerminator::Goto { args, .. } => {
            for a in args {
                mark_temp_use(a, defined, uses, block);
            }
        }
        AmirTerminator::Suspend { future, args, .. } => {
            mark_temp_use(future, defined, uses, block);
            for a in args {
                mark_temp_use(a, defined, uses, block);
            }
        }
        AmirTerminator::Return | AmirTerminator::Unreachable => {}
    }
}

fn mark_temp_use(
    op: &AmirOperand,
    defined: &BitSet<TempId>,
    uses: &mut BitMatrix<BlockId, TempId>,
    block: BlockId,
) {
    if let AmirOperand::Copy(t) | AmirOperand::Move(t) = op
        && !defined.contains(*t)
    {
        uses.insert(block, *t);
    }
}

fn collect_stmt_uses(
    stmt: &AmirStmt,
    defined: &BitSet<LocalId>,
    uses: &mut BitMatrix<BlockId, LocalId>,
    block: BlockId,
) {
    match stmt {
        AmirStmt::Assign { rhs, .. } => collect_rvalue_uses(rhs, defined, uses, block),
        AmirStmt::Store { lhs, rhs } => {
            if !lhs.projections.is_empty() {
                collect_place_use(lhs, defined, uses, block);
            } else {
                collect_projection_uses(lhs, defined, uses, block);
            }
            collect_operand_uses(rhs, defined, uses, block);
        }
        AmirStmt::Call { callee, args, .. } => {
            collect_operand_uses(callee, defined, uses, block);
            for arg in args {
                collect_operand_uses(arg, defined, uses, block);
            }
        }
        AmirStmt::Free(op) => collect_operand_uses(op, defined, uses, block),
        AmirStmt::Destroy(place) => collect_place_use(place, defined, uses, block),
        AmirStmt::StorageLive(_) | AmirStmt::StorageDead(_) | AmirStmt::Nop => {}
    }
}

fn collect_stmt_defs(
    stmt: &AmirStmt,
    defined: &mut BitSet<LocalId>,
    defs: &mut BitMatrix<BlockId, LocalId>,
    block: BlockId,
) {
    if let AmirStmt::Store { lhs, .. } = stmt
        && lhs.projections.is_empty()
    {
        defined.insert(lhs.local);
        defs.insert(block, lhs.local);
    }
}

fn collect_terminator_uses(
    term: &AmirTerminator,
    defined: &BitSet<LocalId>,
    uses: &mut BitMatrix<BlockId, LocalId>,
    block: BlockId,
) {
    match term {
        AmirTerminator::Branch { condition, .. } => {
            collect_operand_uses(condition, defined, uses, block);
        }
        AmirTerminator::SwitchInt { discriminant, .. } => {
            collect_operand_uses(discriminant, defined, uses, block);
        }
        AmirTerminator::Suspend { future, args, .. } => {
            collect_operand_uses(future, defined, uses, block);
            for a in args {
                collect_operand_uses(a, defined, uses, block);
            }
        }
        AmirTerminator::Return | AmirTerminator::Goto { .. } | AmirTerminator::Unreachable => {}
    }
}

fn collect_rvalue_uses(
    rvalue: &AmirRvalue,
    defined: &BitSet<LocalId>,
    uses: &mut BitMatrix<BlockId, LocalId>,
    block: BlockId,
) {
    // Shared visitor: places (Load/Borrow) and any nested operands (RC-ANALYSIS-LOAD).
    for_each_rvalue_place(rvalue, |place| {
        collect_place_use(place, defined, uses, block);
    });
    for_each_rvalue_operand(rvalue, |op| {
        collect_operand_uses(op, defined, uses, block);
    });
}

fn collect_place_use(
    place: &AmirPlace,
    defined: &BitSet<LocalId>,
    uses: &mut BitMatrix<BlockId, LocalId>,
    block: BlockId,
) {
    if !defined.contains(place.local) {
        uses.insert(block, place.local);
    }
    collect_projection_uses(place, defined, uses, block);
}

fn collect_projection_uses(
    place: &AmirPlace,
    defined: &BitSet<LocalId>,
    uses: &mut BitMatrix<BlockId, LocalId>,
    block: BlockId,
) {
    for projection in &place.projections {
        if let AmirProjection::Index(op) = projection {
            collect_operand_uses(op, defined, uses, block);
        }
    }
}

fn collect_operand_uses(
    _op: &AmirOperand,
    _defined: &BitSet<LocalId>,
    _uses: &mut BitMatrix<BlockId, LocalId>,
    _block: BlockId,
) {
}

#[cfg(test)]
#[path = "liveness_tests.rs"]
mod tests;
