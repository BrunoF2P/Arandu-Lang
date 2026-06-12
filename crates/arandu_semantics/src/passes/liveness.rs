use crate::amir::reachability::terminator_targets;
use crate::amir::{
    AmirFunc, AmirOperand, AmirPlace, AmirProjection, AmirRvalue, AmirStmt, AmirTerminator,
    BlockId, LocalId,
};
use crate::{BitMatrix, BitSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalLiveness {
    live_in: Vec<BitSet<LocalId>>,
    live_out: Vec<BitSet<LocalId>>,
}

impl LocalLiveness {
    #[must_use]
    pub fn live_in(&self, block: BlockId) -> &BitSet<LocalId> {
        &self.live_in[block.as_usize()]
    }

    #[must_use]
    pub fn live_out(&self, block: BlockId) -> &BitSet<LocalId> {
        &self.live_out[block.as_usize()]
    }
}

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

    while changed {
        changed = false;
        for &block_id in rpo.iter().rev() {
            let block = &func.blocks[block_id.as_usize()];

            let mut new_out = BitSet::<LocalId>::with_capacity(num_locals);
            for successor in terminator_targets(&block.terminator) {
                new_out.union_with(&live_in[successor.as_usize()]);
            }

            let mut new_in = new_out.clone();
            new_in.difference_with(&block_defs.row_set(block_id));
            new_in.union_with(&block_uses.row_set(block_id));

            let index = block_id.as_usize();
            if new_in != live_in[index] || new_out != live_out[index] {
                live_in[index] = new_in;
                live_out[index] = new_out;
                changed = true;
            }
        }
    }

    LocalLiveness { live_in, live_out }
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
        AmirStmt::StorageLive(_) | AmirStmt::StorageDead(_) => {}
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
        AmirTerminator::Return | AmirTerminator::Goto(_) | AmirTerminator::Unreachable => {}
    }
}

fn collect_rvalue_uses(
    rvalue: &AmirRvalue,
    defined: &BitSet<LocalId>,
    uses: &mut BitMatrix<BlockId, LocalId>,
    block: BlockId,
) {
    match rvalue {
        AmirRvalue::Load(place) | AmirRvalue::Borrow(place) | AmirRvalue::BorrowMut(place) => {
            collect_place_use(place, defined, uses, block);
        }
        AmirRvalue::Use(op)
        | AmirRvalue::Unary { operand: op, .. }
        | AmirRvalue::FieldAccess { base: op, .. }
        | AmirRvalue::Discriminant { value: op }
        | AmirRvalue::EnumPayload { value: op, .. }
        | AmirRvalue::Len(op)
        | AmirRvalue::Alloc(op) => collect_operand_uses(op, defined, uses, block),
        AmirRvalue::Binary { left, right, .. }
        | AmirRvalue::IndexAccess {
            base: left,
            index: right,
        } => {
            collect_operand_uses(left, defined, uses, block);
            collect_operand_uses(right, defined, uses, block);
        }
        AmirRvalue::StructLiteral { fields, .. } => {
            for (_, op) in fields {
                collect_operand_uses(op, defined, uses, block);
            }
        }
        AmirRvalue::Array { items } | AmirRvalue::Tuple { items } => {
            for op in items {
                collect_operand_uses(op, defined, uses, block);
            }
        }
    }
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
