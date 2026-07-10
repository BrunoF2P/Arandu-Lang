//! AMIR lowering pass.
//!
//! Transforms a [`HirProgram`] (High-level IR) into an [`AmirProgram`]
//! (Arandu Mid-level IR). Each HIR function is independently lowered into
//! SSA-like AMIR basic blocks. Aborts early if type-checking already failed.

use crate::amir::{
    AmirBasicBlock, AmirFunc, AmirLocal, AmirOperand, AmirProgram, AmirRvalue, AmirStmt,
    AmirStmtTable, AmirTemp, BlockId, LocalId, TempId,
};
use crate::diagnostics::{DiagCode, Diagnostic, Severity};
use crate::hir::{HirBlock, HirDecl, HirFunc, HirProgram};
use crate::literal_pool::AmirLiteralPool;
use crate::passes::type_checker::types::{ArType, Primitive};
use crate::{SymbolId, TypeCheckResult};
use arandu_lexer::Span;
use rustc_hash::{FxHashMap, FxHashSet};

mod ctx;
mod expr;
mod func;
mod match_lower;
mod ops;
mod pattern;
mod place;
mod stmt;

pub(crate) use func::lower_func;

/// Lowers a [`HirProgram`] into an [`AmirProgram`].
///
/// Returns `Err` immediately if `tc` already contains any [`Severity::Error`]
/// diagnostics. Each function is lowered independently; partial errors are
/// collected and returned together so the caller sees all failures at once.
#[tracing::instrument(level = "trace", target = "arandu_mir::lower_amir", skip(tc, hir))]
pub fn lower_to_amir(
    tc: &TypeCheckResult,
    hir: &HirProgram,
) -> Result<AmirProgram, Vec<Diagnostic>> {
    if tc.diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return Err(tc.diagnostics.clone());
    }

    let mut funcs = Vec::new();
    let mut diagnostics = Vec::new();
    let mut literal_pool = AmirLiteralPool::default();

    for &decl_id in &hir.decls {
        if let HirDecl::Func(
            f @ HirFunc {
                body: Some(body), ..
            },
        ) = hir.pool.decl(decl_id)
        {
            match lower_func(f, *body, tc, hir, &mut literal_pool, &mut diagnostics) {
                Ok(amir_f) => {
                    funcs.push(amir_f);
                }
                Err(diag) => diagnostics.push(diag),
            }
        }
    }

    if diagnostics.is_empty() {
        Ok(AmirProgram {
            funcs,
            literal_pool,
            extern_funcs: rustc_hash::FxHashMap::default(),
        })
    } else {
        Err(diagnostics)
    }
}

pub(crate) fn is_memory_type(ty: &ArType) -> bool {
    match ty {
        ArType::Primitive(p) => matches!(p, Primitive::Str | Primitive::Any),
        ArType::IntLiteral | ArType::FloatLiteral | ArType::Void | ArType::Err | ArType::Error => {
            false
        }
        ArType::Ptr(_) | ArType::Nullable(_) | ArType::Func(_, _) | ArType::Slice(_) => false,
        ArType::Array(_, _)
        | ArType::Named(_, _)
        | ArType::Tuple(_)
        | ArType::Option(_)
        | ArType::Result(_, _)
        | ArType::Coroutine(_)
        | ArType::Range(_) => true,
    }
}

pub fn prune_dummy_loads_stores(func: &mut AmirFunc) {
    let mut new_stmts = AmirStmtTable::new();
    let mut new_blocks = Vec::with_capacity(func.blocks.len());

    for block in &func.blocks {
        let new_range_start = new_stmts.len();
        let mut new_range_len = 0;

        for stmt_id in func.block_stmt_ids(block.id) {
            let stmt = func
                .stmts
                .get(stmt_id)
                // Safety: stmt_id comes from block_stmt_ids(), which only
                // yields IDs that were inserted into func.stmts during lowering.
                .expect("stmt_id from block_stmt_ids is always present in func.stmts");
            let keep = match stmt {
                AmirStmt::Store { lhs, .. } if lhs.projections.is_empty() => {
                    func.locals[lhs.local.as_usize()].is_memory
                }
                AmirStmt::Assign {
                    rhs: AmirRvalue::Load(place),
                    ..
                } if place.projections.is_empty() => func.locals[place.local.as_usize()].is_memory,
                _ => true,
            };

            if keep {
                new_stmts.push(stmt.clone());
                new_range_len += 1;
            }
        }

        new_blocks.push(AmirBasicBlock {
            id: block.id,
            params: block.params.clone(),
            statements: crate::layout::DenseRange::new(new_range_start, new_range_len),
            terminator: block.terminator.clone(),
        });
    }

    func.stmts = new_stmts;
    func.blocks = new_blocks;
    func.cfg = crate::cfg::compute_cfg_edges(&func.blocks);
}

pub(crate) fn amir_unsupported(span: Span, feature: &str, roadmap: &str) -> Diagnostic {
    Diagnostic::error(
        DiagCode::U001FeatureNotSupported,
        format!("AMIR v0.1: {feature} is not supported yet ({roadmap})"),
        span,
    )
    .with_hint("see docs/arandu-compiler-roadmap-v0.1.md for the planned milestone")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeferKind {
    Defer,
    ErrDefer,
}

#[derive(Clone)]
pub(crate) struct DeferFrame {
    entries: Vec<(HirBlock, DeferKind)>,
}

pub(crate) struct LowerCtx<'a> {
    tc: &'a TypeCheckResult,
    hir: &'a HirProgram,
    func_return_type: ArType,
    locals: Vec<AmirLocal>,
    temps: Vec<AmirTemp>,
    blocks: Vec<AmirBasicBlock>,
    stmts: AmirStmtTable,
    current_block: Option<BlockId>,
    symbol_map: FxHashMap<SymbolId, LocalId>,
    /// (`continue_block`, `exit_block`, `defer_frame_depth_at_loop_entry`)
    loop_stack: Vec<(BlockId, BlockId, usize)>,
    literal_pool: &'a mut AmirLiteralPool,
    defer_frames: Vec<DeferFrame>,
    temp_states: Vec<MoveState>,
    temp_origins: Vec<Option<LocalId>>,
    local_states: Vec<MoveState>,

    // SSA builder fields (OSSA Braun et al.)
    predecessors: FxHashMap<BlockId, Vec<BlockId>>,
    sealed_blocks: FxHashSet<BlockId>,
    current_def: FxHashMap<(BlockId, LocalId), AmirOperand>,
    incomplete_phis: FxHashMap<BlockId, Vec<(LocalId, TempId)>>,
    redirected_temps: FxHashMap<TempId, AmirOperand>,
    /// Span of the HIR construct currently being lowered (for `use_span` / diags).
    current_span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MoveState {
    Available,
    Moved,
}
