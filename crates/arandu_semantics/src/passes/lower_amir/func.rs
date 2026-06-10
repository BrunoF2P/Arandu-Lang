#![allow(clippy::collapsible_if)]

use super::{LowerCtx, MoveState};
use crate::TypeCheckResult;
use crate::amir::{
    AmirFunc, AmirOperand, AmirPlace, AmirStmtTable, AmirTemp, AmirTerminator, TempId,
};
use crate::cfg::compute_cfg_edges;
use crate::diagnostics::Diagnostic;
use crate::hir::{HirBlockId, HirFunc, HirProgram};
use crate::literal_pool::AmirLiteralPool;
use fxhash::FxHashMap;

pub(crate) fn lower_func(
    f: &HirFunc,
    body: HirBlockId,
    tc: &TypeCheckResult,
    hir: &HirProgram,
    literal_pool: &mut AmirLiteralPool,
) -> Result<AmirFunc, Diagnostic> {
    let mut ctx = LowerCtx {
        tc,
        hir,
        func_return_type: f.return_type.clone(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: Vec::new(),
        stmts: AmirStmtTable::new(),
        current_block: None,
        symbol_map: FxHashMap::default(),
        loop_stack: Vec::new(),
        literal_pool,
        defer_frames: Vec::new(),
        temp_states: Vec::new(),
        temp_origins: Vec::new(),
        local_states: Vec::new(),
    };

    // Return register is TempId(0)
    ctx.temps.push(AmirTemp {
        id: TempId(0),
        ty: f.return_type.clone(),
        span: arandu_lexer::Span::new(0, 0, 0, 0, 0, 0),
    });
    ctx.temp_states.push(MoveState::Available);
    ctx.temp_origins.push(None);

    let mut params = Vec::new();
    let mut receiver = None;

    // Start with bb0 so we can emit parameter store instructions there
    let bb0 = ctx.new_block();
    ctx.current_block = Some(bb0);

    for param in &f.params {
        let p_temp = ctx.new_temp(param.ty.clone());
        if param.is_receiver {
            receiver = Some(crate::amir::AmirReceiver {
                temp: p_temp,
                kind: param
                    .receiver_kind
                    .unwrap_or(crate::hir::ReceiverKind::Shared),
            });
        }
        params.push(p_temp);

        // Copy incoming parameter SSA register value to local stack slot
        let p_local = ctx.new_local(param.ty.clone(), param.symbol, param.span);
        ctx.emit_store_place(
            AmirPlace {
                local: p_local,
                projections: smallvec::SmallVec::new(),
            },
            AmirOperand::Copy(p_temp),
        )?;
    }

    ctx.lower_block(body, &tc.symbols)?;

    // If last block does not have a terminator, implicitly return
    if let Some(curr) = ctx.current_block {
        if ctx.blocks[curr.as_usize()].terminator.is_unreachable() {
            ctx.blocks[curr.as_usize()].terminator = AmirTerminator::Return;
        }
    }

    compute_cfg_edges(&mut ctx.blocks);

    Ok(AmirFunc {
        symbol: f.symbol,
        return_type: f.return_type.clone(),
        receiver,
        params,
        locals: ctx.locals,
        temps: ctx.temps,
        blocks: ctx.blocks,
        stmts: ctx.stmts,
    })
}

// Extension helper to check if terminator is unreachable
trait TerminatorExt {
    fn is_unreachable(&self) -> bool;
}

impl TerminatorExt for AmirTerminator {
    fn is_unreachable(&self) -> bool {
        matches!(self, AmirTerminator::Unreachable)
    }
}
