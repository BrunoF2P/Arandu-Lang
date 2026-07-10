#![allow(clippy::collapsible_if)]

use super::{LowerCtx, MoveState};
use crate::TypeCheckResult;
use crate::amir::{AmirFunc, AmirOperand, AmirStmtTable, AmirTemp, AmirTerminator, TempId};
use crate::diagnostics::Diagnostic;
use crate::hir::{HirBlockId, HirFunc, HirProgram};
use crate::literal_pool::AmirLiteralPool;
use rustc_hash::{FxHashMap, FxHashSet};

pub(crate) fn lower_func(
    f: &HirFunc,
    body: HirBlockId,
    tc: &TypeCheckResult,
    hir: &HirProgram,
    literal_pool: &mut AmirLiteralPool,
    func_diagnostics: &mut Vec<Diagnostic>,
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
        predecessors: FxHashMap::default(),
        sealed_blocks: FxHashSet::default(),
        current_def: FxHashMap::default(),
        incomplete_phis: FxHashMap::default(),
        redirected_temps: FxHashMap::default(),
        current_span: arandu_lexer::Span::new(0, 0, 0),
    };

    // Return register is TempId(0) — span is the function header.
    let ret_is_copy = f.return_type.is_copy_v01();
    let ret_is_nullable = matches!(f.return_type, crate::types::ArType::Nullable(_));
    let ret_ty = ctx.intern_ty(f.return_type.clone());
    ctx.temps.push(AmirTemp {
        id: TempId(0),
        ty: ret_ty,
        is_copy: ret_is_copy,
        is_nullable: ret_is_nullable,
        span: f.span,
    });
    ctx.temp_states.push(MoveState::Available);
    ctx.temp_origins.push(None);

    let mut params = Vec::new();
    let mut receiver = None;

    // Start with bb0 so we can emit parameter store instructions there
    let bb0 = ctx.new_block();
    ctx.sealed_blocks.insert(bb0);
    ctx.current_block = Some(bb0);

    for param in hir.pool.params_list(f.params) {
        let p_temp = ctx.with_span(param.span, |this| this.new_temp(param.ty.clone()));
        if param.is_receiver {
            receiver = Some(crate::amir::AmirReceiver {
                temp: p_temp,
                kind: param
                    .receiver_kind
                    .unwrap_or(crate::hir::ReceiverKind::Shared),
            });
        }
        params.push(p_temp);
        // Directly bind the parameter temp to the local variable in the entry block
        let p_local = ctx.new_local(param.ty.clone(), param.symbol, param.span);
        ctx.write_variable_source(p_local, AmirOperand::Copy(p_temp))?;
    }

    ctx.current_span = f.span;
    ctx.lower_block(body, &tc.symbols)?;

    // If last block does not have a terminator, implicitly return
    if let Some(curr) = ctx.current_block {
        if ctx.blocks[curr.as_usize()].terminator.is_unreachable() {
            ctx.blocks[curr.as_usize()].terminator = AmirTerminator::Return;
        }
    }

    // Seal all remaining unsealed blocks
    for i in 0..ctx.blocks.len() {
        ctx.seal_block(crate::amir::BlockId::from_usize(i));
    }

    // OSSA Optimization passes
    // 1. Snapshot func for validation without cloning Vecs (take → check → put back).
    let raw_cfg = crate::cfg::compute_cfg_edges(&ctx.blocks);
    let mut raw_func = AmirFunc {
        symbol: f.symbol,
        return_type: ret_ty,
        receiver,
        params: params.clone(),
        locals: std::mem::take(&mut ctx.locals),
        temps: std::mem::take(&mut ctx.temps),
        blocks: std::mem::take(&mut ctx.blocks),
        stmts: std::mem::take(&mut ctx.stmts),
        cfg: raw_cfg,
    };

    // 2. Run checks on raw func
    func_diagnostics.extend(crate::amir_validate::validate_amir_func(
        &raw_func,
        &tc.symbols,
        &tc.type_info.type_interner,
    ));
    func_diagnostics.extend(crate::definite_init::check_definite_init(
        &raw_func,
        &tc.symbols,
    ));
    func_diagnostics.extend(crate::move_checker::check_moves(&raw_func, &tc.symbols));

    // Restore into ctx for phi elimination / rewrite.
    ctx.locals = std::mem::take(&mut raw_func.locals);
    ctx.temps = std::mem::take(&mut raw_func.temps);
    ctx.blocks = std::mem::take(&mut raw_func.blocks);
    ctx.stmts = std::mem::take(&mut raw_func.stmts);
    drop(raw_func);

    // 3. OSSA Optimization passes
    ctx.eliminate_trivial_phis();
    ctx.prune_eliminated_parameters();
    ctx.rewrite_all_operands();

    let cfg = crate::cfg::compute_cfg_edges(&ctx.blocks);
    let mut amir_f = AmirFunc {
        symbol: f.symbol,
        return_type: ret_ty,
        receiver,
        params,
        locals: ctx.locals,
        temps: ctx.temps,
        blocks: ctx.blocks,
        stmts: ctx.stmts,
        cfg,
    };

    // 4. Prune dummy loads and stores
    super::prune_dummy_loads_stores(&mut amir_f);

    Ok(amir_f)
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
