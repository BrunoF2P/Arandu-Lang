//! Definite Initialization Analysis (O008)
//!
//! A forward dataflow analysis over the AMIR CFG that tracks whether each
//! `LocalId` stack slot has been initialized at every program point. Reports
//! an error (`O008UseBeforeInit`) if a `Load` reads from a local that is
//! *possibly* uninitialized along some control-flow path.
//!
//! ## Lattice
//!
//! Each local is in one of two states:
//! - `Initialized` — the local has been written via `Store` on **all** paths.
//! - `MaybeUninitialized` — at least one path exists that has not written to it.
//!
//! The join operation for merge points is `intersect`: a local is initialized
//! at a merge only if it is initialized in *all* predecessors.
//!
//! ## Algorithm
//!
//! We use the simple iterative worklist algorithm over the RPO of the CFG,
//! propagating gen/kill sets per block until a fixpoint is reached. This is
//! the same class of algorithm as the LLVM `MaybeUninitializedPlaces` analysis.

#![allow(
    clippy::collapsible_match,
    clippy::single_match,
    clippy::collapsible_if
)]

use crate::SymbolTable;
use crate::amir::block::BlockId;
use crate::amir::local::LocalId;
use crate::amir::program::AmirFunc;
use crate::amir::stmt::{AmirStmt, AmirTerminator};
use crate::amir::value::AmirRvalue;
use crate::diagnostics::{DiagCode, Diagnostic};

use arandu_lexer::Span;

use std::collections::VecDeque;

/// A bitset tracking which locals are definitely initialized.
#[derive(Debug, Clone, PartialEq, Eq)]
struct InitBits {
    words: Vec<u64>,
    len: usize,
}

impl InitBits {
    fn new(n: usize) -> Self {
        Self {
            words: vec![0; n.div_ceil(64)],
            len: n,
        }
    }

    fn all_init(n: usize) -> Self {
        let mut bits = Self {
            words: vec![u64::MAX; n.div_ceil(64)],
            len: n,
        };
        bits.mask_tail();
        bits
    }

    fn mask_tail(&mut self) {
        let rem = self.len % 64;
        if rem == 0 {
            return;
        }
        if let Some(last) = self.words.last_mut() {
            *last &= (1u64 << rem) - 1;
        }
    }

    fn set(&mut self, id: LocalId) {
        let idx = id.as_usize();
        if idx < self.len {
            self.words[idx / 64] |= 1u64 << (idx % 64);
        }
    }

    fn get(&self, id: LocalId) -> bool {
        let idx = id.as_usize();
        idx < self.len && (self.words[idx / 64] & (1u64 << (idx % 64))) != 0
    }

    /// Intersect: a local is initialized only if initialized in *both* sets.
    fn intersect_with(&mut self, other: &Self) {
        for (a, b) in self.words.iter_mut().zip(other.words.iter()) {
            *a &= *b;
        }
        self.mask_tail();
    }
}

/// Run definite-initialization analysis over a single AMIR function.
///
/// Returns a list of `O008` diagnostics for any load from a possibly
/// uninitialized local.
pub fn check_definite_init(func: &AmirFunc, symbols: &SymbolTable) -> Vec<Diagnostic> {
    let num_locals = func.locals.len();
    let num_blocks = func.blocks.len();

    if num_locals == 0 || num_blocks == 0 {
        return Vec::new();
    }

    // ------------------------------------------------------------------
    // 1. Compute per-block gen sets (locals that are definitely stored)
    //    and collect load locations for later error reporting.
    // ------------------------------------------------------------------
    let mut block_gens: Vec<InitBits> = vec![InitBits::new(num_locals); num_blocks];

    for block in &func.blocks {
        let bi = block.id.as_usize();
        for stmt in func.block_stmts(block.id) {
            match stmt {
                AmirStmt::Store { lhs, .. } if lhs.projections.is_empty() => {
                    // A store to a plain local (no projections) means that
                    // local is definitely initialized after this statement.
                    block_gens[bi].set(lhs.local);
                }
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------------
    // 2. Forward dataflow: propagate init facts across the CFG.
    //
    //    block_in[b] = intersect(block_out[p] for p in predecessors(b))
    //    block_out[b] = block_in[b] ∪ gen[b]
    //
    //    Entry block starts with no locals initialized (unless they
    //    appear in gen).
    // ------------------------------------------------------------------
    let mut block_in: Vec<InitBits> = vec![InitBits::all_init(num_locals); num_blocks];
    let mut block_out: Vec<InitBits> = vec![InitBits::all_init(num_locals); num_blocks];

    let mut worklist = VecDeque::new();
    for block in &func.blocks {
        worklist.push_back(block.id);
    }

    let mut iterations = 0;
    let max_iterations = num_blocks * num_blocks + 10;

    while let Some(bid) = worklist.pop_front() {
        iterations += 1;
        if iterations > max_iterations {
            break; // Safety: prevent infinite loops on malformed CFGs
        }

        let bi = bid.as_usize();
        let block = &func.blocks[bi];

        // Compute IN as intersection of all predecessors' OUT
        let new_in = if bid == BlockId::from_usize(0) || block.predecessors.is_empty() {
            InitBits::new(num_locals)
        } else {
            let mut acc = InitBits::all_init(num_locals);
            for &pred in &block.predecessors {
                acc.intersect_with(&block_out[pred.as_usize()]);
            }
            acc
        };

        // OUT = IN ∪ gen
        let mut new_out = new_in.clone();
        for i in 0..num_locals {
            let lid = LocalId::from_usize(i);
            if block_gens[bi].get(lid) {
                new_out.set(lid);
            }
        }

        if new_out != block_out[bi] {
            block_in[bi] = new_in;
            block_out[bi] = new_out;

            // Add successors to worklist
            match &block.terminator {
                AmirTerminator::Return | AmirTerminator::Unreachable => {}
                AmirTerminator::Goto(b) => {
                    worklist.push_back(*b);
                }
                AmirTerminator::Branch {
                    if_true, if_false, ..
                } => {
                    worklist.push_back(*if_true);
                    worklist.push_back(*if_false);
                }
                AmirTerminator::SwitchInt {
                    targets, otherwise, ..
                } => {
                    for (_, b) in targets {
                        worklist.push_back(*b);
                    }
                    worklist.push_back(*otherwise);
                }
            }
        } else {
            block_in[bi] = new_in;
        }
    }

    // ------------------------------------------------------------------
    // 3. Scan each block: at each statement, track the running init state
    //    and report any loads from uninitialized locals.
    // ------------------------------------------------------------------
    let mut diagnostics = Vec::new();

    for block in &func.blocks {
        let bi = block.id.as_usize();
        let mut current = block_in[bi].clone();

        for stmt in func.block_stmts(block.id) {
            // Check loads (reads) before recording stores (writes)
            check_stmt_loads(stmt, &current, func, symbols, &mut diagnostics);

            // Apply gen: stores update the running state
            match stmt {
                AmirStmt::Store { lhs, .. } if lhs.projections.is_empty() => {
                    current.set(lhs.local);
                }
                _ => {}
            }
        }
    }

    diagnostics
}

/// Check whether any `Load` in a statement reads from an uninitialized local.
fn check_stmt_loads(
    stmt: &AmirStmt,
    current: &InitBits,
    func: &AmirFunc,
    symbols: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match stmt {
        AmirStmt::Assign { rhs, .. } => {
            check_rvalue_loads(rhs, current, func, symbols, diagnostics);
        }
        AmirStmt::Store { rhs, lhs, .. } => {
            // If storing to a projection (e.g. x.field), the base must be initialized
            if !lhs.projections.is_empty() && !current.get(lhs.local) {
                emit_uninit_diag(lhs.local, func, symbols, diagnostics);
            }
            // Check if rhs references uninitialized locals via operand
            // (operands are TempId-based, so they are always SSA-valid)
            let _ = rhs;
        }
        AmirStmt::Call { .. } | AmirStmt::Free(_) => {}
        AmirStmt::Destroy(place) => {
            if !current.get(place.local) {
                emit_uninit_diag(place.local, func, symbols, diagnostics);
            }
        }
        AmirStmt::StorageLive(_) | AmirStmt::StorageDead(_) => {}
    }
}

fn check_rvalue_loads(
    rvalue: &AmirRvalue,
    current: &InitBits,
    func: &AmirFunc,
    symbols: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match rvalue {
        AmirRvalue::Load(place) | AmirRvalue::Borrow(place) | AmirRvalue::BorrowMut(place)
            if !current.get(place.local) =>
        {
            emit_uninit_diag(place.local, func, symbols, diagnostics);
        }
        _ => {}
    }
}

fn emit_uninit_diag(
    local: LocalId,
    func: &AmirFunc,
    symbols: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let local_info = &func.locals[local.as_usize()];
    let name = local_info
        .symbol
        .map_or("<compiler local>".to_string(), |s| {
            symbols.get(s).name.clone()
        });
    diagnostics.push(
        Diagnostic::error(
            DiagCode::O008UseBeforeInit,
            format!("use of possibly uninitialized variable `{name}`"),
            Span::new(0, 0, 0, 0, 0, 0),
        )
        .with_note(format!(
            "variable `{name}` may not be initialized on all paths"
        )),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SymbolId;
    use crate::amir::block::AmirBasicBlock;
    use crate::amir::local::{AmirLocal, AmirTemp, TempId};
    use crate::amir::program::{AmirFunc, extend_block_range};
    use crate::amir::stmt::{AmirStmt, AmirStmtTable, AmirTerminator};
    use crate::amir::value::{AmirConstant, AmirOperand, AmirPlace, AmirRvalue};
    use crate::layout::DenseRange;
    use crate::passes::type_checker::types::ArType;
    use crate::symbol_table::SymbolTable;
    use smallvec::smallvec;

    fn make_symbol_table() -> SymbolTable {
        SymbolTable::new()
    }

    fn make_local(id: usize, sym: Option<SymbolId>) -> AmirLocal {
        AmirLocal {
            id: LocalId::from_usize(id),
            ty: ArType::Primitive(crate::passes::type_checker::types::Primitive::Int),
            symbol: sym,
            span: Span::new(0, 0, 0, 0, 0, 0),
            use_span: None,
        }
    }

    fn make_temp(id: usize) -> AmirTemp {
        AmirTemp {
            id: TempId::from_usize(id),
            ty: ArType::Primitive(crate::passes::type_checker::types::Primitive::Int),
            span: Span::new(0, 0, 0, 0, 0, 0),
        }
    }

    fn place(local: usize) -> AmirPlace {
        AmirPlace {
            local: LocalId::from_usize(local),
            projections: smallvec![],
        }
    }

    fn make_block(
        id: usize,
        statements: Vec<AmirStmt>,
        terminator: AmirTerminator,
        successors: &[usize],
        predecessors: &[usize],
        stmts: &mut AmirStmtTable,
    ) -> AmirBasicBlock {
        let mut range = DenseRange::empty();
        for stmt in statements {
            let instr = stmts.push(stmt);
            extend_block_range(&mut range, instr);
        }
        AmirBasicBlock {
            id: BlockId::from_usize(id),
            statements: range,
            terminator,
            successors: successors.iter().map(|&x| BlockId::from_usize(x)).collect(),
            predecessors: predecessors
                .iter()
                .map(|&x| BlockId::from_usize(x))
                .collect(),
        }
    }

    #[test]
    fn test_init_bits_tracks_locals_across_word_boundaries() {
        let mut bits = InitBits::new(130);
        bits.set(LocalId::from_usize(0));
        bits.set(LocalId::from_usize(64));
        bits.set(LocalId::from_usize(129));

        assert!(bits.get(LocalId::from_usize(0)));
        assert!(bits.get(LocalId::from_usize(64)));
        assert!(bits.get(LocalId::from_usize(129)));
        assert!(!bits.get(LocalId::from_usize(128)));
        assert!(!bits.get(LocalId::from_usize(130)));

        let mut all = InitBits::all_init(130);
        all.intersect_with(&bits);
        assert!(all.get(LocalId::from_usize(0)));
        assert!(all.get(LocalId::from_usize(64)));
        assert!(all.get(LocalId::from_usize(129)));
        assert!(!all.get(LocalId::from_usize(128)));
        assert!(!all.get(LocalId::from_usize(130)));
    }

    #[test]
    fn test_no_error_when_stored_before_load() {
        // bb0: Store local0 = const; Assign t1 = Load(local0); return
        let mut stmts = AmirStmtTable::new();
        let blocks = vec![make_block(
            0,
            vec![
                AmirStmt::Store {
                    lhs: place(0),
                    rhs: AmirOperand::Constant(AmirConstant::Bool(true)),
                },
                AmirStmt::Assign {
                    lhs: TempId::from_usize(1),
                    rhs: AmirRvalue::Load(place(0)),
                },
            ],
            AmirTerminator::Return,
            &[],
            &[],
            &mut stmts,
        )];

        let func = AmirFunc {
            symbol: SymbolId(0),
            return_type: ArType::Void,
            receiver: None,
            params: vec![],
            locals: vec![make_local(0, None)],
            temps: vec![make_temp(0), make_temp(1)],
            blocks,
            stmts,
        };

        let st = make_symbol_table();
        let diags = check_definite_init(&func, &st);
        assert!(diags.is_empty(), "expected no errors, got: {:?}", diags);
    }

    #[test]
    fn test_error_when_loaded_without_store() {
        // bb0: Assign t1 = Load(local0); return
        let mut stmts = AmirStmtTable::new();
        let blocks = vec![make_block(
            0,
            vec![AmirStmt::Assign {
                lhs: TempId::from_usize(1),
                rhs: AmirRvalue::Load(place(0)),
            }],
            AmirTerminator::Return,
            &[],
            &[],
            &mut stmts,
        )];

        let func = AmirFunc {
            symbol: SymbolId(0),
            return_type: ArType::Void,
            receiver: None,
            params: vec![],
            locals: vec![make_local(0, None)],
            temps: vec![make_temp(0), make_temp(1)],
            blocks,
            stmts,
        };

        let st = make_symbol_table();
        let diags = check_definite_init(&func, &st);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagCode::O008UseBeforeInit);
    }

    #[test]
    fn test_error_on_conditional_init() {
        // bb0: Branch -> bb1, bb2
        // bb1: Store local0; Goto bb3
        // bb2: Goto bb3
        // bb3: Load local0  <-- ERROR: only initialized on one branch
        let mut stmts = AmirStmtTable::new();
        let blocks = vec![
            make_block(
                0,
                vec![],
                AmirTerminator::Branch {
                    condition: AmirOperand::Constant(AmirConstant::Bool(true)),
                    if_true: BlockId::from_usize(1),
                    if_false: BlockId::from_usize(2),
                },
                &[1, 2],
                &[],
                &mut stmts,
            ),
            make_block(
                1,
                vec![AmirStmt::Store {
                    lhs: place(0),
                    rhs: AmirOperand::Constant(AmirConstant::Bool(true)),
                }],
                AmirTerminator::Goto(BlockId::from_usize(3)),
                &[3],
                &[0],
                &mut stmts,
            ),
            make_block(
                2,
                vec![],
                AmirTerminator::Goto(BlockId::from_usize(3)),
                &[3],
                &[0],
                &mut stmts,
            ),
            make_block(
                3,
                vec![AmirStmt::Assign {
                    lhs: TempId::from_usize(1),
                    rhs: AmirRvalue::Load(place(0)),
                }],
                AmirTerminator::Return,
                &[],
                &[1, 2],
                &mut stmts,
            ),
        ];

        let func = AmirFunc {
            symbol: SymbolId(0),
            return_type: ArType::Void,
            receiver: None,
            params: vec![],
            locals: vec![make_local(0, None)],
            temps: vec![make_temp(0), make_temp(1)],
            blocks,
            stmts,
        };

        let st = make_symbol_table();
        let diags = check_definite_init(&func, &st);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagCode::O008UseBeforeInit);
    }

    #[test]
    fn test_no_error_on_both_branches_init() {
        // bb0: Branch -> bb1, bb2
        // bb1: Store local0; Goto bb3
        // bb2: Store local0; Goto bb3
        // bb3: Load local0  <-- OK: initialized on both branches
        let mut stmts = AmirStmtTable::new();
        let blocks = vec![
            make_block(
                0,
                vec![],
                AmirTerminator::Branch {
                    condition: AmirOperand::Constant(AmirConstant::Bool(true)),
                    if_true: BlockId::from_usize(1),
                    if_false: BlockId::from_usize(2),
                },
                &[1, 2],
                &[],
                &mut stmts,
            ),
            make_block(
                1,
                vec![AmirStmt::Store {
                    lhs: place(0),
                    rhs: AmirOperand::Constant(AmirConstant::Bool(true)),
                }],
                AmirTerminator::Goto(BlockId::from_usize(3)),
                &[3],
                &[0],
                &mut stmts,
            ),
            make_block(
                2,
                vec![AmirStmt::Store {
                    lhs: place(0),
                    rhs: AmirOperand::Constant(AmirConstant::Bool(true)),
                }],
                AmirTerminator::Goto(BlockId::from_usize(3)),
                &[3],
                &[0],
                &mut stmts,
            ),
            make_block(
                3,
                vec![AmirStmt::Assign {
                    lhs: TempId::from_usize(1),
                    rhs: AmirRvalue::Load(place(0)),
                }],
                AmirTerminator::Return,
                &[],
                &[1, 2],
                &mut stmts,
            ),
        ];

        let func = AmirFunc {
            symbol: SymbolId(0),
            return_type: ArType::Void,
            receiver: None,
            params: vec![],
            locals: vec![make_local(0, None)],
            temps: vec![make_temp(0), make_temp(1)],
            blocks,
            stmts,
        };

        let st = make_symbol_table();
        let diags = check_definite_init(&func, &st);
        assert!(diags.is_empty(), "expected no errors, got: {:?}", diags);
    }
}
