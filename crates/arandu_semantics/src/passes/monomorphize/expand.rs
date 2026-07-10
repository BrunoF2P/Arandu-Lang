//! HIR monomorphization expand — free-function and method specialization.
//!
//! Pipeline step after [`super::analyze_instantiations`]:
//! 1. For each **concrete** instantiation key `(F, [T1, …])` of a generic
//!    free function or method, clone the template body with type-parameter
//!    substitution and a fresh mangled symbol.
//! 2. Rewrite call sites:
//!    - `Call(Generic(Path(F), [T1,…]), args)` → `Call(Path(F_spec), args)`
//!    - `Call(Generic(Field(recv, m), [T1,…]), args)` → same Path rewrite
//!      (receiver is already the first arg from HIR method lowering)
//! 3. Generic **templates** remain in the HIR for diagnostics/pretty-print but
//!    are skipped by AMIR lowering (see `lower_to_amir`).

use arandu_diagnostics::{DiagCode, Diagnostic};
use arandu_lexer::Span;
use arandu_middle::hir::{
    HirBindingItem, HirBlock, HirBlockId, HirCatchHandler, HirCondition, HirDecl, HirExpr,
    HirExprId, HirExprKind, HirForBinding, HirForClause, HirFunc, HirLambdaBody, HirLambdaParam,
    HirMatchArm, HirMatchArmBody, HirParam, HirPattern, HirPlace, HirPlaceSuffix, HirProgram,
    HirSimpleStmt, HirStmt, HirStmtKind, HirStringPart,
};
use arandu_middle::symbol_table::{SymbolId, SymbolKind};
use arandu_middle::types::{ArType, GenericSubst, TypeId, build_subst_ids, substitute_type_id};
use arandu_typeck::TypeCheckResult;
use rustc_hash::FxHashMap;

use super::graph::{InstantiationGraph, InstantiationKey};

/// Expand free-function and method specializations; rewrite call sites in-place.
///
/// Returns the number of specialized functions appended to `hir`.
#[tracing::instrument(level = "debug", target = "arandu_semantics::mono", skip_all)]
pub fn expand_specializations(
    tc: &mut TypeCheckResult,
    hir: &mut HirProgram,
    graph: &InstantiationGraph,
) -> Result<usize, Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();

    // Collect template free funcs: SymbolId → decl index in hir.decls
    let mut template_funcs: FxHashMap<SymbolId, usize> = FxHashMap::default();
    for (i, &decl_id) in hir.decls.iter().enumerate() {
        if let HirDecl::Func(f) = hir.pool.decl(decl_id)
            && f.body.is_some()
            && tc.type_info.generic_params.contains_key(&f.symbol)
        {
            template_funcs.insert(f.symbol, i);
        }
    }

    // Concrete keys only (skip identity template nodes).
    let concrete: Vec<InstantiationKey> = graph
        .iter()
        .map(|n| n.key.clone())
        .filter(|key| {
            template_funcs.contains_key(&key.symbol)
                && !is_identity_instantiation(tc, key.symbol, &key.type_args)
        })
        .collect();

    if concrete.is_empty() {
        // Still rewrite is a no-op; done.
        return Ok(0);
    }

    // key → specialized function symbol
    let mut specialized: FxHashMap<InstantiationKey, SymbolId> = FxHashMap::default();
    let mut created = 0usize;

    for key in &concrete {
        if specialized.contains_key(key) {
            continue;
        }
        match specialize_free_func(tc, hir, key, &template_funcs) {
            Ok(sym) => {
                specialized.insert(key.clone(), sym);
                created += 1;
            }
            Err(d) => diagnostics.push(d),
        }
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    // Rewrite call sites in every function body (templates + monomorphized + monomorphic).
    let decl_ids: Vec<_> = hir.decls.clone();
    for &decl_id in &decl_ids {
        let body = match hir.pool.decl(decl_id) {
            HirDecl::Func(f) => f.body,
            _ => None,
        };
        if let Some(body) = body {
            rewrite_block_calls(hir, body, &specialized, tc);
        }
    }

    Ok(created)
}

fn is_identity_instantiation(tc: &TypeCheckResult, symbol: SymbolId, type_args: &[TypeId]) -> bool {
    let Some(params) = tc.type_info.generic_params.get(&symbol) else {
        return false;
    };
    if params.len() != type_args.len() {
        return false;
    }
    let interner = &tc.type_info.type_interner;
    params.iter().zip(type_args.iter()).all(|(&param, &tid)| {
        matches!(
            interner.resolve(tid),
            ArType::Named(id, ref args) if id == param && args.is_empty()
        )
    })
}

fn specialize_free_func(
    tc: &mut TypeCheckResult,
    hir: &mut HirProgram,
    key: &InstantiationKey,
    template_funcs: &FxHashMap<SymbolId, usize>,
) -> Result<SymbolId, Diagnostic> {
    let &decl_idx = template_funcs.get(&key.symbol).ok_or_else(|| {
        Diagnostic::error(
            DiagCode::G001GenericInstantiationCycle,
            "monomorphize: template function missing from HIR".to_string(),
            Span::new(0, 0, 0),
        )
    })?;
    let template_decl_id = hir.decls[decl_idx];
    let template = match hir.pool.decl(template_decl_id) {
        HirDecl::Func(f) => f.clone_shallow(),
        _ => {
            return Err(Diagnostic::error(
                DiagCode::G001GenericInstantiationCycle,
                "monomorphize: expected function template".to_string(),
                Span::new(0, 0, 0),
            ));
        }
    };
    let body_id = template.body.ok_or_else(|| {
        Diagnostic::error(
            DiagCode::G001GenericInstantiationCycle,
            "monomorphize: template has no body".to_string(),
            template.span,
        )
    })?;

    let params_list = tc
        .type_info
        .generic_params
        .get(&key.symbol)
        .cloned()
        .ok_or_else(|| {
            Diagnostic::error(
                DiagCode::G001GenericInstantiationCycle,
                "monomorphize: missing generic_params".to_string(),
                template.span,
            )
        })?;

    if params_list.len() != key.type_args.len() {
        return Err(Diagnostic::error(
            DiagCode::G002GenericInstantiationLimit,
            format!(
                "generic argument count mismatch for `{}`: expected {}, found {}",
                tc.symbols.get(key.symbol).name,
                params_list.len(),
                key.type_args.len()
            ),
            template.span,
        ));
    }

    let mangled = super::demangle::mangle_symbol(key, &tc.type_info.type_interner, &tc.symbols);

    let global = tc.symbols.global_scope();
    let new_func_sym = tc
        .symbols_mut()
        .define(global, &mangled, SymbolKind::Func, template.span)
        .map_err(|_| {
            Diagnostic::error(
                DiagCode::G001GenericInstantiationCycle,
                format!("monomorphize: duplicate specialized symbol `{mangled}`"),
                template.span,
            )
        })?;

    // Subst and specialized return type
    let subst = build_subst_ids(&params_list, &key.type_args, &tc.type_info.type_interner);
    let ret_ty = substitute_type_id(template.return_type, &subst, &tc.type_info.type_interner);

    // Register specialized function type Func(params, ret) for decl_type lookup
    let mut param_tids = Vec::new();
    let mut symbol_map: FxHashMap<SymbolId, SymbolId> = FxHashMap::default();
    let old_params: Vec<HirParam> = hir.pool.params_list(template.params).to_vec();
    let mut new_params = Vec::with_capacity(old_params.len());
    for (i, p) in old_params.iter().enumerate() {
        let new_ty = substitute_type_id(p.ty, &subst, &tc.type_info.type_interner);
        param_tids.push(new_ty);
        let pname = format!("${i}_{}", tc.symbols.get(p.symbol).name);
        let global = tc.symbols.global_scope();
        let new_sym = tc
            .symbols_mut()
            .define(
                global,
                &format!("{mangled}{pname}"),
                SymbolKind::Param,
                p.span,
            )
            .unwrap_or_else(|existing| existing);
        symbol_map.insert(p.symbol, new_sym);
        tc.type_info_mut().record_decl_type(new_sym, new_ty);
        new_params.push(HirParam {
            symbol: new_sym,
            ty: new_ty,
            span: p.span,
            is_receiver: p.is_receiver,
            receiver_kind: p.receiver_kind,
        });
    }
    let func_ty = ArType::Func(param_tids, ret_ty);
    let func_ty_id = tc.type_info.type_interner.intern(func_ty);
    tc.type_info_mut()
        .record_decl_type(new_func_sym, func_ty_id);

    let new_params_range = hir.pool.alloc_param_list(&new_params);
    let new_body = clone_block(hir, body_id, &subst, &mut symbol_map, tc, &mangled)?;

    let specialized = HirFunc {
        symbol: new_func_sym,
        params: new_params_range,
        return_type: ret_ty,
        body: Some(new_body),
        span: template.span,
    };
    let decl_id = hir.pool.alloc_decl(HirDecl::Func(specialized));
    hir.decls.push(decl_id);
    Ok(new_func_sym)
}

/// Clone a template [`HirFunc`] fields we need without cloning the whole pool.
trait CloneShallow {
    fn clone_shallow(&self) -> Self;
}

impl CloneShallow for HirFunc {
    fn clone_shallow(&self) -> Self {
        Self {
            symbol: self.symbol,
            params: self.params,
            return_type: self.return_type,
            body: self.body,
            span: self.span,
        }
    }
}

fn clone_block(
    hir: &mut HirProgram,
    block_id: HirBlockId,
    subst: &GenericSubst,
    symbol_map: &mut FxHashMap<SymbolId, SymbolId>,
    tc: &mut TypeCheckResult,
    name_prefix: &str,
) -> Result<HirBlockId, Diagnostic> {
    let block = hir.pool.block(block_id).clone();
    let old_stmts: Vec<_> = hir.pool.stmt_list(block.statements).to_vec();
    let mut new_stmt_ids = Vec::with_capacity(old_stmts.len());
    for &sid in &old_stmts {
        let stmt = hir.pool.stmt(sid).clone();
        let new_kind = clone_stmt_kind(hir, &stmt.kind, subst, symbol_map, tc, name_prefix)?;
        new_stmt_ids.push(hir.pool.alloc_stmt(HirStmt {
            kind: new_kind,
            span: stmt.span,
        }));
    }
    let range = hir.pool.alloc_stmt_list(&new_stmt_ids);
    Ok(hir.pool.alloc_block(HirBlock {
        statements: range,
        span: block.span,
    }))
}

fn clone_stmt_kind(
    hir: &mut HirProgram,
    kind: &HirStmtKind,
    subst: &GenericSubst,
    symbol_map: &mut FxHashMap<SymbolId, SymbolId>,
    tc: &mut TypeCheckResult,
    name_prefix: &str,
) -> Result<HirStmtKind, Diagnostic> {
    Ok(match kind {
        HirStmtKind::VarDecl { bindings, value } => {
            let old_b: Vec<_> = hir.pool.bindings_list(*bindings).to_vec();
            let mut new_b = Vec::with_capacity(old_b.len());
            for b in &old_b {
                let new_ty = substitute_type_id(b.ty, subst, &tc.type_info.type_interner);
                let bname = tc.symbols.get(b.symbol).name.clone();
                let new_sym = fresh_symbol(
                    tc,
                    &format!("{name_prefix}${bname}"),
                    SymbolKind::Local,
                    b.span,
                );
                symbol_map.insert(b.symbol, new_sym);
                tc.type_info_mut().record_decl_type(new_sym, new_ty);
                new_b.push(HirBindingItem {
                    symbol: new_sym,
                    ty: new_ty,
                    span: b.span,
                });
            }
            let br = hir.pool.alloc_binding_list(&new_b);
            let value = clone_expr(hir, *value, subst, symbol_map, tc, name_prefix)?;
            HirStmtKind::VarDecl {
                bindings: br,
                value,
            }
        }
        HirStmtKind::Set { places, op, value } => {
            let old_p: Vec<_> = hir.pool.places_list(*places).to_vec();
            let mut new_p = Vec::with_capacity(old_p.len());
            for p in &old_p {
                new_p.push(clone_place(hir, p, subst, symbol_map, tc, name_prefix)?);
            }
            let pr = hir.pool.alloc_place_list(&new_p);
            let value = clone_expr(hir, *value, subst, symbol_map, tc, name_prefix)?;
            HirStmtKind::Set {
                places: pr,
                op: *op,
                value,
            }
        }
        HirStmtKind::Return { values } => {
            let old_v: Vec<_> = hir.pool.expr_list(*values).to_vec();
            let mut new_v = Vec::with_capacity(old_v.len());
            for &e in &old_v {
                new_v.push(clone_expr(hir, e, subst, symbol_map, tc, name_prefix)?);
            }
            HirStmtKind::Return {
                values: hir.pool.alloc_expr_list(&new_v),
            }
        }
        HirStmtKind::Break => HirStmtKind::Break,
        HirStmtKind::Continue => HirStmtKind::Continue,
        HirStmtKind::Free(e) => {
            HirStmtKind::Free(clone_expr(hir, *e, subst, symbol_map, tc, name_prefix)?)
        }
        HirStmtKind::Expr(e) => {
            HirStmtKind::Expr(clone_expr(hir, *e, subst, symbol_map, tc, name_prefix)?)
        }
        HirStmtKind::If {
            condition,
            then_block,
            else_block,
        } => HirStmtKind::If {
            condition: clone_condition(hir, condition, subst, symbol_map, tc, name_prefix)?,
            then_block: clone_block(hir, *then_block, subst, symbol_map, tc, name_prefix)?,
            else_block: else_block
                .map(|b| clone_block(hir, b, subst, symbol_map, tc, name_prefix))
                .transpose()?,
        },
        HirStmtKind::For { clause, body } => HirStmtKind::For {
            clause: clone_for_clause(hir, clause, subst, symbol_map, tc, name_prefix)?,
            body: clone_block(hir, *body, subst, symbol_map, tc, name_prefix)?,
        },
        HirStmtKind::While { condition, body } => HirStmtKind::While {
            condition: clone_condition(hir, condition, subst, symbol_map, tc, name_prefix)?,
            body: clone_block(hir, *body, subst, symbol_map, tc, name_prefix)?,
        },
        HirStmtKind::Match { value, arms } => {
            let value = clone_expr(hir, *value, subst, symbol_map, tc, name_prefix)?;
            let old_arms: Vec<_> = hir.pool.match_arms_list(*arms).to_vec();
            let mut new_arms = Vec::with_capacity(old_arms.len());
            for arm in &old_arms {
                new_arms.push(clone_match_arm(
                    hir,
                    arm,
                    subst,
                    symbol_map,
                    tc,
                    name_prefix,
                )?);
            }
            HirStmtKind::Match {
                value,
                arms: hir.pool.alloc_match_arm_list(&new_arms),
            }
        }
        HirStmtKind::Defer(b) => {
            HirStmtKind::Defer(clone_block(hir, *b, subst, symbol_map, tc, name_prefix)?)
        }
        HirStmtKind::ErrDefer(b) => {
            HirStmtKind::ErrDefer(clone_block(hir, *b, subst, symbol_map, tc, name_prefix)?)
        }
        HirStmtKind::Unsafe(b) => {
            HirStmtKind::Unsafe(clone_block(hir, *b, subst, symbol_map, tc, name_prefix)?)
        }
        HirStmtKind::Error => HirStmtKind::Error,
    })
}

fn clone_for_clause(
    hir: &mut HirProgram,
    clause: &HirForClause,
    subst: &GenericSubst,
    symbol_map: &mut FxHashMap<SymbolId, SymbolId>,
    tc: &mut TypeCheckResult,
    name_prefix: &str,
) -> Result<HirForClause, Diagnostic> {
    Ok(match clause {
        HirForClause::In {
            span,
            bindings,
            iterable,
        } => {
            let old_b: Vec<_> = hir.pool.for_bindings_list(*bindings).to_vec();
            let mut new_b = Vec::with_capacity(old_b.len());
            for b in &old_b {
                let new_ty = substitute_type_id(b.ty, subst, &tc.type_info.type_interner);
                let bname = tc.symbols.get(b.symbol).name.clone();
                let new_sym = fresh_symbol(
                    tc,
                    &format!("{name_prefix}$for_{bname}"),
                    SymbolKind::Local,
                    b.span,
                );
                symbol_map.insert(b.symbol, new_sym);
                tc.type_info_mut().record_decl_type(new_sym, new_ty);
                new_b.push(HirForBinding {
                    symbol: new_sym,
                    ty: new_ty,
                    span: b.span,
                });
            }
            HirForClause::In {
                span: *span,
                bindings: hir.pool.alloc_for_binding_list(&new_b),
                iterable: clone_expr(hir, *iterable, subst, symbol_map, tc, name_prefix)?,
            }
        }
        HirForClause::CStyle {
            span,
            init,
            condition,
            step,
        } => HirForClause::CStyle {
            span: *span,
            init: init
                .as_ref()
                .map(|s| clone_simple_stmt(hir, s, subst, symbol_map, tc, name_prefix))
                .transpose()?,
            condition: condition
                .map(|e| clone_expr(hir, e, subst, symbol_map, tc, name_prefix))
                .transpose()?,
            step: step
                .as_ref()
                .map(|s| clone_simple_stmt(hir, s, subst, symbol_map, tc, name_prefix))
                .transpose()?,
        },
    })
}

fn clone_simple_stmt(
    hir: &mut HirProgram,
    stmt: &HirSimpleStmt,
    subst: &GenericSubst,
    symbol_map: &mut FxHashMap<SymbolId, SymbolId>,
    tc: &mut TypeCheckResult,
    name_prefix: &str,
) -> Result<HirSimpleStmt, Diagnostic> {
    Ok(match stmt {
        HirSimpleStmt::VarDecl { bindings, value } => {
            let kind = clone_stmt_kind(
                hir,
                &HirStmtKind::VarDecl {
                    bindings: *bindings,
                    value: *value,
                },
                subst,
                symbol_map,
                tc,
                name_prefix,
            )?;
            match kind {
                HirStmtKind::VarDecl { bindings, value } => {
                    HirSimpleStmt::VarDecl { bindings, value }
                }
                _ => unreachable!(),
            }
        }
        HirSimpleStmt::Set { places, op, value } => {
            let kind = clone_stmt_kind(
                hir,
                &HirStmtKind::Set {
                    places: *places,
                    op: *op,
                    value: *value,
                },
                subst,
                symbol_map,
                tc,
                name_prefix,
            )?;
            match kind {
                HirStmtKind::Set { places, op, value } => HirSimpleStmt::Set { places, op, value },
                _ => unreachable!(),
            }
        }
        HirSimpleStmt::Expr(e) => {
            HirSimpleStmt::Expr(clone_expr(hir, *e, subst, symbol_map, tc, name_prefix)?)
        }
    })
}

fn clone_match_arm(
    hir: &mut HirProgram,
    arm: &HirMatchArm,
    subst: &GenericSubst,
    symbol_map: &mut FxHashMap<SymbolId, SymbolId>,
    tc: &mut TypeCheckResult,
    name_prefix: &str,
) -> Result<HirMatchArm, Diagnostic> {
    let pattern = clone_pattern_id(hir, arm.pattern, subst, symbol_map, tc, name_prefix)?;
    let guard = arm
        .guard
        .map(|g| clone_expr(hir, g, subst, symbol_map, tc, name_prefix))
        .transpose()?;
    let body = match &arm.body {
        HirMatchArmBody::Expr(e) => {
            HirMatchArmBody::Expr(clone_expr(hir, *e, subst, symbol_map, tc, name_prefix)?)
        }
        HirMatchArmBody::Block(b) => {
            HirMatchArmBody::Block(clone_block(hir, *b, subst, symbol_map, tc, name_prefix)?)
        }
    };
    Ok(HirMatchArm {
        span: arm.span,
        pattern,
        guard,
        body,
    })
}

fn clone_pattern_id(
    hir: &mut HirProgram,
    pat_id: arandu_middle::hir::HirPatternId,
    subst: &GenericSubst,
    symbol_map: &mut FxHashMap<SymbolId, SymbolId>,
    tc: &mut TypeCheckResult,
    name_prefix: &str,
) -> Result<arandu_middle::hir::HirPatternId, Diagnostic> {
    let pat = hir.pool.pattern(pat_id).clone();
    let new_pat = clone_pattern(hir, &pat, subst, symbol_map, tc, name_prefix)?;
    Ok(hir.pool.alloc_pattern(new_pat))
}

fn clone_pattern(
    hir: &mut HirProgram,
    pat: &HirPattern,
    subst: &GenericSubst,
    symbol_map: &mut FxHashMap<SymbolId, SymbolId>,
    tc: &mut TypeCheckResult,
    name_prefix: &str,
) -> Result<HirPattern, Diagnostic> {
    Ok(match pat {
        HirPattern::Wildcard { span } => HirPattern::Wildcard { span: *span },
        HirPattern::Bind { span, name, symbol } => {
            let new_sym = fresh_symbol(
                tc,
                &format!("{name_prefix}$pat_{name}"),
                SymbolKind::Local,
                *span,
            );
            symbol_map.insert(*symbol, new_sym);
            // Type of binding comes from typeck at use; leave unbound in decl_types if unknown.
            if let Some(tid) = tc.type_info.decl_type_id(*symbol) {
                let new_ty = substitute_type_id(tid, subst, &tc.type_info.type_interner);
                tc.type_info_mut().record_decl_type(new_sym, new_ty);
            }
            HirPattern::Bind {
                span: *span,
                name: name.clone(),
                symbol: new_sym,
            }
        }
        HirPattern::Literal { span, expr } => HirPattern::Literal {
            span: *span,
            expr: clone_expr(hir, *expr, subst, symbol_map, tc, name_prefix)?,
        },
        HirPattern::Enum {
            span,
            type_symbol,
            variant,
            variant_symbol,
            payload,
        } => {
            let old_p: Vec<_> = hir.pool.pattern_list(*payload).to_vec();
            let mut new_p = Vec::with_capacity(old_p.len());
            for &p in &old_p {
                new_p.push(clone_pattern_id(
                    hir,
                    p,
                    subst,
                    symbol_map,
                    tc,
                    name_prefix,
                )?);
            }
            HirPattern::Enum {
                span: *span,
                type_symbol: *type_symbol,
                variant: variant.clone(),
                variant_symbol: *variant_symbol,
                payload: hir.pool.alloc_pattern_list(&new_p),
            }
        }
        HirPattern::TypeTuple {
            span,
            name,
            payload,
        } => {
            let old_p: Vec<_> = hir.pool.pattern_list(*payload).to_vec();
            let mut new_p = Vec::with_capacity(old_p.len());
            for &p in &old_p {
                new_p.push(clone_pattern_id(
                    hir,
                    p,
                    subst,
                    symbol_map,
                    tc,
                    name_prefix,
                )?);
            }
            HirPattern::TypeTuple {
                span: *span,
                name: name.clone(),
                payload: hir.pool.alloc_pattern_list(&new_p),
            }
        }
        HirPattern::Struct {
            span,
            struct_symbol,
            fields,
        } => {
            let old_f: Vec<_> = hir.pool.field_pattern_list(*fields).to_vec();
            let mut new_f = Vec::with_capacity(old_f.len());
            for &fid in &old_f {
                let fp = hir.pool.field_pattern(fid).clone();
                let pattern = fp
                    .pattern
                    .map(|p| clone_pattern_id(hir, p, subst, symbol_map, tc, name_prefix))
                    .transpose()?;
                new_f.push(
                    hir.pool
                        .alloc_field_pattern(arandu_middle::hir::HirFieldPattern {
                            span: fp.span,
                            name: fp.name.clone(),
                            pattern,
                        }),
                );
            }
            HirPattern::Struct {
                span: *span,
                struct_symbol: *struct_symbol,
                fields: hir.pool.alloc_field_pattern_list(&new_f),
            }
        }
        HirPattern::Tuple { span, items } => {
            let old_p: Vec<_> = hir.pool.pattern_list(*items).to_vec();
            let mut new_p = Vec::with_capacity(old_p.len());
            for &p in &old_p {
                new_p.push(clone_pattern_id(
                    hir,
                    p,
                    subst,
                    symbol_map,
                    tc,
                    name_prefix,
                )?);
            }
            HirPattern::Tuple {
                span: *span,
                items: hir.pool.alloc_pattern_list(&new_p),
            }
        }
        HirPattern::Range {
            span,
            start,
            inclusive,
            end,
        } => HirPattern::Range {
            span: *span,
            start: clone_expr(hir, *start, subst, symbol_map, tc, name_prefix)?,
            inclusive: *inclusive,
            end: clone_expr(hir, *end, subst, symbol_map, tc, name_prefix)?,
        },
    })
}

fn clone_condition(
    hir: &mut HirProgram,
    cond: &HirCondition,
    subst: &GenericSubst,
    symbol_map: &mut FxHashMap<SymbolId, SymbolId>,
    tc: &mut TypeCheckResult,
    name_prefix: &str,
) -> Result<HirCondition, Diagnostic> {
    Ok(match cond {
        HirCondition::Expr(e) => {
            HirCondition::Expr(clone_expr(hir, *e, subst, symbol_map, tc, name_prefix)?)
        }
        HirCondition::Is { expr, pattern } => HirCondition::Is {
            expr: clone_expr(hir, *expr, subst, symbol_map, tc, name_prefix)?,
            pattern: clone_pattern_id(hir, *pattern, subst, symbol_map, tc, name_prefix)?,
        },
    })
}

fn clone_place(
    hir: &mut HirProgram,
    place: &HirPlace,
    subst: &GenericSubst,
    symbol_map: &mut FxHashMap<SymbolId, SymbolId>,
    tc: &mut TypeCheckResult,
    name_prefix: &str,
) -> Result<HirPlace, Diagnostic> {
    let root = *symbol_map
        .get(&place.root_symbol)
        .unwrap_or(&place.root_symbol);
    let mut suffixes = place.suffixes.clone();
    suffixes.clear();
    for s in &place.suffixes {
        suffixes.push(match s {
            HirPlaceSuffix::Field {
                span,
                name,
                field_symbol,
                ty,
            } => HirPlaceSuffix::Field {
                span: *span,
                name: name.clone(),
                field_symbol: *field_symbol,
                ty: substitute_type_id(*ty, subst, &tc.type_info.type_interner),
            },
            HirPlaceSuffix::Index { span, expr, ty } => HirPlaceSuffix::Index {
                span: *span,
                expr: clone_expr(hir, *expr, subst, symbol_map, tc, name_prefix)?,
                ty: substitute_type_id(*ty, subst, &tc.type_info.type_interner),
            },
        });
    }
    Ok(HirPlace {
        root_symbol: root,
        suffixes,
        ty: substitute_type_id(place.ty, subst, &tc.type_info.type_interner),
        span: place.span,
    })
}

fn clone_expr(
    hir: &mut HirProgram,
    expr_id: HirExprId,
    subst: &GenericSubst,
    symbol_map: &mut FxHashMap<SymbolId, SymbolId>,
    tc: &mut TypeCheckResult,
    name_prefix: &str,
) -> Result<HirExprId, Diagnostic> {
    let expr = hir.pool.expr(expr_id).clone();
    let new_ty = substitute_type_id(expr.ty, subst, &tc.type_info.type_interner);
    let kind = clone_expr_kind(hir, &expr.kind, subst, symbol_map, tc, name_prefix)?;
    Ok(hir.pool.alloc_expr(HirExpr {
        kind,
        ty: new_ty,
        span: expr.span,
    }))
}

fn clone_expr_kind(
    hir: &mut HirProgram,
    kind: &HirExprKind,
    subst: &GenericSubst,
    symbol_map: &mut FxHashMap<SymbolId, SymbolId>,
    tc: &mut TypeCheckResult,
    name_prefix: &str,
) -> Result<HirExprKind, Diagnostic> {
    use HirExprKind::*;
    Ok(match kind {
        Path { symbol } => Path {
            symbol: *symbol_map.get(symbol).unwrap_or(symbol),
        },
        TypePath {
            type_symbol,
            member_symbol,
        } => TypePath {
            type_symbol: *type_symbol,
            member_symbol: *member_symbol,
        },
        Generic { callee, args } => Generic {
            callee: clone_expr(hir, *callee, subst, symbol_map, tc, name_prefix)?,
            args: args
                .iter()
                .map(|&a| substitute_type_id(a, subst, &tc.type_info.type_interner))
                .collect(),
        },
        Field { base, field } => Field {
            base: clone_expr(hir, *base, subst, symbol_map, tc, name_prefix)?,
            field: field.clone(),
        },
        SafeField { base, field } => SafeField {
            base: clone_expr(hir, *base, subst, symbol_map, tc, name_prefix)?,
            field: field.clone(),
        },
        Index { base, index } => Index {
            base: clone_expr(hir, *base, subst, symbol_map, tc, name_prefix)?,
            index: clone_expr(hir, *index, subst, symbol_map, tc, name_prefix)?,
        },
        SafeIndex { base, index } => SafeIndex {
            base: clone_expr(hir, *base, subst, symbol_map, tc, name_prefix)?,
            index: clone_expr(hir, *index, subst, symbol_map, tc, name_prefix)?,
        },
        Call {
            callee,
            args,
            trailing_block,
        } => {
            let old_args: Vec<_> = hir.pool.expr_list(*args).to_vec();
            let mut new_args = Vec::with_capacity(old_args.len());
            for &a in &old_args {
                new_args.push(clone_expr(hir, a, subst, symbol_map, tc, name_prefix)?);
            }
            Call {
                callee: clone_expr(hir, *callee, subst, symbol_map, tc, name_prefix)?,
                args: hir.pool.alloc_expr_list(&new_args),
                trailing_block: trailing_block
                    .map(|b| clone_block(hir, b, subst, symbol_map, tc, name_prefix))
                    .transpose()?,
            }
        }
        StructLiteral {
            struct_symbol,
            fields,
        } => {
            let old_f: Vec<_> = hir.pool.field_inits_list(*fields).to_vec();
            let mut new_f = Vec::with_capacity(old_f.len());
            for f in &old_f {
                new_f.push(arandu_middle::hir::HirFieldInit {
                    span: f.span,
                    name: f.name.clone(),
                    value: clone_expr(hir, f.value, subst, symbol_map, tc, name_prefix)?,
                });
            }
            StructLiteral {
                struct_symbol: *struct_symbol,
                fields: hir.pool.alloc_field_init_list(&new_f),
            }
        }
        Array { items } => {
            let old: Vec<_> = hir.pool.expr_list(*items).to_vec();
            let mut new_items = Vec::with_capacity(old.len());
            for &e in &old {
                new_items.push(clone_expr(hir, e, subst, symbol_map, tc, name_prefix)?);
            }
            Array {
                items: hir.pool.alloc_expr_list(&new_items),
            }
        }
        Lambda { params, body } => {
            let old_p: Vec<_> = hir.pool.lambda_params_list(*params).to_vec();
            let mut new_p = Vec::with_capacity(old_p.len());
            for p in &old_p {
                let new_ty = substitute_type_id(p.ty, subst, &tc.type_info.type_interner);
                let new_sym = fresh_symbol(
                    tc,
                    &format!("{name_prefix}$lam_{}", tc.symbols.get(p.symbol).name),
                    SymbolKind::Param,
                    p.span,
                );
                symbol_map.insert(p.symbol, new_sym);
                tc.type_info_mut().record_decl_type(new_sym, new_ty);
                new_p.push(HirLambdaParam {
                    span: p.span,
                    symbol: new_sym,
                    ty: new_ty,
                });
            }
            let body = match body {
                HirLambdaBody::Expr(e) => {
                    HirLambdaBody::Expr(clone_expr(hir, *e, subst, symbol_map, tc, name_prefix)?)
                }
                HirLambdaBody::Block(b) => {
                    HirLambdaBody::Block(clone_block(hir, *b, subst, symbol_map, tc, name_prefix)?)
                }
            };
            Lambda {
                params: hir.pool.alloc_lambda_param_list(&new_p),
                body,
            }
        }
        Alloc { expr } => Alloc {
            expr: clone_expr(hir, *expr, subst, symbol_map, tc, name_prefix)?,
        },
        AsyncBlock { block } => AsyncBlock {
            block: clone_block(hir, *block, subst, symbol_map, tc, name_prefix)?,
        },
        UnsafeBlock { block } => UnsafeBlock {
            block: clone_block(hir, *block, subst, symbol_map, tc, name_prefix)?,
        },
        If {
            condition,
            then_block,
            else_block,
        } => If {
            condition: clone_condition(hir, condition, subst, symbol_map, tc, name_prefix)?,
            then_block: clone_block(hir, *then_block, subst, symbol_map, tc, name_prefix)?,
            else_block: clone_block(hir, *else_block, subst, symbol_map, tc, name_prefix)?,
        },
        Match { value, arms } => {
            let value = clone_expr(hir, *value, subst, symbol_map, tc, name_prefix)?;
            let old_arms: Vec<_> = hir.pool.match_arms_list(*arms).to_vec();
            let mut new_arms = Vec::with_capacity(old_arms.len());
            for arm in &old_arms {
                new_arms.push(clone_match_arm(
                    hir,
                    arm,
                    subst,
                    symbol_map,
                    tc,
                    name_prefix,
                )?);
            }
            Match {
                value,
                arms: hir.pool.alloc_match_arm_list(&new_arms),
            }
        }
        Catch { expr, handler } => Catch {
            expr: clone_expr(hir, *expr, subst, symbol_map, tc, name_prefix)?,
            handler: clone_catch_handler(hir, handler, subst, symbol_map, tc, name_prefix)?,
        },
        NullCoalesce { left, right } => NullCoalesce {
            left: clone_expr(hir, *left, subst, symbol_map, tc, name_prefix)?,
            right: clone_expr(hir, *right, subst, symbol_map, tc, name_prefix)?,
        },
        Cast { expr, target_ty } => Cast {
            expr: clone_expr(hir, *expr, subst, symbol_map, tc, name_prefix)?,
            target_ty: substitute_type_id(*target_ty, subst, &tc.type_info.type_interner),
        },
        Unary { op, expr } => Unary {
            op: *op,
            expr: clone_expr(hir, *expr, subst, symbol_map, tc, name_prefix)?,
        },
        Binary { op, left, right } => Binary {
            op: *op,
            left: clone_expr(hir, *left, subst, symbol_map, tc, name_prefix)?,
            right: clone_expr(hir, *right, subst, symbol_map, tc, name_prefix)?,
        },
        Int(v) => Int(v.clone()),
        Float(v) => Float(v.clone()),
        Bool(v) => Bool(*v),
        Char(v) => Char(v.clone()),
        Str(v) => Str(v.clone()),
        StringInterp { parts } => {
            let mut new_parts = Vec::with_capacity(parts.len());
            for p in parts {
                new_parts.push(match p {
                    HirStringPart::Text(t) => HirStringPart::Text(t.clone()),
                    HirStringPart::Expr(e) => HirStringPart::Expr(clone_expr(
                        hir,
                        *e,
                        subst,
                        symbol_map,
                        tc,
                        name_prefix,
                    )?),
                });
            }
            StringInterp { parts: new_parts }
        }
        ToStr { value } => ToStr {
            value: clone_expr(hir, *value, subst, symbol_map, tc, name_prefix)?,
        },
        Nil => Nil,
        Error => Error,
        ResultCtor { variant, value } => ResultCtor {
            variant: *variant,
            value: clone_expr(hir, *value, subst, symbol_map, tc, name_prefix)?,
        },
        Try { expr } => Try {
            expr: clone_expr(hir, *expr, subst, symbol_map, tc, name_prefix)?,
        },
    })
}

fn clone_catch_handler(
    hir: &mut HirProgram,
    handler: &HirCatchHandler,
    subst: &GenericSubst,
    symbol_map: &mut FxHashMap<SymbolId, SymbolId>,
    tc: &mut TypeCheckResult,
    name_prefix: &str,
) -> Result<HirCatchHandler, Diagnostic> {
    Ok(match handler {
        HirCatchHandler::Expr(e) => {
            HirCatchHandler::Expr(clone_expr(hir, *e, subst, symbol_map, tc, name_prefix)?)
        }
        HirCatchHandler::Block {
            error_symbol,
            error_name,
            block,
        } => {
            let error_symbol = if let Some(sym) = error_symbol {
                let new_sym = fresh_symbol(
                    tc,
                    &format!("{name_prefix}$catch"),
                    SymbolKind::Local,
                    Span::new(0, 0, 0),
                );
                symbol_map.insert(*sym, new_sym);
                if let Some(tid) = tc.type_info.decl_type_id(*sym) {
                    let new_ty = substitute_type_id(tid, subst, &tc.type_info.type_interner);
                    tc.type_info_mut().record_decl_type(new_sym, new_ty);
                }
                Some(new_sym)
            } else {
                None
            };
            HirCatchHandler::Block {
                error_symbol,
                error_name: error_name.clone(),
                block: clone_block(hir, *block, subst, symbol_map, tc, name_prefix)?,
            }
        }
    })
}

fn fresh_symbol(tc: &mut TypeCheckResult, name: &str, kind: SymbolKind, span: Span) -> SymbolId {
    let mut candidate = name.to_string();
    let mut n = 0u32;
    let scope = tc.symbols.global_scope();
    loop {
        match tc.symbols_mut().define(scope, &candidate, kind, span) {
            Ok(id) => return id,
            Err(_) => {
                n += 1;
                candidate = format!("{name}__{n}");
            }
        }
    }
}

// ── Call-site rewrite ───────────────────────────────────────────────────────

fn rewrite_block_calls(
    hir: &mut HirProgram,
    block_id: HirBlockId,
    specialized: &FxHashMap<InstantiationKey, SymbolId>,
    tc: &TypeCheckResult,
) {
    let stmt_ids: Vec<_> = hir
        .pool
        .stmt_list(hir.pool.block(block_id).statements)
        .to_vec();
    for sid in stmt_ids {
        rewrite_stmt_calls(hir, sid, specialized, tc);
    }
}

fn rewrite_stmt_calls(
    hir: &mut HirProgram,
    stmt_id: arandu_middle::hir::HirStmtId,
    specialized: &FxHashMap<InstantiationKey, SymbolId>,
    tc: &TypeCheckResult,
) {
    let kind = hir.pool.stmt(stmt_id).kind.clone();
    match kind {
        HirStmtKind::VarDecl { value, .. }
        | HirStmtKind::Expr(value)
        | HirStmtKind::Free(value) => {
            rewrite_expr_calls(hir, value, specialized, tc);
        }
        HirStmtKind::Set { places, value, .. } => {
            rewrite_expr_calls(hir, value, specialized, tc);
            let index_exprs: Vec<_> = hir
                .pool
                .places_list(places)
                .iter()
                .flat_map(|p| p.suffixes.iter())
                .filter_map(|s| match s {
                    HirPlaceSuffix::Index { expr, .. } => Some(*expr),
                    _ => None,
                })
                .collect();
            for e in index_exprs {
                rewrite_expr_calls(hir, e, specialized, tc);
            }
        }
        HirStmtKind::Return { values } => {
            let es: Vec<_> = hir.pool.expr_list(values).to_vec();
            for e in es {
                rewrite_expr_calls(hir, e, specialized, tc);
            }
        }
        HirStmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            rewrite_condition_calls(hir, &condition, specialized, tc);
            rewrite_block_calls(hir, then_block, specialized, tc);
            if let Some(eb) = else_block {
                rewrite_block_calls(hir, eb, specialized, tc);
            }
        }
        HirStmtKind::While { condition, body } => {
            rewrite_condition_calls(hir, &condition, specialized, tc);
            rewrite_block_calls(hir, body, specialized, tc);
        }
        HirStmtKind::For { clause, body } => {
            match clause {
                HirForClause::In { iterable, .. } => {
                    rewrite_expr_calls(hir, iterable, specialized, tc);
                }
                HirForClause::CStyle {
                    condition,
                    init: _,
                    step: _,
                    ..
                } => {
                    if let Some(c) = condition {
                        rewrite_expr_calls(hir, c, specialized, tc);
                    }
                }
            }
            rewrite_block_calls(hir, body, specialized, tc);
        }
        HirStmtKind::Match { value, arms } => {
            rewrite_expr_calls(hir, value, specialized, tc);
            let arms_snap: Vec<_> = hir.pool.match_arms_list(arms).to_vec();
            for arm in arms_snap {
                if let Some(g) = arm.guard {
                    rewrite_expr_calls(hir, g, specialized, tc);
                }
                match arm.body {
                    HirMatchArmBody::Expr(e) => rewrite_expr_calls(hir, e, specialized, tc),
                    HirMatchArmBody::Block(b) => rewrite_block_calls(hir, b, specialized, tc),
                }
            }
        }
        HirStmtKind::Defer(b) | HirStmtKind::ErrDefer(b) | HirStmtKind::Unsafe(b) => {
            rewrite_block_calls(hir, b, specialized, tc);
        }
        HirStmtKind::Break | HirStmtKind::Continue | HirStmtKind::Error => {}
    }
}

fn rewrite_condition_calls(
    hir: &mut HirProgram,
    cond: &HirCondition,
    specialized: &FxHashMap<InstantiationKey, SymbolId>,
    tc: &TypeCheckResult,
) {
    match cond {
        HirCondition::Expr(e) => rewrite_expr_calls(hir, *e, specialized, tc),
        HirCondition::Is { expr, .. } => rewrite_expr_calls(hir, *expr, specialized, tc),
    }
}

fn rewrite_expr_calls(
    hir: &mut HirProgram,
    expr_id: HirExprId,
    specialized: &FxHashMap<InstantiationKey, SymbolId>,
    tc: &TypeCheckResult,
) {
    // First recurse into children, then rewrite this node if it is Call(Generic(...)).
    let kind = hir.pool.expr(expr_id).kind.clone();
    match &kind {
        HirExprKind::Generic { callee, .. }
        | HirExprKind::Field { base: callee, .. }
        | HirExprKind::SafeField { base: callee, .. }
        | HirExprKind::Alloc { expr: callee }
        | HirExprKind::Try { expr: callee }
        | HirExprKind::Cast { expr: callee, .. }
        | HirExprKind::Unary { expr: callee, .. }
        | HirExprKind::ToStr { value: callee }
        | HirExprKind::ResultCtor { value: callee, .. } => {
            rewrite_expr_calls(hir, *callee, specialized, tc);
        }
        HirExprKind::Index { base, index }
        | HirExprKind::SafeIndex { base, index }
        | HirExprKind::Binary {
            left: base,
            right: index,
            ..
        }
        | HirExprKind::NullCoalesce {
            left: base,
            right: index,
        } => {
            rewrite_expr_calls(hir, *base, specialized, tc);
            rewrite_expr_calls(hir, *index, specialized, tc);
        }
        HirExprKind::Call {
            callee,
            args,
            trailing_block,
        } => {
            rewrite_expr_calls(hir, *callee, specialized, tc);
            let args_snap: Vec<_> = hir.pool.expr_list(*args).to_vec();
            for a in args_snap {
                rewrite_expr_calls(hir, a, specialized, tc);
            }
            if let Some(b) = trailing_block {
                rewrite_block_calls(hir, *b, specialized, tc);
            }
            // Rewrite Call(Generic(Path(F), tys), …) → Call(Path(F_spec), …)
            try_rewrite_generic_call(hir, expr_id, *callee, specialized, tc);
        }
        HirExprKind::StructLiteral { fields, .. } => {
            let vals: Vec<_> = hir
                .pool
                .field_inits_list(*fields)
                .iter()
                .map(|f| f.value)
                .collect();
            for e in vals {
                rewrite_expr_calls(hir, e, specialized, tc);
            }
        }
        HirExprKind::Array { items } => {
            let es: Vec<_> = hir.pool.expr_list(*items).to_vec();
            for e in es {
                rewrite_expr_calls(hir, e, specialized, tc);
            }
        }
        HirExprKind::If {
            condition,
            then_block,
            else_block,
        } => {
            rewrite_condition_calls(hir, condition, specialized, tc);
            rewrite_block_calls(hir, *then_block, specialized, tc);
            rewrite_block_calls(hir, *else_block, specialized, tc);
        }
        HirExprKind::Match { value, arms } => {
            rewrite_expr_calls(hir, *value, specialized, tc);
            let arms_snap: Vec<_> = hir.pool.match_arms_list(*arms).to_vec();
            for arm in arms_snap {
                if let Some(g) = arm.guard {
                    rewrite_expr_calls(hir, g, specialized, tc);
                }
                match arm.body {
                    HirMatchArmBody::Expr(e) => rewrite_expr_calls(hir, e, specialized, tc),
                    HirMatchArmBody::Block(b) => rewrite_block_calls(hir, b, specialized, tc),
                }
            }
        }
        HirExprKind::Catch { expr, handler } => {
            rewrite_expr_calls(hir, *expr, specialized, tc);
            match handler {
                HirCatchHandler::Expr(e) => rewrite_expr_calls(hir, *e, specialized, tc),
                HirCatchHandler::Block { block, .. } => {
                    rewrite_block_calls(hir, *block, specialized, tc)
                }
            }
        }
        HirExprKind::Lambda { body, .. } => match body {
            HirLambdaBody::Expr(e) => rewrite_expr_calls(hir, *e, specialized, tc),
            HirLambdaBody::Block(b) => rewrite_block_calls(hir, *b, specialized, tc),
        },
        HirExprKind::AsyncBlock { block } | HirExprKind::UnsafeBlock { block } => {
            rewrite_block_calls(hir, *block, specialized, tc);
        }
        HirExprKind::StringInterp { parts } => {
            for p in parts {
                if let HirStringPart::Expr(e) = p {
                    rewrite_expr_calls(hir, *e, specialized, tc);
                }
            }
        }
        _ => {}
    }
}

fn try_rewrite_generic_call(
    hir: &mut HirProgram,
    call_expr_id: HirExprId,
    callee_id: HirExprId,
    specialized: &FxHashMap<InstantiationKey, SymbolId>,
    tc: &TypeCheckResult,
) {
    let HirExprKind::Generic {
        callee: inner_callee,
        args: type_args,
    } = hir.pool.expr(callee_id).kind.clone()
    else {
        return;
    };
    let symbol = match &hir.pool.expr(inner_callee).kind {
        HirExprKind::Path { symbol } => *symbol,
        HirExprKind::TypePath { member_symbol, .. } => *member_symbol,
        HirExprKind::Field { base, field } | HirExprKind::SafeField { base, field } => {
            let base_ty = tc.type_info.type_interner.resolve(hir.pool.expr(*base).ty);
            let actual = match base_ty {
                ArType::Nullable(inner) => tc.type_info.type_interner.resolve(inner),
                other => other,
            };
            let struct_id = match actual {
                ArType::Named(id, _) => Some(id),
                ArType::Ptr(inner) => match tc.type_info.type_interner.resolve(inner) {
                    ArType::Named(id, _) => Some(id),
                    _ => None,
                },
                _ => None,
            };
            let Some(struct_id) = struct_id else {
                return;
            };
            let struct_name = tc.symbols.get(struct_id).name.as_str();
            let Some(sym) = tc
                .symbols
                .lookup_associated_member(struct_name, field.as_str())
            else {
                return;
            };
            sym
        }
        _ => return,
    };
    let key = InstantiationKey { symbol, type_args };
    let Some(&spec_sym) = specialized.get(&key) else {
        return;
    };
    // Overwrite Generic → Path(specialized). Method calls already include receiver in args.
    let call_ty = hir.pool.expr(call_expr_id).ty;
    let path_ty = tc.type_info.decl_type_id(spec_sym).unwrap_or(call_ty);
    let span = hir.pool.expr(callee_id).span;
    *hir.pool.expr_mut(callee_id) = HirExpr {
        kind: HirExprKind::Path { symbol: spec_sym },
        ty: path_ty,
        span,
    };
}
