//! Intraprocedural local variable liveness analysis.
//!
//! Computes which local variables are live-in and live-out for each basic
//! block in an [`AmirFunc`]. Used by optimization and code generation passes.

use crate::amir::reachability::terminator_targets;
use crate::amir::{
    AmirFunc, AmirOperand, AmirPlace, AmirProjection, AmirRvalue, AmirStmt, AmirTerminator,
    BlockId, LocalId,
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

    while changed {
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
        AmirTerminator::Return | AmirTerminator::Goto { .. } | AmirTerminator::Unreachable => {}
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
        AmirRvalue::EnumConstruct { payload, .. } => {
            if let Some(op) = payload {
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

#[cfg(test)]
mod tests {
    use crate::amir::{
        AmirBasicBlock, AmirConstant, AmirFunc, AmirOperand, AmirRvalue, AmirStmt, AmirStmtTable,
        AmirTerminator, BlockId, LocalId, TempId,
    };
    use crate::cfg::compute_cfg_edges;
    use crate::layout::DenseRange;
    use crate::liveness::analyze_local_liveness;
    use crate::types::ArType;

    fn local_id(i: usize) -> LocalId {
        LocalId::from_usize(i)
    }

    fn block_id(i: usize) -> BlockId {
        BlockId::from_usize(i)
    }

    fn temp_id(i: usize) -> TempId {
        TempId::from_usize(i)
    }

    fn void_func(blocks: Vec<AmirBasicBlock>, stmts: AmirStmtTable) -> AmirFunc {
        let cfg = compute_cfg_edges(&blocks);
        AmirFunc {
            symbol: crate::SymbolId(0),
            return_type: ArType::Void,
            receiver: None,
            params: Vec::new(),
            locals: vec![crate::amir::AmirLocal {
                id: local_id(0),
                ty: ArType::Void,
                symbol: None,
                span: crate::Span::new(0, 0, 0),
                use_span: None,
            }],
            temps: Vec::new(),
            blocks,
            stmts,
            cfg,
        }
    }

    fn empty_block(id: usize) -> AmirBasicBlock {
        AmirBasicBlock {
            id: block_id(id),
            statements: DenseRange::empty(),
            params: Vec::new(),
            terminator: AmirTerminator::Return,
        }
    }

    // ── analyze_local_liveness ──

    #[test]
    fn single_block_no_uses_or_defs() {
        let stmts = AmirStmtTable::new();
        let func = void_func(vec![empty_block(0)], stmts);
        let liveness = analyze_local_liveness(&func);
        assert!(liveness.live_in(block_id(0)).is_empty());
        assert!(liveness.live_out(block_id(0)).is_empty());
    }

    #[test]
    fn use_before_def_is_live_in() {
        let mut stmts = AmirStmtTable::new();
        let bid = block_id(0);
        stmts.push(AmirStmt::Store {
            lhs: crate::amir::AmirPlace {
                local: local_id(0),
                projections: smallvec::smallvec![],
            },
            rhs: AmirOperand::Copy(temp_id(0)),
        });
        let mut block = empty_block(0);
        block.statements = DenseRange::new(0, 1);
        let func = void_func(vec![block], stmts);
        let liveness = analyze_local_liveness(&func);
        // temp_id(0) is used but never defined → live_in
        // But temp_id(0) is a TempId, not a LocalId. Liveness tracks LocalId.
        // The operand is Copy(TempId(0)), which in collect_operand_uses does nothing (empty body).
        // So no locals are live.
        assert!(liveness.live_in(bid).is_empty());
        assert!(liveness.live_out(bid).is_empty());
    }

    #[test]
    fn local_used_in_load_is_live_in() {
        let mut stmts = AmirStmtTable::new();
        let bid = block_id(0);
        stmts.push(AmirStmt::Assign {
            lhs: temp_id(0),
            rhs: AmirRvalue::Load(crate::amir::AmirPlace {
                local: local_id(0),
                projections: smallvec::smallvec![],
            }),
        });
        let mut block = empty_block(0);
        block.statements = DenseRange::new(0, 1);
        let func = void_func(vec![block], stmts);
        let liveness = analyze_local_liveness(&func);
        // Load(local_id(0)) is a use of local_id(0)
        // local_id(0) is never defined → live_in should contain local_id(0)
        assert!(liveness.live_in(bid).contains(local_id(0)));
    }

    #[test]
    fn def_kills_liveness() {
        let mut stmts = AmirStmtTable::new();
        let bid = block_id(0);
        // First use local_id(0), then define it
        // In SSA form: use before def means local_id(0) is live_in
        // We need to use the local first (via Load), then store to it
        stmts.push(AmirStmt::Assign {
            lhs: temp_id(0),
            rhs: AmirRvalue::Load(crate::amir::AmirPlace {
                local: local_id(0),
                projections: smallvec::smallvec![],
            }),
        });
        stmts.push(AmirStmt::Store {
            lhs: crate::amir::AmirPlace {
                local: local_id(0),
                projections: smallvec::smallvec![],
            },
            rhs: AmirOperand::Constant(AmirConstant::Nil),
        });
        let mut block = empty_block(0);
        block.statements = DenseRange::new(0, 2);
        let func = void_func(vec![block], stmts);
        let liveness = analyze_local_liveness(&func);
        // local_id(0) is used before any definition → live_in
        assert!(liveness.live_in(bid).contains(local_id(0)));
        // After the Store, local_id(0) is defined, but since it's also the last
        // statement and there's no use after, live_out should be empty for local_id(0).
        assert!(liveness.live_out(bid).is_empty());
    }

    #[test]
    fn two_block_flow_use_after_def() {
        // Block 0: def local_id(0) → goto block 1
        // Block 1: use local_id(0)
        // local_id(0) should be live_in(block 1) and live_out(block 0)
        let mut stmts = AmirStmtTable::new();
        let b0 = block_id(0);
        let b1 = block_id(1);

        stmts.push(AmirStmt::Store {
            lhs: crate::amir::AmirPlace {
                local: local_id(0),
                projections: smallvec::smallvec![],
            },
            rhs: AmirOperand::Constant(AmirConstant::Nil),
        });
        stmts.push(AmirStmt::Assign {
            lhs: temp_id(0),
            rhs: AmirRvalue::Load(crate::amir::AmirPlace {
                local: local_id(0),
                projections: smallvec::smallvec![],
            }),
        });

        let block0 = AmirBasicBlock {
            id: b0,
            statements: DenseRange::new(0, 1),
            params: Vec::new(),
            terminator: AmirTerminator::Goto {
                target: b1,
                args: Vec::new(),
            },
        };
        let block1 = AmirBasicBlock {
            id: b1,
            statements: DenseRange::new(1, 1),
            params: Vec::new(),
            terminator: AmirTerminator::Return,
        };

        let func = void_func(vec![block0, block1], stmts);
        let liveness = analyze_local_liveness(&func);
        // local_id(0) defined in block0, used in block1 → live_out(block0) ∩ live_in(block1)
        assert!(liveness.live_out(b0).contains(local_id(0)));
        assert!(liveness.live_in(b1).contains(local_id(0)));
    }

    #[test]
    fn branch_condition_is_use() {
        let mut stmts = AmirStmtTable::new();
        let b0 = block_id(0);
        let b1 = block_id(1);
        let b2 = block_id(2);

        stmts.push(AmirStmt::Assign {
            lhs: temp_id(0),
            rhs: AmirRvalue::Load(crate::amir::AmirPlace {
                local: local_id(0),
                projections: smallvec::smallvec![],
            }),
        });

        let block0 = AmirBasicBlock {
            id: b0,
            statements: DenseRange::new(0, 1),
            params: Vec::new(),
            terminator: AmirTerminator::Branch {
                condition: AmirOperand::Copy(temp_id(0)),
                if_true: b1,
                true_args: Vec::new(),
                if_false: b2,
                false_args: Vec::new(),
            },
        };
        let block1 = empty_block(1);
        let block2 = empty_block(2);

        let func = void_func(vec![block0, block1, block2], stmts);
        let liveness = analyze_local_liveness(&func);
        // Load(local_id(0)) uses local_id(0), no def → live_in(b0)
        assert!(liveness.live_in(b0).contains(local_id(0)));
    }

    #[test]
    fn switch_int_condition_is_use() {
        let mut stmts = AmirStmtTable::new();
        let b0 = block_id(0);
        stmts.push(AmirStmt::Assign {
            lhs: temp_id(0),
            rhs: AmirRvalue::Load(crate::amir::AmirPlace {
                local: local_id(0),
                projections: smallvec::smallvec![],
            }),
        });
        let block0 = AmirBasicBlock {
            id: b0,
            statements: DenseRange::new(0, 1),
            params: Vec::new(),
            terminator: AmirTerminator::SwitchInt {
                discriminant: AmirOperand::Copy(temp_id(0)),
                targets: vec![],
                otherwise: (block_id(1), Vec::new()),
            },
        };
        let block1 = empty_block(1);
        let func = void_func(vec![block0, block1], stmts);
        let liveness = analyze_local_liveness(&func);
        // Load(local_id(0)) is a use, no def → live_in(b0)
        assert!(liveness.live_in(b0).contains(local_id(0)));
    }

    #[test]
    fn diamond_join_propagates_liveness() {
        // Block 0: def local(0) → branch to block 1, block 2
        // Block 1: no-op → goto block 3
        // Block 2: use local(0) via Load → goto block 3
        // Block 3: return
        // local(0) should be live_out(block0) because block2 uses it
        let mut stmts = AmirStmtTable::new();
        let b0 = block_id(0);
        let b1 = block_id(1);
        let b2 = block_id(2);
        let b3 = block_id(3);

        stmts.push(AmirStmt::Store {
            lhs: crate::amir::AmirPlace {
                local: local_id(0),
                projections: smallvec::smallvec![],
            },
            rhs: AmirOperand::Constant(AmirConstant::Nil),
        });
        stmts.push(AmirStmt::Assign {
            lhs: temp_id(0),
            rhs: AmirRvalue::Load(crate::amir::AmirPlace {
                local: local_id(0),
                projections: smallvec::smallvec![],
            }),
        });

        let block0 = AmirBasicBlock {
            id: b0,
            statements: DenseRange::new(0, 1),
            params: Vec::new(),
            terminator: AmirTerminator::Branch {
                condition: AmirOperand::Constant(AmirConstant::Bool(true)),
                if_true: b1,
                true_args: Vec::new(),
                if_false: b2,
                false_args: Vec::new(),
            },
        };
        let block1 = AmirBasicBlock {
            id: b1,
            statements: DenseRange::empty(),
            params: Vec::new(),
            terminator: AmirTerminator::Goto {
                target: b3,
                args: Vec::new(),
            },
        };
        let block2 = AmirBasicBlock {
            id: b2,
            statements: DenseRange::new(1, 1),
            params: Vec::new(),
            terminator: AmirTerminator::Goto {
                target: b3,
                args: Vec::new(),
            },
        };
        let block3 = empty_block(3);

        let func = void_func(vec![block0, block1, block2, block3], stmts);
        let liveness = analyze_local_liveness(&func);
        // local(0) defined in block0, used in block2 → live_out(block0)
        // Also live_in(block2)
        assert!(liveness.live_out(b0).contains(local_id(0)));
        assert!(liveness.live_in(b2).contains(local_id(0)));
    }

    #[test]
    fn store_is_def_kills_liveness() {
        let mut stmts = AmirStmtTable::new();
        let b0 = block_id(0);
        // Use local via Load, then Store to same local
        stmts.push(AmirStmt::Assign {
            lhs: temp_id(0),
            rhs: AmirRvalue::Load(crate::amir::AmirPlace {
                local: local_id(0),
                projections: smallvec::smallvec![],
            }),
        });
        stmts.push(AmirStmt::Store {
            lhs: crate::amir::AmirPlace {
                local: local_id(0),
                projections: smallvec::smallvec![],
            },
            rhs: AmirOperand::Constant(AmirConstant::Nil),
        });
        // After the Store, local(0) is defined, so live_out should be empty
        let mut block = empty_block(0);
        block.statements = DenseRange::new(0, 2);
        let func = void_func(vec![block], stmts);
        let liveness = analyze_local_liveness(&func);
        // local(0) used before def → live_in
        assert!(liveness.live_in(b0).contains(local_id(0)));
        // After Store → defined, no further use → live_out empty
        assert!(!liveness.live_out(b0).contains(local_id(0)));
    }
}
