//! Intraprocedural AMIR move checking (M1).
//!
//! This pass tracks whole-local ownership state across the AMIR CFG. It is
//! intentionally conservative for v0.1: projections are treated as reads of the
//! base local and moves are recovered from `Load(place)` followed by consuming
//! `Move(temp)` operands.

#![allow(clippy::collapsible_if)]

use crate::amir::{
    AmirFunc, AmirOperand, AmirPlace, AmirRvalue, AmirStmt, AmirTerminator, LocalId, TempId,
    for_each_rvalue_operand, for_each_rvalue_place,
};
use crate::diagnostics::{DiagCode, Diagnostic};
use crate::{BitSet, SymbolTable};
use arandu_lexer::Span;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalMoveState {
    Available,
    Moved,
    MaybeMoved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MoveState {
    moved: BitSet<LocalId>,
    maybe_moved: BitSet<LocalId>,
}

impl MoveState {
    fn new(num_locals: usize) -> Self {
        Self {
            moved: BitSet::with_capacity(num_locals),
            maybe_moved: BitSet::with_capacity(num_locals),
        }
    }

    #[tracing::instrument(level = "trace", target = "arandu_mir::move_checker", skip_all)]
    fn join_predecessors(preds: impl Iterator<Item = Self>, num_locals: usize) -> Self {
        let mut preds = preds.peekable();
        let Some(mut acc) = preds.next() else {
            return Self::new(num_locals);
        };

        for pred in preds {
            let mut new_m = acc.moved.clone();
            new_m.intersect_with(&pred.moved);

            let mut union_m = acc.moved.clone();
            union_m.union_with(&pred.moved);
            union_m.difference_with(&new_m);

            acc.maybe_moved.union_with(&pred.maybe_moved);
            acc.maybe_moved.union_with(&union_m);

            acc.moved = new_m;
        }
        acc
    }

    fn get(&self, local: LocalId) -> LocalMoveState {
        if self.moved.contains(local) {
            LocalMoveState::Moved
        } else if self.maybe_moved.contains(local) {
            LocalMoveState::MaybeMoved
        } else {
            LocalMoveState::Available
        }
    }

    fn set(&mut self, local: LocalId, state: LocalMoveState) {
        match state {
            LocalMoveState::Available => {
                self.moved.remove(local);
                self.maybe_moved.remove(local);
            }
            LocalMoveState::Moved => {
                self.moved.insert(local);
                self.maybe_moved.remove(local);
            }
            LocalMoveState::MaybeMoved => {
                self.moved.remove(local);
                self.maybe_moved.insert(local);
            }
        }
    }
    fn is_monotonic_from(&self, old: &Self) -> bool {
        if !self.moved.is_superset_of(&old.moved) {
            return false;
        }
        for id in old.maybe_moved.iter() {
            if !self.maybe_moved.contains(id) && !self.moved.contains(id) {
                return false;
            }
        }
        true
    }
}

pub fn check_moves(func: &AmirFunc, symbols: &SymbolTable) -> Vec<Diagnostic> {
    let num_locals = func.locals.len();
    let num_blocks = func.blocks.len();

    if num_locals == 0 || num_blocks == 0 {
        return Vec::new();
    }

    let temp_origins = temp_origins(func);
    let mut block_in = vec![MoveState::new(num_locals); num_blocks];
    let mut block_out = vec![MoveState::new(num_locals); num_blocks];
    let mut worklist = VecDeque::new();

    for block in &func.blocks {
        worklist.push_back(block.id);
    }

    let mut iterations = 0;
    // Theoretical max height of the dataflow lattice: each local can flip from Available -> MaybeMoved -> Moved.
    // So the absolute max number of block state updates is `num_blocks * num_locals * 2`.
    let sanity_limit = num_blocks * num_locals * 2 + 1000;

    while let Some(bid) = worklist.pop_front() {
        iterations += 1;
        assert!(
            iterations <= sanity_limit,
            "move checker failed to converge within theoretical limit: {iterations} > {sanity_limit} ({num_blocks} blocks) — possível bug de monotonicidade no dataflow"
        );

        let bi = bid.as_usize();
        let block = &func.blocks[bi];
        let new_in = MoveState::join_predecessors(
            func.predecessors(bid)
                .iter()
                .map(|pred| block_out[pred.as_usize()].clone()),
            num_locals,
        );
        let mut new_out = new_in.clone();
        apply_block(block.id, func, &temp_origins, &mut new_out, None);

        // Monotonicity check: the dataflow lattice grows (Available -> MaybeMoved -> Moved).
        debug_assert!(
            new_out.is_monotonic_from(&block_out[bi]),
            "Move checker dataflow is not monotonic at block {bi}"
        );

        if new_in != block_in[bi] || new_out != block_out[bi] {
            block_in[bi] = new_in;
            block_out[bi] = new_out;
            for succ in successors(&block.terminator) {
                worklist.push_back(succ);
            }
        }
    }

    let mut diagnostics = Vec::new();
    for block in &func.blocks {
        let mut state = block_in[block.id.as_usize()].clone();
        apply_block(
            block.id,
            func,
            &temp_origins,
            &mut state,
            Some((symbols, &mut diagnostics)),
        );
    }

    diagnostics
}

fn temp_origins(func: &AmirFunc) -> Vec<Option<LocalId>> {
    let mut origins = vec![None; func.temps.len()];
    for (i, &param_temp) in func.params.iter().enumerate() {
        origins[param_temp.as_usize()] = Some(LocalId::from_usize(i));
    }
    for block in &func.blocks {
        for param in &block.params {
            origins[param.id.as_usize()] = Some(param.local);
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        for block in &func.blocks {
            for stmt in func.block_stmts(block.id) {
                if let AmirStmt::Assign { lhs, rhs } = stmt {
                    let mut found_origin = None;
                    match rhs {
                        AmirRvalue::Load(place) if place.projections.is_empty() => {
                            found_origin = Some(place.local);
                        }
                        AmirRvalue::Use(AmirOperand::Copy(t) | AmirOperand::Move(t)) => {
                            found_origin = origins[t.as_usize()];
                        }
                        _ => {}
                    }
                    if let Some(loc) = found_origin {
                        if origins[lhs.as_usize()].is_none() {
                            origins[lhs.as_usize()] = Some(loc);
                            changed = true;
                        }
                    }
                }
            }
        }
    }
    origins
}

fn apply_block(
    block: crate::amir::BlockId,
    func: &AmirFunc,
    temp_origins: &[Option<LocalId>],
    state: &mut MoveState,
    mut diagnostics: Option<(&SymbolTable, &mut Vec<Diagnostic>)>,
) {
    for stmt in func.block_stmts(block) {
        match stmt {
            AmirStmt::Assign { rhs, .. } => {
                check_rvalue_reads(rhs, func, state, &mut diagnostics);
                consume_rvalue(rhs, func, temp_origins, state, &mut diagnostics);
            }
            AmirStmt::Store { lhs, rhs } => {
                if !lhs.projections.is_empty() {
                    check_place_read(lhs, func, state, &mut diagnostics);
                }
                consume_operand(rhs, func, temp_origins, state, &mut diagnostics, false);
                if lhs.projections.is_empty() {
                    state.set(lhs.local, LocalMoveState::Available);
                }
            }
            AmirStmt::Call { callee, args, .. } => {
                consume_operand(callee, func, temp_origins, state, &mut diagnostics, false);
                for arg in args {
                    consume_operand(arg, func, temp_origins, state, &mut diagnostics, false);
                }
            }
            AmirStmt::Free(op) => {
                consume_operand(op, func, temp_origins, state, &mut diagnostics, true);
            }
            AmirStmt::Destroy(place) => {
                check_consume_place(place, func, state, &mut diagnostics, true);
                state.set(place.local, LocalMoveState::Moved);
            }
            AmirStmt::StorageLive(_) | AmirStmt::StorageDead(_) | AmirStmt::Nop => {}
        }
    }

    match &func.block(block).terminator {
        AmirTerminator::Branch {
            condition,
            true_args,
            false_args,
            ..
        } => {
            check_operand_read(condition, func, temp_origins, state, &mut diagnostics);
            for arg in true_args {
                consume_operand(arg, func, temp_origins, state, &mut diagnostics, false);
            }
            for arg in false_args {
                consume_operand(arg, func, temp_origins, state, &mut diagnostics, false);
            }
        }
        AmirTerminator::SwitchInt {
            discriminant,
            targets,
            otherwise,
            ..
        } => {
            check_operand_read(discriminant, func, temp_origins, state, &mut diagnostics);
            for (_, _, args) in targets {
                for arg in args {
                    consume_operand(arg, func, temp_origins, state, &mut diagnostics, false);
                }
            }
            for arg in &otherwise.1 {
                consume_operand(arg, func, temp_origins, state, &mut diagnostics, false);
            }
        }
        AmirTerminator::Goto { args, .. } => {
            for arg in args {
                consume_operand(arg, func, temp_origins, state, &mut diagnostics, false);
            }
        }
        AmirTerminator::Return | AmirTerminator::Unreachable => {}
    }
}

fn check_rvalue_reads(
    rvalue: &AmirRvalue,
    func: &AmirFunc,
    state: &MoveState,
    diagnostics: &mut Option<(&SymbolTable, &mut Vec<Diagnostic>)>,
) {
    for_each_rvalue_place(rvalue, |place| {
        check_place_read(place, func, state, diagnostics);
    });
}

fn consume_rvalue(
    rvalue: &AmirRvalue,
    func: &AmirFunc,
    temp_origins: &[Option<LocalId>],
    state: &mut MoveState,
    diagnostics: &mut Option<(&SymbolTable, &mut Vec<Diagnostic>)>,
) {
    // Shared visitor covers all operand-bearing rvalues (RC-ANALYSIS-LOAD).
    // Load/Borrow/BorrowMut only contribute Index projection operands, not the base place.
    for_each_rvalue_operand(rvalue, |op| {
        consume_operand(op, func, temp_origins, state, diagnostics, false);
    });
}

fn check_operand_read(
    op: &AmirOperand,
    func: &AmirFunc,
    temp_origins: &[Option<LocalId>],
    state: &MoveState,
    diagnostics: &mut Option<(&SymbolTable, &mut Vec<Diagnostic>)>,
) {
    let (AmirOperand::Copy(temp) | AmirOperand::Move(temp)) = op else {
        return;
    };
    if let Some(local) = origin_for(*temp, temp_origins) {
        check_local_read(local, func, state, diagnostics);
    }
}

fn consume_operand(
    op: &AmirOperand,
    func: &AmirFunc,
    temp_origins: &[Option<LocalId>],
    state: &mut MoveState,
    diagnostics: &mut Option<(&SymbolTable, &mut Vec<Diagnostic>)>,
    double_free: bool,
) {
    let AmirOperand::Move(temp) = op else {
        check_operand_read(op, func, temp_origins, state, diagnostics);
        return;
    };

    if func.temps[temp.as_usize()].is_copy {
        return;
    }
    let Some(local) = origin_for(*temp, temp_origins) else {
        return;
    };
    check_consume_local(local, func, state, diagnostics, double_free);
    state.set(local, LocalMoveState::Moved);
}

fn check_place_read(
    place: &AmirPlace,
    func: &AmirFunc,
    state: &MoveState,
    diagnostics: &mut Option<(&SymbolTable, &mut Vec<Diagnostic>)>,
) {
    check_local_read(place.local, func, state, diagnostics);
}

fn check_consume_place(
    place: &AmirPlace,
    func: &AmirFunc,
    state: &MoveState,
    diagnostics: &mut Option<(&SymbolTable, &mut Vec<Diagnostic>)>,
    double_free: bool,
) {
    check_consume_local(place.local, func, state, diagnostics, double_free);
}

fn check_local_read(
    local: LocalId,
    func: &AmirFunc,
    state: &MoveState,
    diagnostics: &mut Option<(&SymbolTable, &mut Vec<Diagnostic>)>,
) {
    let Some((symbols, diagnostics)) = diagnostics.as_mut() else {
        return;
    };
    match state.get(local) {
        LocalMoveState::Available => {}
        LocalMoveState::Moved => diagnostics.push(move_diag(
            DiagCode::O001UseAfterMove,
            local,
            func,
            symbols,
            "use of moved value",
            "value was moved before this use",
        )),
        LocalMoveState::MaybeMoved => diagnostics.push(move_diag(
            DiagCode::O007InconsistentMoveBetweenBranches,
            local,
            func,
            symbols,
            "value may have been moved on some control-flow paths",
            "ensure all branches leave the value in a consistent ownership state",
        )),
    }
}

fn check_consume_local(
    local: LocalId,
    func: &AmirFunc,
    state: &MoveState,
    diagnostics: &mut Option<(&SymbolTable, &mut Vec<Diagnostic>)>,
    double_free: bool,
) {
    let Some((symbols, diagnostics)) = diagnostics.as_mut() else {
        return;
    };
    match state.get(local) {
        LocalMoveState::Available => {}
        LocalMoveState::Moved if double_free => diagnostics.push(move_diag(
            DiagCode::O005DoubleFree,
            local,
            func,
            symbols,
            "double free/drop of moved value",
            "value was already consumed on this path",
        )),
        LocalMoveState::Moved => diagnostics.push(move_diag(
            DiagCode::O001UseAfterMove,
            local,
            func,
            symbols,
            "use of moved value",
            "value was already consumed on this path",
        )),
        LocalMoveState::MaybeMoved => diagnostics.push(move_diag(
            DiagCode::O007InconsistentMoveBetweenBranches,
            local,
            func,
            symbols,
            "value may have been moved on some control-flow paths",
            "ensure all branches leave the value in a consistent ownership state",
        )),
    }
}

fn origin_for(temp: TempId, temp_origins: &[Option<LocalId>]) -> Option<LocalId> {
    temp_origins.get(temp.as_usize()).copied().flatten()
}

#[cold]
#[inline(never)]
fn move_diag(
    code: DiagCode,
    local: LocalId,
    func: &AmirFunc,
    symbols: &SymbolTable,
    prefix: &str,
    note: &str,
) -> Diagnostic {
    let name = local_name(local, func, symbols);
    let span = local_diag_span(local, func, symbols);
    Diagnostic::error(code, format!("{prefix} `{name}`"), span).with_note(note)
}

/// Prefer use site → declaration → symbol span → zero (S-SPAN-THREAD).
fn local_diag_span(local: LocalId, func: &AmirFunc, symbols: &SymbolTable) -> Span {
    let Some(l) = func.locals.get(local.as_usize()) else {
        return Span::new(0, 0, 0);
    };
    if let Some(u) = l.use_span {
        if u.start != u.end {
            return u;
        }
    }
    if l.span.start != l.span.end {
        return l.span;
    }
    if let Some(sym) = l.symbol {
        let s = symbols.get(sym).span;
        if s.start != s.end {
            return s;
        }
    }
    Span::new(0, 0, 0)
}

fn local_name(local: LocalId, func: &AmirFunc, symbols: &SymbolTable) -> String {
    func.locals
        .get(local.as_usize())
        .and_then(|local| local.symbol)
        .map_or_else(
            || format!("s{}", local.as_usize()),
            |symbol| symbols.get(symbol).name.to_string(),
        )
}

fn successors(term: &AmirTerminator) -> Vec<crate::amir::BlockId> {
    match term {
        AmirTerminator::Return | AmirTerminator::Unreachable => Vec::new(),
        AmirTerminator::Goto { target, .. } => vec![*target],
        AmirTerminator::Branch {
            if_true, if_false, ..
        } => vec![*if_true, *if_false],
        AmirTerminator::SwitchInt {
            targets, otherwise, ..
        } => {
            let mut out: Vec<_> = targets.iter().map(|(_, block, _)| *block).collect();
            out.push(otherwise.0);
            out
        }
    }
}

#[cfg(test)]
mod tests {

    fn intern_ty(ty: crate::types::ArType) -> crate::types::TypeId {
        // Fresh interner per call is OK in unit tests (pre-interns primitives).
        crate::types::TypeInterner::new().intern(ty)
    }
    use super::*;
    use crate::amir::program::extend_block_range;
    use crate::amir::{AmirBasicBlock, AmirLocal, AmirStmtTable, AmirTemp, BlockId};
    use crate::layout::DenseRange;
    use crate::passes::type_checker::types::{ArType, Primitive};
    use smallvec::smallvec;

    fn non_copy_ty() -> ArType {
        ArType::Named(crate::SymbolId::new(0, 0), Vec::new())
    }

    fn int_ty() -> ArType {
        ArType::Primitive(Primitive::Int)
    }

    fn local(id: usize, ty: ArType) -> AmirLocal {
        let is_memory = !ty.is_copy_v01() && !matches!(ty, ArType::Primitive(_));
        AmirLocal {
            id: LocalId::from_usize(id),
            ty: intern_ty(ty),
            is_memory,
            symbol: None,
            span: Span::new(0, 0, 0),
            use_span: None,
        }
    }

    fn temp(id: usize, ty: ArType) -> AmirTemp {
        let is_copy = ty.is_copy_v01();
        AmirTemp {
            id: TempId::from_usize(id),
            ty: intern_ty(ty),
            is_copy,
            span: Span::new(0, 0, 0),
        }
    }

    fn place(local: usize) -> AmirPlace {
        AmirPlace {
            local: LocalId::from_usize(local),
            projections: smallvec![],
        }
    }

    fn block(statements: Vec<AmirStmt>, stmts: &mut AmirStmtTable) -> AmirBasicBlock {
        let mut range = DenseRange::empty();
        for stmt in statements {
            let instr = stmts.push(stmt);
            extend_block_range(&mut range, instr);
        }
        AmirBasicBlock {
            id: BlockId::from_usize(0),
            statements: range,
            params: Vec::new(),
            terminator: AmirTerminator::Return,
        }
    }

    fn make_func(
        blocks: Vec<AmirBasicBlock>,
        locals: Vec<AmirLocal>,
        temps: Vec<AmirTemp>,
        stmts: AmirStmtTable,
    ) -> AmirFunc {
        let cfg = crate::cfg::compute_cfg_edges(&blocks);
        AmirFunc {
            symbol: crate::SymbolId::new(0, 0),
            return_type: intern_ty(ArType::Void),
            receiver: None,
            params: Vec::new(),
            locals,
            temps,
            blocks,
            stmts,
            cfg,
        }
    }

    #[test]
    fn duplicate_destroy_reports_double_free() {
        let mut stmts = AmirStmtTable::new();
        let func = make_func(
            vec![block(
                vec![AmirStmt::Destroy(place(0)), AmirStmt::Destroy(place(0))],
                &mut stmts,
            )],
            vec![local(0, non_copy_ty())],
            Vec::new(),
            stmts,
        );
        let symbols = SymbolTable::new(0);
        let diags = check_moves(&func, &symbols);

        assert!(
            diags
                .iter()
                .any(|diag| diag.code == DiagCode::O005DoubleFree)
        );
    }

    #[test]
    fn local_diag_span_prefers_use_over_decl() {
        let symbols = SymbolTable::new(0);
        let mut stmts = AmirStmtTable::new();
        let func = make_func(
            vec![block(
                vec![AmirStmt::Destroy(place(0)), AmirStmt::Destroy(place(0))],
                &mut stmts,
            )],
            {
                let mut l = local(0, non_copy_ty());
                l.span = Span::new(0, 1, 2);
                l.use_span = Some(Span::new(0, 10, 15));
                vec![l]
            },
            Vec::new(),
            stmts,
        );
        let diags = check_moves(&func, &symbols);
        let d = diags
            .iter()
            .find(|d| d.code == DiagCode::O005DoubleFree)
            .expect("O005");
        assert_eq!(d.span, Span::new(0, 10, 15));
    }

    #[test]
    fn available_local_no_error() {
        let mut stmts = AmirStmtTable::new();
        let func = make_func(
            vec![block(
                vec![AmirStmt::Assign {
                    lhs: TempId::from_usize(0),
                    rhs: AmirRvalue::Load(place(0)),
                }],
                &mut stmts,
            )],
            vec![local(0, non_copy_ty())],
            vec![temp(0, non_copy_ty())],
            stmts,
        );
        let symbols = SymbolTable::new(0);
        assert!(check_moves(&func, &symbols).is_empty());
    }

    #[test]
    fn use_after_move_reports_error() {
        let mut stmts = AmirStmtTable::new();
        let func = make_func(
            vec![block(
                vec![
                    AmirStmt::Assign {
                        lhs: TempId::from_usize(0),
                        rhs: AmirRvalue::Load(place(0)),
                    },
                    AmirStmt::Destroy(place(0)),
                    AmirStmt::Assign {
                        lhs: TempId::from_usize(0),
                        rhs: AmirRvalue::Load(place(0)),
                    },
                ],
                &mut stmts,
            )],
            vec![local(0, non_copy_ty())],
            vec![temp(0, non_copy_ty())],
            stmts,
        );
        let symbols = SymbolTable::new(0);
        let diags = check_moves(&func, &symbols);
        assert!(diags.iter().any(|d| d.code == DiagCode::O001UseAfterMove));
    }

    #[test]
    fn move_on_one_branch_maybe_moved() {
        let mut stmts = AmirStmtTable::new();
        let b0 = BlockId::from_usize(0);
        let b1 = BlockId::from_usize(1);
        let b2 = BlockId::from_usize(2);
        let b3 = BlockId::from_usize(3);

        let mut range0 = DenseRange::empty();
        extend_block_range(
            &mut range0,
            stmts.push(AmirStmt::Assign {
                lhs: TempId::from_usize(0),
                rhs: AmirRvalue::Load(place(0)),
            }),
        );
        let block0 = AmirBasicBlock {
            id: b0,
            statements: range0,
            params: Vec::new(),
            terminator: AmirTerminator::Branch {
                condition: AmirOperand::Copy(TempId::from_usize(0)),
                if_true: b1,
                true_args: Vec::new(),
                if_false: b2,
                false_args: Vec::new(),
            },
        };

        let mut range1 = DenseRange::empty();
        extend_block_range(&mut range1, stmts.push(AmirStmt::Destroy(place(0))));
        let block1 = AmirBasicBlock {
            id: b1,
            statements: range1,
            params: Vec::new(),
            terminator: AmirTerminator::Goto {
                target: b3,
                args: Vec::new(),
            },
        };

        let block2 = AmirBasicBlock {
            id: b2,
            statements: DenseRange::empty(),
            params: Vec::new(),
            terminator: AmirTerminator::Goto {
                target: b3,
                args: Vec::new(),
            },
        };

        let mut range3 = DenseRange::empty();
        extend_block_range(
            &mut range3,
            stmts.push(AmirStmt::Assign {
                lhs: TempId::from_usize(0),
                rhs: AmirRvalue::Load(place(0)),
            }),
        );
        let block3 = AmirBasicBlock {
            id: b3,
            statements: range3,
            params: Vec::new(),
            terminator: AmirTerminator::Return,
        };

        let func = make_func(
            vec![block0, block1, block2, block3],
            vec![local(0, non_copy_ty())],
            vec![temp(0, non_copy_ty())],
            stmts,
        );
        let symbols = SymbolTable::new(0);
        let diags = check_moves(&func, &symbols);
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagCode::O007InconsistentMoveBetweenBranches)
        );
    }

    #[test]
    fn copy_type_move_does_not_mark_origin_moved() {
        let mut stmts = AmirStmtTable::new();
        let func = make_func(
            vec![block(
                vec![
                    AmirStmt::Assign {
                        lhs: TempId::from_usize(0),
                        rhs: AmirRvalue::Load(place(0)),
                    },
                    AmirStmt::Store {
                        lhs: place(1),
                        rhs: AmirOperand::Copy(TempId::from_usize(0)),
                    },
                ],
                &mut stmts,
            )],
            vec![local(0, int_ty())],
            vec![temp(0, int_ty())],
            stmts,
        );
        let symbols = SymbolTable::new(0);

        assert!(check_moves(&func, &symbols).is_empty());
    }
}
