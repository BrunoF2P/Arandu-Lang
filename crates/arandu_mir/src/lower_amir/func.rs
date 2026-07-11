#![allow(clippy::collapsible_if)]

use super::{CalleeArgModes, LowerCtx, MoveState};
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
    arg_modes: &CalleeArgModes,
    literal_pool: &mut AmirLiteralPool,
    func_diagnostics: &mut Vec<Diagnostic>,
) -> Result<AmirFunc, Diagnostic> {
    let mut ctx = LowerCtx {
        tc,
        hir,
        arg_modes,
        func_return_type: f.return_type,
        func_is_async: f.is_async,
        coroutine_depth: 0,
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
    let ret_ty = f.return_type;
    let ret_is_copy = tc.type_info.type_interner.is_copy_v01(ret_ty);
    let ret_is_nullable = tc
        .type_info
        .type_interner
        .with_type(ret_ty, |t| matches!(t, crate::types::ArType::Nullable(_)));
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
        let p_temp = ctx.with_span(param.span, |this| this.new_temp_id(param.ty));
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
        let p_local = ctx.new_local_id(param.ty, param.symbol, param.span);
        ctx.write_variable_source(p_local, AmirOperand::Copy(p_temp))?;
    }

    ctx.current_span = f.span;
    // SYN.1: only when the last statement is an expression (implicit return).
    // Empty bodies / last=`return`/`if`/… keep ordinary `lower_block` so we do
    // not invent a `Nil` store into a `void` return temp (breaks C backend).
    let last_is_expr = {
        let stmts = hir.pool.stmt_list(hir.pool.block(body).statements);
        stmts.last().is_some_and(|&sid| {
            matches!(
                hir.pool.stmt(sid).kind,
                crate::hir::HirStmtKind::Expr(_)
            )
        })
    };
    if last_is_expr {
        // Async bodies return bare `T` in source; wrap as `CoroutineReady` (A3).
        if f.is_async {
            if let crate::types::ArType::Coroutine(payload_ty) =
                tc.type_info.type_interner.resolve(ret_ty)
            {
                ctx.lower_block_as_expr_async_tail(body, payload_ty, &tc.symbols)?;
            } else {
                ctx.lower_block_as_expr(body, Some(TempId(0)), &tc.symbols)?;
            }
        } else {
            ctx.lower_block_as_expr(body, Some(TempId(0)), &tc.symbols)?;
        }
    } else {
        ctx.lower_block(body, &tc.symbols)?;
    }

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

    // A3.4: rewrite Borrow→RelativeBorrow (LocalId index) for refs that cross Suspend;
    // *p becomes Load of the local so addresses never pin the state blob.
    crate::pin_free::apply_pin_free_refs(&mut amir_f, &tc.type_info.type_interner);

    // M2: O002/O003/O006 on the *final* AMIR (after prune/rewrite).
    // Dummy Store of `&T` locals would otherwise hide holder liveness on raw AMIR.
    func_diagnostics.extend(crate::borrow_check::check_borrows(&amir_f, &tc.symbols));

    // A3.2: remaining absolute Ref/RefMut live into resume → O010.
    func_diagnostics.extend(crate::suspend_check::check_borrow_across_suspend(
        &amir_f,
        &tc.symbols,
        &tc.type_info.type_interner,
    ));

    // F2.3 + G2: escape analysis (O010 / O004); `@no_fallback` promotes O004→error.
    let escape_opts = crate::escape_analysis::EscapeCheckOptions {
        no_fallback: f.no_fallback,
    };
    func_diagnostics.extend(crate::escape_analysis::check_escapes(
        &amir_f,
        &tc.symbols,
        &tc.type_info.type_interner,
        escape_opts,
    ));
    // F2.3.runtime: materialize GenInsert/GenGet for escaping int locals (MVP).
    crate::gen_promote::apply_gen_promotion(&mut amir_f, &tc.type_info.type_interner, escape_opts);

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
