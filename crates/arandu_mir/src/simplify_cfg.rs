use crate::amir::{AmirBasicBlock, AmirFunc, AmirStmtTable, AmirTerminator, BlockId};
use crate::cfg::{clear_block, compute_cfg_edges, retarget_successor, transfer_edges};
use crate::layout::DenseRange;
use std::collections::VecDeque;

/// CFG simplification pass: jump threading, block merging, unreachable removal.
///
/// Uses a local worklist inside an outer fixpoint loop so that each
/// transformation feeds the next (e.g. block removal enables new merges).
/// Only `remove_unreachable_blocks` recomputes the full CFG (block indices
/// change); threading and merging update the CFG incrementally.
///
/// Returns `true` if any change was made.
pub fn simplify_cfg(func: &mut AmirFunc) -> bool {
    // Recompute CFG from scratch to catch any terminator changes made by
    // prior passes (SCCP, DCE) — those passes do not update the CFG.
    func.cfg = compute_cfg_edges(&func.blocks);

    let mut changed = false;
    let mut outer_iters = 0u32;

    loop {
        outer_iters += 1;
        if outer_iters > 64 {
            break;
        }

        let mut local = false;

        // Phase 1 — jump threading + block merging with local worklist.
        let mut worklist: VecDeque<BlockId> =
            (0..func.blocks.len()).map(BlockId::from_usize).collect();
        let max_visits = func.blocks.len().saturating_mul(4).max(16);

        let mut visited = 0u64;
        while let Some(bid) = worklist.pop_front() {
            visited += 1;
            if visited > max_visits as u64 {
                break;
            }
            if bid.as_usize() >= func.blocks.len() {
                continue;
            }

            let mut block_changed = false;
            block_changed |= thread_block(func, bid);
            block_changed |= merge_block_candidate(func, bid);

            if block_changed {
                local = true;
                for &pred in func.predecessors(bid) {
                    worklist.push_back(pred);
                }
                for &succ in func.successors(bid) {
                    worklist.push_back(succ);
                }
            }
        }

        // Phase 2 — unreachable removal (full CFG recompute, indices shift).
        local |= remove_unreachable_blocks(func);

        changed |= local;
        if !local {
            break;
        }
    }

    changed
}

// ---------------------------------------------------------------------------
// Jump threading
// ---------------------------------------------------------------------------

/// If `bid` ends with `Goto(target)` and `target` is an empty `Goto`-only
/// chain, bypass the chain and go directly to the final non-trivial block.
fn thread_block(func: &mut AmirFunc, bid: BlockId) -> bool {
    let (target, args) = match &func.block(bid).terminator {
        AmirTerminator::Goto { target, args } => (*target, args),
        _ => return false,
    };
    if !args.is_empty() {
        return false;
    }

    let final_target = resolve_goto_chain(func, target);
    if final_target == target {
        return false;
    }

    func.block_mut(bid).terminator = AmirTerminator::Goto {
        target: final_target,
        args: Vec::new(),
    };
    retarget_successor(&mut func.cfg, bid, target, final_target);
    true
}

fn resolve_goto_chain(func: &AmirFunc, mut block: BlockId) -> BlockId {
    loop {
        let b = func.block(block);
        if !b.statements.is_empty() || !b.params.is_empty() {
            return block;
        }
        match &b.terminator {
            AmirTerminator::Goto { target, args } => {
                if !args.is_empty() {
                    return block;
                }
                block = *target;
            }
            _ => return block,
        }
    }
}

// ---------------------------------------------------------------------------
// Block merging
// ---------------------------------------------------------------------------

/// If `bid` has exactly one successor `succ` and `succ` has exactly one
/// predecessor `bid`, merge them.
fn merge_block_candidate(func: &mut AmirFunc, bid: BlockId) -> bool {
    let succs = func.successors(bid);
    if succs.len() != 1 {
        return false;
    }
    let succ = succs[0];
    if succ.as_usize() >= func.blocks.len() || succ == bid {
        return false;
    }
    if func.predecessors(succ).len() != 1 || func.predecessors(succ)[0] != bid {
        return false;
    }

    merge_two_blocks(func, bid, succ);
    true
}

fn merge_two_blocks(func: &mut AmirFunc, into: BlockId, from: BlockId) {
    let mut new_stmts = AmirStmtTable::new();
    let mut new_ranges: Vec<DenseRange> = Vec::with_capacity(func.blocks.len());

    for bi in 0..func.blocks.len() {
        let bid = BlockId::from_usize(bi);
        let start = new_stmts.len();
        let mut count = 0usize;

        if bid == into {
            for stmt_id in func.block_stmt_ids(bid) {
                new_stmts.push(func.stmt(stmt_id).clone());
                count += 1;
            }
            for stmt_id in func.block_stmt_ids(from) {
                new_stmts.push(func.stmt(stmt_id).clone());
                count += 1;
            }
        } else if bid == from {
            // stmts already merged into `into`
        } else {
            for stmt_id in func.block_stmt_ids(bid) {
                new_stmts.push(func.stmt(stmt_id).clone());
                count += 1;
            }
        }

        new_ranges.push(DenseRange::new(start, count));
    }

    let from_term = func.block(from).terminator.clone();
    func.block_mut(into).terminator = from_term;
    func.stmts = new_stmts;
    for (i, range) in new_ranges.into_iter().enumerate() {
        func.blocks[i].statements = range;
    }

    // Incremental CFG update: `into` inherits `from`'s successors;
    // `from` is cleared and will be removed by unreachable sweep.
    transfer_edges(&mut func.cfg, into, from);
    clear_block(&mut func.cfg, from);
}

// ---------------------------------------------------------------------------
// Unreachable block removal
// ---------------------------------------------------------------------------

fn remove_unreachable_blocks(func: &mut AmirFunc) -> bool {
    let n = func.blocks.len();
    if n == 0 {
        return false;
    }

    let mut reachable = vec![false; n];
    let mut queue = VecDeque::new();
    reachable[0] = true;
    queue.push_back(BlockId::from_usize(0));

    while let Some(bid) = queue.pop_front() {
        let idx = bid.as_usize();
        if idx >= n {
            continue;
        }
        for succ in &func.cfg.successors[idx] {
            let sidx = succ.as_usize();
            if sidx < n && !reachable[sidx] {
                reachable[sidx] = true;
                queue.push_back(*succ);
            }
        }
    }

    let reachable_count = reachable.iter().filter(|&&r| r).count();
    if reachable_count == n {
        return false;
    }

    let mut old_to_new: Vec<Option<BlockId>> = vec![None; n];
    let mut new_idx = 0usize;
    for old in 0..n {
        if reachable[old] {
            old_to_new[old] = Some(BlockId::from_usize(new_idx));
            new_idx += 1;
        }
    }

    let mut new_blocks: Vec<AmirBasicBlock> = Vec::with_capacity(reachable_count);
    for old in 0..n {
        if !reachable[old] {
            continue;
        }
        let new_term = remap_terminator(&func.blocks[old].terminator, &old_to_new);
        new_blocks.push(AmirBasicBlock {
            id: old_to_new[old].unwrap(),
            statements: func.blocks[old].statements,
            params: func.blocks[old].params.clone(),
            terminator: new_term,
        });
    }

    let mut new_stmts = AmirStmtTable::new();
    let mut new_ranges: Vec<DenseRange> = Vec::with_capacity(reachable_count);

    for (old, &reached) in reachable.iter().enumerate() {
        if !reached {
            continue;
        }
        let start = new_stmts.len();
        let mut count = 0usize;
        for stmt_id in func.block_stmt_ids(BlockId::from_usize(old)) {
            new_stmts.push(func.stmt(stmt_id).clone());
            count += 1;
        }
        new_ranges.push(DenseRange::new(start, count));
    }

    func.blocks = new_blocks;
    func.stmts = new_stmts;

    for (block, range) in func.blocks.iter_mut().zip(new_ranges) {
        block.statements = range;
    }

    func.cfg = compute_cfg_edges(&func.blocks);
    true
}

// ---------------------------------------------------------------------------
// Terminator helpers
// ---------------------------------------------------------------------------

fn remap_terminator(term: &AmirTerminator, map: &[Option<BlockId>]) -> AmirTerminator {
    match term {
        AmirTerminator::Goto { target, args } => {
            let new_target = map[target.as_usize()].unwrap_or(*target);
            AmirTerminator::Goto {
                target: new_target,
                args: args.clone(),
            }
        }
        AmirTerminator::Branch {
            condition,
            if_true,
            true_args,
            if_false,
            false_args,
        } => {
            let new_t = map[if_true.as_usize()].unwrap_or(*if_true);
            let new_f = map[if_false.as_usize()].unwrap_or(*if_false);
            AmirTerminator::Branch {
                condition: condition.clone(),
                if_true: new_t,
                true_args: true_args.clone(),
                if_false: new_f,
                false_args: false_args.clone(),
            }
        }
        AmirTerminator::SwitchInt {
            discriminant,
            targets,
            otherwise,
        } => {
            let new_otherwise = (
                map[otherwise.0.as_usize()].unwrap_or(otherwise.0),
                otherwise.1.clone(),
            );
            let new_targets: Vec<_> = targets
                .iter()
                .map(|(val, b, args)| (*val, map[b.as_usize()].unwrap_or(*b), args.clone()))
                .collect();
            AmirTerminator::SwitchInt {
                discriminant: discriminant.clone(),
                targets: new_targets,
                otherwise: new_otherwise,
            }
        }
        _ => term.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amir::program::extend_block_range;
    use crate::amir::{
        AmirBasicBlock, AmirConstant, AmirOperand, AmirPlace, AmirStmt, AmirTemp, AmirTerminator,
        LocalId, TempId,
    };
    use crate::cfg::compute_cfg_edges;
    use crate::layout::DenseRange;
    use crate::passes::type_checker::types::{ArType, Primitive};
    use smallvec::smallvec;

    fn bbid(id: usize) -> BlockId {
        BlockId::from_usize(id)
    }

    fn int_temp(id: usize) -> AmirTemp {
        AmirTemp {
            id: TempId::from_usize(id),
            ty: ArType::Primitive(Primitive::Int),
            span: arandu_lexer::Span::new(0, 0, 0),
        }
    }

    fn block(id: usize, stmts: Vec<AmirStmt>, stmts_table: &mut AmirStmtTable) -> AmirBasicBlock {
        let mut range = DenseRange::empty();
        for stmt in stmts {
            let instr = stmts_table.push(stmt);
            extend_block_range(&mut range, instr);
        }
        AmirBasicBlock {
            id: BlockId::from_usize(id),
            statements: range,
            params: Vec::new(),
            terminator: AmirTerminator::Return,
        }
    }

    fn make_func(blocks: Vec<AmirBasicBlock>, stmts: AmirStmtTable) -> AmirFunc {
        let cfg = compute_cfg_edges(&blocks);
        AmirFunc {
            symbol: crate::SymbolId(0),
            return_type: ArType::Void,
            receiver: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: vec![int_temp(0)],
            blocks,
            stmts,
            cfg,
        }
    }

    // ── Jump threading ──

    #[test]
    fn jump_thread_skips_empty_goto_chain() {
        let mut st = AmirStmtTable::new();
        let mut func = make_func(
            vec![
                AmirBasicBlock {
                    id: bbid(0),
                    statements: DenseRange::empty(),
                    params: Vec::new(),
                    terminator: AmirTerminator::Goto {
                        target: bbid(1),
                        args: Vec::new(),
                    },
                },
                AmirBasicBlock {
                    id: bbid(1),
                    statements: DenseRange::empty(),
                    params: Vec::new(),
                    terminator: AmirTerminator::Goto {
                        target: bbid(2),
                        args: Vec::new(),
                    },
                },
                block(2, vec![], &mut st),
            ],
            st,
        );
        func.cfg = compute_cfg_edges(&func.blocks);

        assert!(simplify_cfg(&mut func));
        // After threading (bb0→bb1→bb2 become bb0→bb2) then merging
        // (bb0 + bb2) and unreachable removal: only 1 block remains.
        assert_eq!(func.blocks.len(), 1);
        assert!(matches!(
            func.block(bbid(0)).terminator,
            AmirTerminator::Return
        ));
    }

    #[test]
    fn jump_thread_no_change_when_no_chain() {
        let mut st = AmirStmtTable::new();
        let mut func = make_func(vec![block(0, vec![], &mut st)], st);
        func.cfg = compute_cfg_edges(&func.blocks);
        assert!(!simplify_cfg(&mut func));
    }

    // ── Merge blocks ──

    #[test]
    fn merge_single_pred_single_succ() {
        let mut st = AmirStmtTable::new();
        let _i0 = st.push(AmirStmt::Store {
            lhs: AmirPlace {
                local: LocalId::from_usize(0),
                projections: smallvec![],
            },
            rhs: AmirOperand::Constant(AmirConstant::Bool(true)),
        });
        let _i1 = st.push(AmirStmt::Store {
            lhs: AmirPlace {
                local: LocalId::from_usize(1),
                projections: smallvec![],
            },
            rhs: AmirOperand::Constant(AmirConstant::Bool(false)),
        });

        let mut func = make_func(
            vec![
                AmirBasicBlock {
                    id: bbid(0),
                    statements: DenseRange::new(0, 1),
                    params: Vec::new(),
                    terminator: AmirTerminator::Goto {
                        target: bbid(1),
                        args: Vec::new(),
                    },
                },
                AmirBasicBlock {
                    id: bbid(1),
                    statements: DenseRange::new(1, 1),
                    params: Vec::new(),
                    terminator: AmirTerminator::Return,
                },
            ],
            st,
        );
        func.cfg = compute_cfg_edges(&func.blocks);

        assert!(simplify_cfg(&mut func));

        let b0 = func.block(bbid(0));
        assert_eq!(b0.statements.len, 2);
        assert!(matches!(b0.terminator, AmirTerminator::Return));
    }

    // ── Remove unreachable ──

    #[test]
    fn remove_unreachable_removes_orphaned_blocks() {
        let mut st = AmirStmtTable::new();
        let mut func = make_func(
            vec![
                block(0, vec![], &mut st),
                block(1, vec![], &mut st),
                block(2, vec![], &mut st),
            ],
            st,
        );
        func.blocks[0].terminator = AmirTerminator::Return;
        func.cfg = compute_cfg_edges(&func.blocks);

        assert!(simplify_cfg(&mut func));
        assert_eq!(func.blocks.len(), 1);
    }

    #[test]
    fn remove_unreachable_rewrites_terminator_targets() {
        let mut st = AmirStmtTable::new();
        let mut func = make_func(
            vec![
                AmirBasicBlock {
                    id: bbid(0),
                    statements: DenseRange::empty(),
                    params: Vec::new(),
                    terminator: AmirTerminator::Goto {
                        target: bbid(1),
                        args: Vec::new(),
                    },
                },
                block(1, vec![], &mut st),
                block(2, vec![], &mut st),
            ],
            st,
        );
        func.cfg = compute_cfg_edges(&func.blocks);

        assert!(simplify_cfg(&mut func));
        // bb0 merges with bb1 (single-pred+single-succ), then unreachable
        // sweep removes bb2.  Only bb0 survives with bb1's Return.
        assert_eq!(func.blocks.len(), 1);
        assert!(matches!(
            func.block(bbid(0)).terminator,
            AmirTerminator::Return
        ));
    }

    #[test]
    fn no_change_when_all_reachable() {
        let mut st = AmirStmtTable::new();
        let mut func = make_func(vec![block(0, vec![], &mut st)], st);
        func.cfg = compute_cfg_edges(&func.blocks);
        assert!(!simplify_cfg(&mut func));
    }

    // ── simplify_cfg integration ──

    #[test]
    fn simplify_cfg_removes_goto_chain() {
        let mut st = AmirStmtTable::new();
        let mut func = make_func(
            vec![
                AmirBasicBlock {
                    id: bbid(0),
                    statements: DenseRange::empty(),
                    params: Vec::new(),
                    terminator: AmirTerminator::Goto {
                        target: bbid(1),
                        args: Vec::new(),
                    },
                },
                AmirBasicBlock {
                    id: bbid(1),
                    statements: DenseRange::empty(),
                    params: Vec::new(),
                    terminator: AmirTerminator::Goto {
                        target: bbid(2),
                        args: Vec::new(),
                    },
                },
                block(2, vec![], &mut st),
            ],
            st,
        );
        func.cfg = compute_cfg_edges(&func.blocks);

        assert!(simplify_cfg(&mut func));
        assert_eq!(func.blocks.len(), 1);
    }
}
