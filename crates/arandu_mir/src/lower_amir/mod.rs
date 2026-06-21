use crate::amir::{
    AmirBasicBlock, AmirLocal, AmirProgram, AmirStmtTable, AmirTemp, BlockId, LocalId,
};
use crate::amir_validate::validate_amir_func;
use crate::definite_init::check_definite_init;
use crate::diagnostics::{DiagCode, Diagnostic, Severity};
use crate::hir::{HirBlock, HirDecl, HirFunc, HirProgram};
use crate::literal_pool::AmirLiteralPool;
use crate::move_checker::check_moves;
use crate::passes::type_checker::types::ArType;
use crate::{SymbolId, TypeCheckResult};
use arandu_lexer::Span;
use rustc_hash::FxHashMap;

mod ctx;
mod expr;
mod func;
mod match_lower;
mod pattern;
mod stmt;

pub(crate) use func::lower_func;

pub fn lower_to_amir(
    tc: &TypeCheckResult,
    hir: &HirProgram,
) -> Result<AmirProgram, Vec<Diagnostic>> {
    let _scope =
        arandu_middle::types::type_interner::InternerScope::new(&tc.type_info.type_interner);
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
            match lower_func(f, *body, tc, hir, &mut literal_pool) {
                Ok(amir_f) => {
                    diagnostics.extend(validate_amir_func(&amir_f, &tc.symbols));
                    diagnostics.extend(check_definite_init(&amir_f, &tc.symbols));
                    diagnostics.extend(check_moves(&amir_f, &tc.symbols));
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
        })
    } else {
        Err(diagnostics)
    }
}

pub(crate) fn amir_unsupported(span: Span, feature: &str, roadmap: &str) -> Diagnostic {
    Diagnostic::error(
        DiagCode::U001FeatureNotSupported,
        format!("AMIR v0.1: {feature} is not supported yet ({roadmap})"),
        span,
    )
    .with_hint("see docs/arandu-compiler-roadmap-v0.1.md for the planned milestone")
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeferKind {
    Defer,
    ErrDefer,
}

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MoveState {
    Available,
    Moved,
}
