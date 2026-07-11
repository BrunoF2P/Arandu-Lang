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
use arandu_middle::hir::{HirDecl, HirFunc, HirParam, HirProgram};
use arandu_middle::symbol_table::{SymbolId, SymbolKind};
use arandu_middle::types::{ArType, TypeId, build_subst_ids, substitute_type_id};
use arandu_typeck::TypeCheckResult;
use rustc_hash::FxHashMap;

use super::graph::{InstantiationGraph, InstantiationKey};

mod clone;
mod rewrite;

use clone::clone_block;
use rewrite::rewrite_block_calls;

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
        is_async: template.is_async,
        no_fallback: template.no_fallback,
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
            is_async: self.is_async,
            no_fallback: self.no_fallback,
        }
    }
}

