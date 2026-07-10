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
use crate::amir::block::BlockId;
use crate::amir::local::LocalId;
use crate::amir::program::AmirFunc;
use crate::amir::stmt::{AmirStmt, AmirTerminator};
use crate::amir::value::AmirRvalue;
use crate::amir::for_each_rvalue_place;
use crate::diagnostics::{DiagCode, Diagnostic};
use crate::{BitMatrix, BitSet, SymbolTable};

use std::collections::VecDeque;

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
    let mut block_gens = BitMatrix::<BlockId, LocalId>::new(num_blocks, num_locals);

    for block in &func.blocks {
        let bid = block.id;
        for stmt in func.block_stmts(bid) {
            match stmt {
                AmirStmt::Store { lhs, .. } if lhs.projections.is_empty() => {
                    // A store to a plain local (no projections) means that
                    // local is definitely initialized after this statement.
                    block_gens.insert(bid, lhs.local);
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
    let mut block_in = vec![BitSet::<LocalId>::all_set(num_locals); num_blocks];
    let mut block_out = vec![BitSet::<LocalId>::all_set(num_locals); num_blocks];

    let mut worklist = VecDeque::new();
    for block in &func.blocks {
        worklist.push_back(block.id);
    }

    let mut iterations = 0;
    // Theoretical max height of the dataflow lattice: each local can flip from 1 to 0 once per block.
    // So the absolute max number of block state updates is `num_blocks * num_locals`.
    let sanity_limit = num_blocks * num_locals + 1000;

    while let Some(bid) = worklist.pop_front() {
        iterations += 1;
        assert!(
            iterations <= sanity_limit,
            "definite initialization checker failed to converge within theoretical limit: {iterations} > {sanity_limit} ({num_blocks} blocks) — possível bug de monotonicidade no dataflow"
        );

        let bi = bid.as_usize();
        let block = &func.blocks[bi];

        // Compute IN as intersection of all predecessors' OUT
        let new_in = if bid == BlockId::from_usize(0) || func.predecessors(bid).is_empty() {
            BitSet::with_capacity(num_locals)
        } else {
            let mut acc = BitSet::all_set(num_locals);
            for &pred in func.predecessors(bid) {
                acc.intersect_with(&block_out[pred.as_usize()]);
            }
            acc
        };

        // OUT = IN ∪ gen
        let mut new_out = new_in.clone();
        new_out.union_with(&block_gens.row_set(bid));

        // Monotonicity check: intersection dataflow starts at Top (All) and shrinks.
        // Therefore, new_out must be a subset of old_out.
        debug_assert!(
            block_out[bi].is_superset_of(&new_out),
            "Definite init dataflow is not monotonic at block {bi}"
        );

        if new_out != block_out[bi] {
            block_in[bi] = new_in;
            block_out[bi] = new_out;

            // Add successors to worklist
            match &block.terminator {
                AmirTerminator::Return | AmirTerminator::Unreachable => {}
                AmirTerminator::Goto { target, .. } => {
                    worklist.push_back(*target);
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
                    for (_, b, _) in targets {
                        worklist.push_back(*b);
                    }
                    worklist.push_back(otherwise.0);
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
                    current.insert(lhs.local);
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
    current: &BitSet<LocalId>,
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
            if !lhs.projections.is_empty() && !current.contains(lhs.local) {
                emit_uninit_diag(lhs.local, func, symbols, diagnostics);
            }
            // Check if rhs references uninitialized locals via operand
            // (operands are TempId-based, so they are always SSA-valid)
            let _ = rhs;
        }
        AmirStmt::Call { .. } | AmirStmt::Free(_) => {}
        AmirStmt::Destroy(place) => {
            if !current.contains(place.local) {
                emit_uninit_diag(place.local, func, symbols, diagnostics);
            }
        }
        AmirStmt::StorageLive(_) | AmirStmt::StorageDead(_) | AmirStmt::Nop => {}
    }
}

fn check_rvalue_loads(
    rvalue: &AmirRvalue,
    current: &BitSet<LocalId>,
    func: &AmirFunc,
    symbols: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Shared place visitor (RC-ANALYSIS-LOAD): Load / Borrow / BorrowMut.
    for_each_rvalue_place(rvalue, |place| {
        if !current.contains(place.local) {
            emit_uninit_diag(place.local, func, symbols, diagnostics);
        }
    });
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
            symbols.get(s).name.to_string()
        });
    // Prefer use site when recorded; otherwise declaration span (RC-SPAN-ZERO).
    let span = local_info.use_span.unwrap_or(local_info.span);
    diagnostics.push(
        Diagnostic::error(
            DiagCode::O008UseBeforeInit,
            format!("use of possibly uninitialized variable `{name}`"),
            span,
        )
        .with_note(format!(
            "variable `{name}` may not be initialized on all paths"
        )),
    );
}

#[cfg(test)]
mod tests {

    fn intern_ty(ty: crate::types::ArType) -> crate::types::TypeId {
        // Fresh interner per call is OK in unit tests (pre-interns primitives).
        crate::types::TypeInterner::new().intern(ty)
    }
    use super::*;
    use crate::SymbolId;
    use crate::SymbolTable;
    use crate::amir::block::AmirBasicBlock;
    use crate::amir::local::{AmirLocal, AmirTemp, TempId};
    use crate::amir::program::{AmirFunc, extend_block_range};
    use crate::amir::stmt::{AmirStmt, AmirStmtTable, AmirTerminator};
    use crate::amir::value::{AmirConstant, AmirOperand, AmirPlace, AmirRvalue};
    use crate::layout::DenseRange;
    use crate::passes::type_checker::types::ArType;
    use arandu_lexer::Span;
    use smallvec::smallvec;

    fn make_symbol_table() -> SymbolTable {
        SymbolTable::new(0)
    }

    fn make_local(id: usize, sym: Option<SymbolId>) -> AmirLocal {
        AmirLocal {
            id: LocalId::from_usize(id),
            ty: intern_ty(ArType::Primitive(crate::passes::type_checker::types::Primitive::Int)),
            is_memory: false,
            symbol: sym,
            span: Span::new(0, 0, 0),
            use_span: None,
        }
    }

    fn make_temp(id: usize) -> AmirTemp {
        AmirTemp {
            id: TempId::from_usize(id),
            ty: intern_ty(ArType::Primitive(crate::passes::type_checker::types::Primitive::Int)),
            is_copy: true,
            span: Span::new(0, 0, 0),
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
        _successors: &[usize],
        _predecessors: &[usize],
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
            params: Vec::new(),
            terminator,
        }
    }

    fn make_func(
        blocks: Vec<AmirBasicBlock>,
        stmts: AmirStmtTable,
        locals: Vec<AmirLocal>,
        temps: Vec<AmirTemp>,
    ) -> AmirFunc {
        let cfg = crate::cfg::compute_cfg_edges(&blocks);
        AmirFunc {
            symbol: SymbolId::new(0, 0),
            return_type: intern_ty(ArType::Void),
            receiver: None,
            params: vec![],
            locals,
            temps,
            blocks,
            stmts,
            cfg,
        }
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

        let func = make_func(
            blocks,
            stmts,
            vec![make_local(0, None)],
            vec![make_temp(0), make_temp(1)],
        );

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

        let func = make_func(
            blocks,
            stmts,
            vec![make_local(0, None)],
            vec![make_temp(0), make_temp(1)],
        );

        let st = make_symbol_table();
        let diags = check_definite_init(&func, &st);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagCode::O008UseBeforeInit);
    }

    #[test]
    fn test_o008_prefers_use_span_over_decl_span() {
        // Declaration at 0..3; recorded use at 10..15 — O008 must point at use.
        let mut local = make_local(0, None);
        local.span = Span::new(0, 0, 3);
        local.use_span = Some(Span::new(0, 10, 15));

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
        let func = make_func(
            blocks,
            stmts,
            vec![local],
            vec![make_temp(0), make_temp(1)],
        );
        let st = make_symbol_table();
        let diags = check_definite_init(&func, &st);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagCode::O008UseBeforeInit);
        assert_eq!(diags[0].span, Span::new(0, 10, 15));
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
                    true_args: Vec::new(),
                    if_false: BlockId::from_usize(2),
                    false_args: Vec::new(),
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
                AmirTerminator::Goto {
                    target: BlockId::from_usize(3),
                    args: Vec::new(),
                },
                &[3],
                &[0],
                &mut stmts,
            ),
            make_block(
                2,
                vec![],
                AmirTerminator::Goto {
                    target: BlockId::from_usize(3),
                    args: Vec::new(),
                },
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

        let func = make_func(
            blocks,
            stmts,
            vec![make_local(0, None)],
            vec![make_temp(0), make_temp(1)],
        );

        let st = make_symbol_table();
        let diags = check_definite_init(&func, &st);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagCode::O008UseBeforeInit);
    }

    #[test]
    fn test_no_error_with_loop_init() {
        // bb0: Store local0 = const; Goto bb1
        // bb1: Load local0; Branch(cond, bb1, bb2)
        // bb2: return
        let mut stmts = AmirStmtTable::new();
        let blocks = vec![
            make_block(
                0,
                vec![AmirStmt::Store {
                    lhs: place(0),
                    rhs: AmirOperand::Constant(AmirConstant::Bool(true)),
                }],
                AmirTerminator::Goto {
                    target: BlockId::from_usize(1),
                    args: Vec::new(),
                },
                &[1],
                &[],
                &mut stmts,
            ),
            make_block(
                1,
                vec![AmirStmt::Assign {
                    lhs: TempId::from_usize(1),
                    rhs: AmirRvalue::Load(place(0)),
                }],
                AmirTerminator::Branch {
                    condition: AmirOperand::Copy(TempId::from_usize(1)),
                    if_true: BlockId::from_usize(1),
                    true_args: Vec::new(),
                    if_false: BlockId::from_usize(2),
                    false_args: Vec::new(),
                },
                &[1, 2],
                &[0],
                &mut stmts,
            ),
            make_block(2, vec![], AmirTerminator::Return, &[], &[1], &mut stmts),
        ];
        let func = make_func(
            blocks,
            stmts,
            vec![make_local(0, None)],
            vec![make_temp(0), make_temp(1)],
        );
        let st = make_symbol_table();
        let diags = check_definite_init(&func, &st);
        assert!(diags.is_empty(), "expected no errors, got: {:?}", diags);
    }

    #[test]
    fn test_no_error_with_empty_func() {
        let stmts = AmirStmtTable::new();
        let blocks = vec![make_block(
            0,
            vec![],
            AmirTerminator::Return,
            &[],
            &[],
            &mut stmts.clone(),
        )];
        let func = make_func(blocks, stmts, vec![], vec![]);
        let st = make_symbol_table();
        let diags = check_definite_init(&func, &st);
        assert!(diags.is_empty());
    }

    #[test]
    fn test_error_no_init_at_all() {
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
        let func = make_func(
            blocks,
            stmts,
            vec![make_local(0, None)],
            vec![make_temp(0), make_temp(1)],
        );
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
                    true_args: Vec::new(),
                    if_false: BlockId::from_usize(2),
                    false_args: Vec::new(),
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
                AmirTerminator::Goto {
                    target: BlockId::from_usize(3),
                    args: Vec::new(),
                },
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
                AmirTerminator::Goto {
                    target: BlockId::from_usize(3),
                    args: Vec::new(),
                },
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

        let func = make_func(
            blocks,
            stmts,
            vec![make_local(0, None)],
            vec![make_temp(0), make_temp(1)],
        );

        let st = make_symbol_table();
        let diags = check_definite_init(&func, &st);
        assert!(diags.is_empty(), "expected no errors, got: {:?}", diags);
    }
}
