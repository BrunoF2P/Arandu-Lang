use crate::amir::{AmirBasicBlock, AmirLocal, AmirProgram, AmirTemp, BlockId, LocalId};
use crate::diagnostics::{DiagCode, Diagnostic, Severity};
use crate::hir::{HirBlock, HirDecl, HirFunc, HirProgram};
use crate::literal_pool::AmirLiteralPool;
use crate::passes::type_checker::types::ArType;
use crate::{SymbolId, TypeCheckResult};
use arandu_lexer::Span;
use std::collections::HashMap;

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
    if tc.diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return Err(tc.diagnostics.clone());
    }

    let mut funcs = Vec::new();
    let mut diagnostics = Vec::new();
    let mut literal_pool = AmirLiteralPool::default();

    for decl in &hir.decls {
        if let HirDecl::Func(
            f @ HirFunc {
                body: Some(body), ..
            },
        ) = decl
        {
            match lower_func(f, body, tc, hir, &mut literal_pool) {
                Ok(amir_f) => funcs.push(amir_f),
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

pub(crate) fn amir_unsupported(span: Span, feature: &str) -> Diagnostic {
    Diagnostic::error(
        DiagCode::L002AmirUnsupportedFeature,
        format!("AMIR v0.1: {feature} is not yet supported"),
        span,
    )
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
    current_block: Option<BlockId>,
    symbol_map: HashMap<SymbolId, LocalId>,
    /// (`continue_block`, `exit_block`, `defer_frame_depth_at_loop_entry`)
    loop_stack: Vec<(BlockId, BlockId, usize)>,
    literal_pool: &'a mut AmirLiteralPool,
    defer_frames: Vec<DeferFrame>,
}
