//! F2.1 — Intraprocedural may-borrow dataflow over AMIR (dense bitsets / A9).
//!
//! Tracks which stack locals **may** be under an active loan at each block
//! boundary:
//! - **shared** — loaned via [`AmirRvalue::Borrow`] (`&T`)
//! - **exclusive** — loaned via [`AmirRvalue::BorrowMut`] (`&mut T`)
//!
//! ## Lattice
//!
//! Join at merge points is **union** (may-analysis): a local is considered
//! borrowed at a join if *any* predecessor still has it borrowed. That is
//! sound for conflict detection (M2): over-approximating loans only adds
//! false positives under incomplete end-of-loan (refined by F2.2 liveness
//! windows).
//!
//! ## Transfer
//!
//! | Event | Effect |
//! |-------|--------|
//! | `Borrow(place)` | `shared += place.local` |
//! | `BorrowMut(place)` | `exclusive += place.local` |
//! | `StorageDead(local)` | kill both bits for `local` |
//!
//! Reassignment / Destroy do **not** kill loans: overwriting or freeing a
//! borrowed local is exactly what M2 (O002/O006) will reject. Loan *end*
//! without storage death is F2.2 (`live range` of the reference temp).
//!
//! Pure analysis — no diagnostics (those are M2). Salsa only memoizes
//! compact [`borrow_in_counts`] / query wrappers in `arandu_query`.

use crate::BitSet;
use crate::amir::{AmirFunc, AmirRvalue, AmirStmt, AmirTerminator, BlockId, LocalId};
use std::collections::VecDeque;

/// May-borrowed state for all locals at one program point.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BorrowState {
    /// Locals that may be under a shared (`&`) loan.
    pub shared: BitSet<LocalId>,
    /// Locals that may be under an exclusive (`&mut`) loan.
    pub exclusive: BitSet<LocalId>,
}

impl BorrowState {
    #[must_use]
    pub fn new(num_locals: usize) -> Self {
        Self {
            shared: BitSet::with_capacity(num_locals),
            exclusive: BitSet::with_capacity(num_locals),
        }
    }

    #[must_use]
    pub fn maybe_shared(&self, local: LocalId) -> bool {
        self.shared.contains(local)
    }

    #[must_use]
    pub fn maybe_exclusive(&self, local: LocalId) -> bool {
        self.exclusive.contains(local)
    }

    #[must_use]
    pub fn maybe_borrowed(&self, local: LocalId) -> bool {
        self.maybe_shared(local) || self.maybe_exclusive(local)
    }

    /// Join of predecessor OUT sets (union / may-analysis).
    #[tracing::instrument(level = "trace", target = "arandu_mir::borrow_facts", skip_all)]
    fn join_predecessors<'a>(preds: impl Iterator<Item = &'a Self>, num_locals: usize) -> Self {
        let mut preds = preds.peekable();
        let Some(first) = preds.next() else {
            return Self::new(num_locals);
        };
        let mut acc = first.clone();
        for pred in preds {
            acc.shared.union_with(&pred.shared);
            acc.exclusive.union_with(&pred.exclusive);
        }
        acc
    }

    fn apply_rvalue(&mut self, rhs: &AmirRvalue) {
        match rhs {
            AmirRvalue::Borrow(place) => {
                self.shared.insert(place.local);
            }
            AmirRvalue::BorrowMut(place) => {
                self.exclusive.insert(place.local);
            }
            _ => {}
        }
    }

    fn apply_stmt(&mut self, stmt: &AmirStmt) {
        match stmt {
            AmirStmt::Assign { rhs, .. } => self.apply_rvalue(rhs),
            AmirStmt::StorageDead(local) => {
                self.shared.remove(*local);
                self.exclusive.remove(*local);
            }
            AmirStmt::Store { .. }
            | AmirStmt::Call { .. }
            | AmirStmt::Free(_)
            | AmirStmt::Destroy(_)
            | AmirStmt::StorageLive(_)
            | AmirStmt::Nop => {}
        }
    }

    fn apply_block(&mut self, block: BlockId, func: &AmirFunc) -> u32 {
        let mut sites = 0u32;
        for stmt in func.block_stmts(block) {
            if let AmirStmt::Assign {
                rhs: AmirRvalue::Borrow(_) | AmirRvalue::BorrowMut(_),
                ..
            } = stmt
            {
                sites += 1;
            }
            self.apply_stmt(stmt);
        }
        // Terminators never open loans in AMIR today.
        let _ = &func.block(block).terminator;
        sites
    }
}

/// Full-function borrow facts (per-block IN/OUT + site counts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuncBorrowFacts {
    pub block_in: Vec<BorrowState>,
    pub block_out: Vec<BorrowState>,
    /// Number of `Borrow`/`BorrowMut` rvalues in each block.
    pub borrow_site_counts: Vec<u32>,
}

impl FuncBorrowFacts {
    #[must_use]
    pub fn maybe_shared_at_entry(&self, block: BlockId, local: LocalId) -> bool {
        self.block_in
            .get(block.as_usize())
            .is_some_and(|s| s.maybe_shared(local))
    }

    #[must_use]
    pub fn maybe_exclusive_at_entry(&self, block: BlockId, local: LocalId) -> bool {
        self.block_in
            .get(block.as_usize())
            .is_some_and(|s| s.maybe_exclusive(local))
    }

    #[must_use]
    pub fn maybe_borrowed_at_entry(&self, block: BlockId, local: LocalId) -> bool {
        self.block_in
            .get(block.as_usize())
            .is_some_and(|s| s.maybe_borrowed(local))
    }
}

/// Forward may-borrow dataflow over the CFG (same worklist class as move/init).
#[must_use]
pub fn analyze_borrow_facts(func: &AmirFunc) -> FuncBorrowFacts {
    let num_locals = func.locals.len();
    let num_blocks = func.blocks.len();

    if num_blocks == 0 {
        return FuncBorrowFacts {
            block_in: vec![],
            block_out: vec![],
            borrow_site_counts: vec![],
        };
    }

    let mut block_in = vec![BorrowState::new(num_locals); num_blocks];
    let mut block_out = vec![BorrowState::new(num_locals); num_blocks];
    let mut borrow_site_counts = vec![0u32; num_blocks];
    let mut worklist = VecDeque::new();

    for block in &func.blocks {
        worklist.push_back(block.id);
    }

    let mut iterations = 0;
    let sanity_limit = num_blocks * num_locals.max(1) * 2 + 1000;

    while let Some(bid) = worklist.pop_front() {
        iterations += 1;
        assert!(
            iterations <= sanity_limit,
            "borrow facts dataflow failed to converge: {iterations} > {sanity_limit} ({num_blocks} blocks)"
        );

        let bi = bid.as_usize();
        let block = &func.blocks[bi];

        let new_in = if bid == BlockId::from_usize(0) || func.predecessors(bid).is_empty() {
            BorrowState::new(num_locals)
        } else {
            BorrowState::join_predecessors(
                func.predecessors(bid)
                    .iter()
                    .map(|pred| &block_out[pred.as_usize()]),
                num_locals,
            )
        };

        let mut new_out = new_in.clone();
        let sites = new_out.apply_block(bid, func);
        borrow_site_counts[bi] = sites;

        // Note: OUT is not monotonic under `StorageDead` kills; we recompute
        // from IN each visit so the worklist still reaches a fixpoint.

        if new_in != block_in[bi] || new_out != block_out[bi] {
            block_in[bi] = new_in;
            block_out[bi] = new_out;
            for succ in successors(&block.terminator) {
                worklist.push_back(succ);
            }
        }
    }

    FuncBorrowFacts {
        block_in,
        block_out,
        borrow_site_counts,
    }
}

/// Shared-loan cardinality at each block entry (for Salsa / HashEq).
#[must_use]
pub fn shared_in_counts(func: &AmirFunc) -> Vec<u32> {
    analyze_borrow_facts(func)
        .block_in
        .iter()
        .map(|s| s.shared.len() as u32)
        .collect()
}

/// Exclusive-loan cardinality at each block entry.
#[must_use]
pub fn exclusive_in_counts(func: &AmirFunc) -> Vec<u32> {
    analyze_borrow_facts(func)
        .block_in
        .iter()
        .map(|s| s.exclusive.len() as u32)
        .collect()
}

/// Compact per-block borrow summary for memoization (no bitsets in the query result).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockBorrowSummary {
    pub shared_in: u32,
    pub exclusive_in: u32,
    pub borrow_sites: u32,
}

/// Summaries for all blocks in one pure call (avoids re-running dataflow per block).
#[must_use]
pub fn block_borrow_summaries(func: &AmirFunc) -> Vec<BlockBorrowSummary> {
    let facts = analyze_borrow_facts(func);
    facts
        .block_in
        .iter()
        .zip(facts.borrow_site_counts.iter())
        .map(|(st, &sites)| BlockBorrowSummary {
            shared_in: st.shared.len() as u32,
            exclusive_in: st.exclusive.len() as u32,
            borrow_sites: sites,
        })
        .collect()
}

fn successors(term: &AmirTerminator) -> impl Iterator<Item = BlockId> + '_ {
    crate::amir::reachability::terminator_targets(term).into_iter()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::Span;
    use crate::amir::{
        AmirBasicBlock, AmirFunc, AmirLocal, AmirOperand, AmirPlace, AmirRvalue, AmirStmt,
        AmirStmtTable, AmirTemp, AmirTerminator, TempId,
    };
    use crate::cfg::compute_cfg_edges;
    use crate::layout::DenseRange;
    use crate::types::{ArType, Primitive, TypeInterner};
    use smallvec::smallvec;

    fn intern_ty(ty: ArType) -> crate::types::TypeId {
        TypeInterner::new().intern(ty)
    }

    fn local(i: usize, ty: crate::types::TypeId) -> AmirLocal {
        AmirLocal {
            id: LocalId::from_usize(i),
            ty,
            is_memory: true,
            symbol: None,
            span: Span::new(0, 0, 0),
            use_span: None,
        }
    }

    fn temp(i: usize, ty: crate::types::TypeId) -> AmirTemp {
        AmirTemp {
            id: TempId::from_usize(i),
            ty,
            is_copy: true,
            is_nullable: false,
            span: Span::new(0, 0, 0),
        }
    }

    fn place(l: usize) -> AmirPlace {
        AmirPlace {
            local: LocalId::from_usize(l),
            projections: smallvec![],
        }
    }

    /// Single block: `s0 = …; t0 = &s0`
    #[test]
    fn borrow_marks_shared_at_block_out() {
        let int = intern_ty(ArType::Primitive(Primitive::Int));
        let ref_int = intern_ty(ArType::Ref(int));
        let mut stmts = AmirStmtTable::new();
        stmts.push(AmirStmt::Store {
            lhs: place(0),
            rhs: AmirOperand::Constant(crate::amir::AmirConstant::Pool(
                crate::literal_pool::LiteralId(0),
            )),
        });
        stmts.push(AmirStmt::Assign {
            lhs: TempId::from_usize(0),
            rhs: AmirRvalue::Borrow(place(0)),
        });
        let block = AmirBasicBlock {
            id: BlockId::from_usize(0),
            statements: DenseRange::new(0, 2),
            params: vec![],
            terminator: AmirTerminator::Return,
        };
        let blocks = vec![block];
        let cfg = compute_cfg_edges(&blocks);
        let func = AmirFunc {
            symbol: crate::SymbolId::new(0, 0),
            return_type: int,
            receiver: None,
            params: vec![],
            locals: vec![local(0, int)],
            temps: vec![temp(0, ref_int)],
            blocks,
            stmts,
            cfg,
        };

        let facts = analyze_borrow_facts(&func);
        assert_eq!(facts.borrow_site_counts[0], 1);
        // Entry empty; after transfer, s0 is shared-borrowed.
        assert!(!facts.maybe_shared_at_entry(BlockId::from_usize(0), LocalId::from_usize(0)));
        assert!(facts.block_out[0].maybe_shared(LocalId::from_usize(0)));
        assert!(!facts.block_out[0].maybe_exclusive(LocalId::from_usize(0)));
    }

    /// Two blocks: borrow in bb0, bb1 should see shared at entry.
    #[test]
    fn borrow_propagates_to_successor_entry() {
        let int = intern_ty(ArType::Primitive(Primitive::Int));
        let ref_int = intern_ty(ArType::Ref(int));
        let mut stmts = AmirStmtTable::new();
        // bb0: t0 = &s0
        stmts.push(AmirStmt::Assign {
            lhs: TempId::from_usize(0),
            rhs: AmirRvalue::Borrow(place(0)),
        });
        // bb1: nop
        stmts.push(AmirStmt::Nop);

        let bb0 = AmirBasicBlock {
            id: BlockId::from_usize(0),
            statements: DenseRange::new(0, 1),
            params: vec![],
            terminator: AmirTerminator::Goto {
                target: BlockId::from_usize(1),
                args: vec![],
            },
        };
        let bb1 = AmirBasicBlock {
            id: BlockId::from_usize(1),
            statements: DenseRange::new(1, 1),
            params: vec![],
            terminator: AmirTerminator::Return,
        };
        let blocks = vec![bb0, bb1];
        let cfg = compute_cfg_edges(&blocks);
        let func = AmirFunc {
            symbol: crate::SymbolId::new(0, 0),
            return_type: int,
            receiver: None,
            params: vec![],
            locals: vec![local(0, int)],
            temps: vec![temp(0, ref_int)],
            blocks,
            stmts,
            cfg,
        };

        let facts = analyze_borrow_facts(&func);
        assert!(facts.maybe_shared_at_entry(BlockId::from_usize(1), LocalId::from_usize(0)));
        assert_eq!(facts.borrow_site_counts[0], 1);
        assert_eq!(facts.borrow_site_counts[1], 0);
    }

    #[test]
    fn borrow_mut_marks_exclusive() {
        let int = intern_ty(ArType::Primitive(Primitive::Int));
        let mut stmts = AmirStmtTable::new();
        stmts.push(AmirStmt::Assign {
            lhs: TempId::from_usize(0),
            rhs: AmirRvalue::BorrowMut(place(0)),
        });
        let block = AmirBasicBlock {
            id: BlockId::from_usize(0),
            statements: DenseRange::new(0, 1),
            params: vec![],
            terminator: AmirTerminator::Return,
        };
        let blocks = vec![block];
        let cfg = compute_cfg_edges(&blocks);
        let func = AmirFunc {
            symbol: crate::SymbolId::new(0, 0),
            return_type: int,
            receiver: None,
            params: vec![],
            locals: vec![local(0, int)],
            temps: vec![temp(0, intern_ty(ArType::RefMut(int)))],
            blocks,
            stmts,
            cfg,
        };
        let facts = analyze_borrow_facts(&func);
        assert!(facts.block_out[0].maybe_exclusive(LocalId::from_usize(0)));
        assert!(!facts.block_out[0].maybe_shared(LocalId::from_usize(0)));
    }

    #[test]
    fn storage_dead_kills_loan() {
        let int = intern_ty(ArType::Primitive(Primitive::Int));
        let mut stmts = AmirStmtTable::new();
        stmts.push(AmirStmt::Assign {
            lhs: TempId::from_usize(0),
            rhs: AmirRvalue::Borrow(place(0)),
        });
        stmts.push(AmirStmt::StorageDead(LocalId::from_usize(0)));
        let block = AmirBasicBlock {
            id: BlockId::from_usize(0),
            statements: DenseRange::new(0, 2),
            params: vec![],
            terminator: AmirTerminator::Return,
        };
        let blocks = vec![block];
        let cfg = compute_cfg_edges(&blocks);
        let func = AmirFunc {
            symbol: crate::SymbolId::new(0, 0),
            return_type: int,
            receiver: None,
            params: vec![],
            locals: vec![local(0, int)],
            temps: vec![temp(0, intern_ty(ArType::Ref(int)))],
            blocks,
            stmts,
            cfg,
        };
        let facts = analyze_borrow_facts(&func);
        assert!(!facts.block_out[0].maybe_borrowed(LocalId::from_usize(0)));
    }
}
