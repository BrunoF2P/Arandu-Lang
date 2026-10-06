//! Loop-Invariant Code Motion (LICM) pass for AMIR.
//!
//! Hoists computations that are loop-invariant out of their loop, so the
//! register allocator can keep them in registers across iterations instead of
//! rematerialising them from stack slots on every pass.
//!
//! ## Speculation safety
//!
//! Hoisting is only sound if the instruction would have executed on every path
//! that reaches the loop header. Two guards enforce that here:
//!
//! 1. Only *pure* rvalues move: binary/unary arithmetic and plain use. Loads,
//!    calls, allocations and anything that can trap stay put, because hoisting
//!    a trapping load into a preheader can introduce a fault on a path that
//!    never executed it.
//! 2. The defining block must **dominate the loop header**. That guarantees the
//!    computation already happened before the loop is entered, so running it
//!    once in the preheader performs no extra work and cannot fault.
//!
//! Together these make the transform semantics-preserving even for partially
//! executed loops: we never speculate a computation earlier than the original
//! program evaluated it.

use crate::amir::{
    AmirBasicBlock, AmirFunc, AmirOperand, AmirRvalue, AmirStmt, AmirStmtTable, BlockId,
    Dominators, InstrId, TempId,
};
use rustc_hash::FxHashSet;

/// A natural loop reduced to what LICM needs.
#[derive(Debug, Clone)]
struct LoopInfo {
    header: BlockId,
    /// The single predecessor outside the loop, where hoisted code is placed.
    preheader: BlockId,
    /// Blocks in the loop, header included.
    body: Vec<BlockId>,
}

/// Applies Loop-Invariant Code Motion to `func`.
///
/// Returns `true` if any instruction was hoisted.
pub fn licm(func: &mut AmirFunc) -> bool {
    if func.blocks.is_empty() || func.stmts.is_empty() {
        return false;
    }

    let doms = Dominators::new(func);
    let order = crate::amir::reverse_post_order(func);
    let loops = discover_loops(func, &doms);

    // Bigger loops first so deep nesting is scheduled before the loops that
    // contain it; an outer pass then sees inner-hoisted values as invariant.
    let mut worklist = loops;
    worklist.sort_by_key(|l| std::cmp::Reverse(l.body.len()));

    let mut changed = false;
    for lp in &worklist {
        // Re-derive dominators between hoists: moving statements can change
        // block shapes, and stale idom data would admit unsafe candidates.
        let fresh = Dominators::new(func);
        if hoist_loop(func, lp, &fresh, &order) {
            changed = true;
        }
    }

    changed
}

/// Finds natural loops that have a unique preheader candidate.
fn discover_loops(func: &AmirFunc, doms: &Dominators) -> Vec<LoopInfo> {
    let mut headers: Vec<BlockId> = Vec::new();
    let mut back_edges: Vec<(BlockId, BlockId)> = Vec::new();
    for (bid, succs) in successors_of(func) {
        for succ in succs {
            // Back edge: the successor dominates this block.
            if doms.dominates(succ, bid) {
                back_edges.push((bid, succ));
                if !headers.contains(&succ) {
                    headers.push(succ);
                }
            }
        }
    }
    if headers.is_empty() {
        return Vec::new();
    }

    let mut loops = Vec::new();
    for header in headers {
        // Natural loop body: walk predecessors starting from the *back-edge
        // sources*, never from the header itself. Seeding the worklist with the
        // header would pull the preheader into the body (it is a predecessor of
        // the header), which then makes every loop look like it has no outside
        // edge and silently disables LICM.
        let mut body = vec![header];
        let mut worklist: Vec<BlockId> = back_edges
            .iter()
            .filter(|&&(_, v)| v == header)
            .map(|&(u, _)| u)
            .filter(|&u| !body.contains(&u))
            .collect();
        for &b in &worklist {
            body.push(b);
        }
        while let Some(b) = worklist.pop() {
            for &p in func.predecessors(b) {
                // `p != header` stops the walk at the header instead of
                // escaping through the preheader.
                if p != header && !body.contains(&p) {
                    body.push(p);
                    worklist.push(p);
                }
            }
        }

        let mut outside = Vec::new();
        for &p in func.predecessors(header) {
            if !body.contains(&p) {
                outside.push(p);
            }
        }
        // Zero outside predecessors means the header *is* the entry; several
        // means there is no single edge to split into a preheader. Either way
        // we cannot place hoisted code, so skip the loop.
        if outside.len() != 1 {
            continue;
        }
        loops.push(LoopInfo {
            header,
            preheader: outside[0],
            body,
        });
    }
    loops
}

fn successors_of(func: &AmirFunc) -> Vec<(BlockId, Vec<BlockId>)> {
    (0..func.blocks.len())
        .map(|i| {
            let b = BlockId::from_usize(i);
            let succs = func.successors(b).to_vec();
            (b, succs)
        })
        .collect()
}

/// Temps that hold the same value on every iteration of `lp` **and** are
/// already available at the preheader.
///
/// Availability matters separately from invariance: a value can be invariant
/// yet defined in a sibling block that does not reach the preheader. Hoisting a
/// use of it upward would then read an undefined temp, so we require the
/// definition to dominate the preheader.
///
/// Included: function params, the receiver, preheader block params, header
/// block params whose incoming values are themselves available, and temps whose
/// every definition dominates the preheader.
fn compute_invariant(func: &AmirFunc, lp: &LoopInfo, doms: &Dominators) -> FxHashSet<TempId> {
    let mut invariant: FxHashSet<TempId> = FxHashSet::default();

    for &p in &func.params {
        invariant.insert(p);
    }
    if let Some(receiver) = &func.receiver {
        invariant.insert(receiver.temp);
    }
    for bp in func.block_params(func.block(lp.preheader).params) {
        invariant.insert(bp.id);
    }

    // Header params take their value from the incoming jump args. A param is
    // only invariant when **every** incoming edge supplies an available value:
    // with several predecessors, one loop-variant argument is enough to make it
    // change per iteration.
    let header_params: Vec<TempId> = func
        .block_params(func.block(lp.header).params)
        .iter()
        .map(|p| p.id)
        .collect();
    let mut all_available = vec![true; header_params.len()];
    for &pred in func.predecessors(lp.header) {
        let incoming = incoming_args(func, lp.header, pred);
        // A short argument list leaves the remaining params undefined on this
        // edge, which disqualifies them just like an unavailable operand.
        if incoming.len() < header_params.len() {
            all_available.iter_mut().for_each(|slot| *slot = false);
        }
        for (slot, (_param, arg)) in incoming.iter().enumerate() {
            let available = match *arg {
                AmirOperand::Copy(t) | AmirOperand::Move(t) => invariant.contains(&t),
                AmirOperand::Constant(_)
                | AmirOperand::FunctionRef(_)
                | AmirOperand::GlobalRef(_) => true,
            };
            if !available && slot < all_available.len() {
                all_available[slot] = false;
            }
        }
    }
    for (slot, &param) in header_params.iter().enumerate() {
        if all_available[slot] {
            invariant.insert(param);
        }
    }

    // A temp is loop-invariant-and-available when it is never assigned inside
    // the loop and at least one of its definitions dominates the preheader.
    for temp in 0..func.temps.len() {
        let t = TempId::from_usize(temp);
        if invariant.contains(&t) {
            continue;
        }
        let mut defined_in_loop = false;
        let mut dominates_preheader = false;
        for bid in 0..func.blocks.len() {
            let bid = BlockId::from_usize(bid);
            for stmt_id in func.block_stmt_ids(bid) {
                if let Some(AmirStmt::Assign { lhs, .. }) = func.try_stmt(stmt_id)
                    && *lhs == t
                {
                    if lp.body.contains(&bid) {
                        defined_in_loop = true;
                    } else if doms.dominates(bid, lp.preheader) {
                        dominates_preheader = true;
                    }
                }
            }
        }
        if !defined_in_loop && dominates_preheader {
            invariant.insert(t);
        }
    }

    invariant
}

/// Yields `(header param, incoming operand)` for each edge `pred -> header`.
fn incoming_args(func: &AmirFunc, header: BlockId, pred: BlockId) -> Vec<(TempId, AmirOperand)> {
    let params: Vec<TempId> = func
        .block_params(func.block(header).params)
        .iter()
        .map(|p| p.id)
        .collect();
    let args: Vec<AmirOperand> = match &func.block(pred).terminator {
        crate::amir::AmirTerminator::Goto { args, .. } => args.clone(),
        crate::amir::AmirTerminator::Branch {
            if_true,
            true_args,
            if_false,
            false_args,
            ..
        } => {
            if *if_true == header {
                true_args.clone()
            } else if *if_false == header {
                false_args.clone()
            } else {
                Vec::new()
            }
        }
        crate::amir::AmirTerminator::SwitchInt {
            targets, otherwise, ..
        } => {
            let mut out = Vec::new();
            for (_, t, a) in targets {
                if *t == header {
                    out.extend(a.iter().copied());
                }
            }
            if otherwise.0 == header {
                out.extend(otherwise.1.iter().copied());
            }
            out
        }
        crate::amir::AmirTerminator::Return
        | crate::amir::AmirTerminator::Unreachable
        | crate::amir::AmirTerminator::Suspend { .. } => Vec::new(),
    };
    params.into_iter().zip(args).collect()
}

/// Whether `rv` is safe to execute earlier than it appears.
fn is_speculatable(rv: &AmirRvalue) -> bool {
    match rv {
        AmirRvalue::Use(_) | AmirRvalue::Binary { .. } | AmirRvalue::Unary { .. } => true,
        // Loads, calls, allocations and projections may trap or have effects.
        _ => false,
    }
}

fn operands(rv: &AmirRvalue) -> Vec<AmirOperand> {
    match rv {
        AmirRvalue::Use(op) => vec![*op],
        AmirRvalue::Binary { left, right, .. } => vec![*left, *right],
        AmirRvalue::Unary { operand, .. } => vec![*operand],
        _ => Vec::new(),
    }
}

fn operand_is_invariant(op: &AmirOperand, invariant: &FxHashSet<TempId>) -> bool {
    match op {
        AmirOperand::Copy(t) | AmirOperand::Move(t) => invariant.contains(t),
        AmirOperand::Constant(_) | AmirOperand::FunctionRef(_) | AmirOperand::GlobalRef(_) => true,
    }
}

fn hoist_loop(func: &mut AmirFunc, lp: &LoopInfo, doms: &Dominators, order: &[BlockId]) -> bool {
    let invariant = compute_invariant(func, lp, doms);

    let mut candidates: Vec<(BlockId, InstrId, TempId)> = Vec::new();
    for &bid in &lp.body {
        for stmt_id in func.block_stmt_ids(bid) {
            let Some(AmirStmt::Assign { lhs, rhs }) = func.try_stmt(stmt_id) else {
                continue;
            };
            // The result must not already be available at the preheader, and
            // the computation must be safe to run earlier than written.
            if invariant.contains(lhs) || !is_speculatable(rhs) {
                continue;
            }
            // Every operand must already hold its loop-invariant value in the
            // preheader. Combined with `is_speculatable` this is what makes the
            // move safe even when the body never executes.
            if !operands(rhs)
                .iter()
                .all(|op| operand_is_invariant(op, &invariant))
            {
                continue;
            }
            candidates.push((bid, stmt_id, *lhs));
        }
    }

    if candidates.is_empty() {
        return false;
    }

    // Deterministic order keeps the transform reproducible.
    let rank = |b: &BlockId| order.iter().position(|x| x == b).unwrap_or(usize::MAX);
    candidates.sort_by_key(|(b, s, _)| (rank(b), s.as_usize()));

    let mut hoisted: Vec<AmirStmt> = Vec::new();
    let mut blanked: Vec<InstrId> = Vec::new();
    for (_, stmt_id, lhs) in &candidates {
        if let Some(AmirStmt::Assign { rhs, .. }) = func.try_stmt(*stmt_id) {
            hoisted.push(AmirStmt::Assign {
                lhs: *lhs,
                rhs: rhs.clone(),
            });
            blanked.push(*stmt_id);
        }
    }
    if hoisted.is_empty() {
        return false;
    }

    let preheader_ids: Vec<InstrId> = func.block_stmt_ids(lp.preheader).collect();
    let mut table = AmirStmtTable::new();
    let mut blocks: Vec<AmirBasicBlock> = Vec::with_capacity(func.blocks.len());

    for (bi, block) in func.blocks.iter().enumerate() {
        let bid = BlockId::from_usize(bi);
        let start = table.len();
        if bid == lp.preheader {
            for id in &preheader_ids {
                if let Some(s) = func.try_stmt(*id) {
                    table.push(s.clone());
                }
            }
            for s in &hoisted {
                table.push(s.clone());
            }
        } else {
            for id in func.block_stmt_ids(bid) {
                if blanked.contains(&id) {
                    table.push(AmirStmt::Nop);
                } else if let Some(s) = func.try_stmt(id) {
                    table.push(s.clone());
                }
            }
        }
        let len = table.len() - start;
        blocks.push(AmirBasicBlock {
            id: block.id,
            params: block.params,
            statements: crate::layout::DenseRange::new(start, len),
            terminator: block.terminator.clone(),
        });
    }

    func.stmts = table;
    func.blocks = blocks;
    func.cfg = crate::cfg::compute_cfg_edges(&func.blocks);
    true
}
