use rustc_hash::FxHashMap;

use arandu_lexer::Span;
use arandu_parser::{FuncSignature, GenericParam, TypeName, WhereItem};

use super::unify;
use super::{ArType, LowerCtx};
use super::{GenericSubst, TypeInterner, build_subst, substitute_type};
use crate::passes::type_checker::TypeChecker;
use arandu_middle::types::lower::{lower_result_type_ctx, lower_type_expr_ctx};
use crate::{ScopeId, SymbolId, SymbolKind};

#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    /// Method name → function type **without** receiver (params are only explicit parameters).
    pub methods: Vec<(String, ArType)>,
}

/// Collect interface method signatures and per-type-parameter trait constraints.
pub fn collect_interfaces_and_constraints(
    checker: &mut TypeChecker,
    program: &arandu_parser::Program,
) {
    for decl_id in &program.decls {
        let decl = checker.pool.decl(*decl_id);
        use arandu_parser::TopLevelDecl;
        match decl {
            TopLevelDecl::Interface(iface) => collect_interface(checker, iface),
            TopLevelDecl::Struct(s) => {
                if let Some(sym) = checker
                    .resolved
                    .definitions
                    .get(&crate::NodeKey::from(s.span))
                {
                    let scope = checker.symbols.get(*sym).scope;
                    collect_decl_constraints(
                        checker,
                        &s.generic_params,
                        &s.where_clause,
                        s.span,
                        Some(*sym),
                        scope,
                    );
                }
            }
            TopLevelDecl::Enum(e) => {
                if let Some(sym) = checker
                    .resolved
                    .definitions
                    .get(&crate::NodeKey::from(e.span))
                {
                    let scope = checker.symbols.get(*sym).scope;
                    collect_decl_constraints(
                        checker,
                        &e.generic_params,
                        &e.where_clause,
                        e.span,
                        Some(*sym),
                        scope,
                    );
                }
            }
            TopLevelDecl::Func(f) => {
                let key = match &f.name {
                    arandu_parser::FuncName::Free { span, .. } => crate::NodeKey::from(*span),
                    arandu_parser::FuncName::Method { span, .. } => crate::NodeKey::from(*span),
                };
                if let Some(sym) = checker.resolved.definitions.get(&key) {
                    let scope = checker.symbols.get(*sym).scope;
                    collect_decl_constraints(
                        checker,
                        &f.generic_params,
                        &f.where_clause,
                        f.span,
                        Some(*sym),
                        scope,
                    );
                }
            }
            TopLevelDecl::TypeAlias(a) => {
                if let Some(sym) = checker
                    .resolved
                    .definitions
                    .get(&crate::NodeKey::from(a.span))
                {
                    let scope = checker.symbols.get(*sym).scope;
                    collect_decl_constraints(
                        checker,
                        &a.generic_params,
                        &[],
                        a.span,
                        Some(*sym),
                        scope,
                    );
                }
            }
            _ => {}
        }
    }
}

fn collect_interface(checker: &mut TypeChecker, decl: &arandu_parser::InterfaceDecl) {
    let Some(iface_sym) = checker
        .resolved
        .definitions
        .get(&crate::NodeKey::from(decl.span))
        .copied()
    else {
        return;
    };
    let iface_scope = checker.symbols.get(iface_sym).scope;
    let type_param_symbols = super::extract_generic_param_symbols(checker, &decl.generic_params);
    if !type_param_symbols.is_empty() {
        checker
            .type_info
            .generic_params
            .insert(iface_sym, type_param_symbols.clone());
    }

    let mut methods = Vec::new();
    for member in &decl.members {
        let sig_ty = lower_func_signature(checker, member, iface_scope);
        methods.push((member.name.clone(), sig_ty));
    }

    checker
        .type_info
        .interfaces
        .insert(iface_sym, InterfaceInfo { methods });

    collect_decl_constraints(
        checker,
        &decl.generic_params,
        &decl.where_clause,
        decl.span,
        Some(iface_sym),
        iface_scope,
    );
}

fn lower_func_signature(checker: &mut TypeChecker, sig: &FuncSignature, scope: ScopeId) -> ArType {
    let ctx = LowerCtx {
        pool: checker.pool,
        symbols: &checker.symbols,
        scope,
        resolved: &checker.resolved,
    };
    let mut param_types = Vec::new();
    for param in &sig.params {
        let ty = lower_type_expr_ctx(param.ty, &ctx, &mut checker.type_info.type_interner);
        param_types.push(checker.type_info.type_interner.intern(ty));
    }
    let ret = if let Some(result) = &sig.result {
        lower_result_type_ctx(result, &ctx, &mut checker.type_info.type_interner)
    } else {
        ArType::Void
    };
    let ret_id = checker.type_info.type_interner.intern(ret);
    ArType::Func(param_types, ret_id)
}

fn collect_decl_constraints(
    checker: &mut TypeChecker,
    generic_params: &[GenericParam],
    where_clause: &[WhereItem],
    decl_span: Span,
    decl_symbol: Option<SymbolId>,
    scope: ScopeId,
) {
    let param_symbols = if let Some(_decl_sym) = decl_symbol {
        super::extract_generic_param_symbols(checker, generic_params)
    } else {
        Vec::new()
    };

    if !param_symbols.is_empty()
        && let Some(decl_sym) = decl_symbol
    {
        checker
            .type_info
            .generic_params
            .entry(decl_sym)
            .or_insert_with(|| param_symbols.clone());
    }

    let name_to_sym: FxHashMap<String, SymbolId> = generic_params
        .iter()
        .zip(param_symbols.iter())
        .map(|(gp, sym)| (gp.name.clone(), *sym))
        .collect();

    for gp in generic_params {
        let Some(&param_sym) = name_to_sym.get(&gp.name) else {
            continue;
        };
        for constraint in &gp.constraints {
            if let Some(iface_sym) = resolve_interface_constraint(checker, constraint, scope) {
                checker
                    .type_info
                    .param_constraints
                    .entry(param_sym)
                    .or_default()
                    .push(iface_sym);
            }
        }
    }

    for item in where_clause {
        let Some(&param_sym) = name_to_sym.get(&item.name) else {
            checker.diagnostics.push(crate::Diagnostic::error(
                crate::DiagCode::T011GenericConstraintNotSatisfied,
                format!(
                    "where clause '{}' does not name a generic parameter of this declaration",
                    item.name
                ),
                item.span,
            ));
            continue;
        };
        for constraint in &item.constraints {
            if let Some(iface_sym) = resolve_interface_constraint(checker, constraint, scope) {
                checker
                    .type_info
                    .param_constraints
                    .entry(param_sym)
                    .or_default()
                    .push(iface_sym);
            }
        }
    }

    let _ = decl_span;
}

fn resolve_interface_constraint(
    checker: &mut TypeChecker,
    type_name: &TypeName,
    _scope: ScopeId,
) -> Option<SymbolId> {
    let key = crate::NodeKey::from(type_name.span);
    let Some(sym) = checker.resolved.type_refs.get(&key).copied() else {
        checker.diagnostics.push(crate::Diagnostic::error(
            crate::DiagCode::N002UndefinedType,
            format!("unknown constraint type '{}'", type_name.path.join(".")),
            type_name.span,
        ));
        return None;
    };
    match checker.symbols.get(sym).kind {
        SymbolKind::Interface => Some(sym),
        _ => {
            checker.diagnostics.push(crate::Diagnostic::error(
                crate::DiagCode::T011GenericConstraintNotSatisfied,
                format!(
                    "'{}' is not an interface and cannot be used as a type constraint",
                    type_name.path.join(".")
                ),
                type_name.span,
            ));
            None
        }
    }
}

/// After monomorphic instantiation, verify each type argument satisfies its constraints.
pub(crate) fn check_instantiation_constraints(
    checker: &mut TypeChecker,
    decl_symbol: SymbolId,
    param_symbols: &[SymbolId],
    arg_types: &[ArType],
    span: Span,
) {
    for (param_sym, arg_ty) in param_symbols.iter().zip(arg_types) {
        let constraints = checker.type_info.param_constraints.get(param_sym).cloned();
        let Some(constraints) = constraints else {
            continue;
        };
        for &iface_sym in &constraints {
            if !type_satisfies_interface(checker, arg_ty, iface_sym, span) {
                let iface_name = checker.symbols.get(iface_sym).name.clone();
                let ty_display = arg_ty.display(&checker.symbols, &checker.type_info.type_interner);
                let note = missing_methods_note(checker, arg_ty, iface_sym);
                let diag = crate::Diagnostic::error(
                    crate::DiagCode::T025InterfaceNotSatisfied,
                    format!("type '{ty_display}' does not satisfy interface '{iface_name}'"),
                    span,
                )
                .with_note(note);
                checker.diagnostics.push(diag);
            }
        }
    }
    let _ = decl_symbol;
}

fn missing_methods_note(
    checker: &mut TypeChecker,
    concrete: &ArType,
    iface_sym: SymbolId,
) -> String {
    let missing = missing_interface_methods(checker, concrete, iface_sym);
    if missing.is_empty() {
        "required method signatures are incompatible".to_string()
    } else {
        format!("missing or incompatible methods: {}", missing.join(", "))
    }
}

pub(crate) fn type_satisfies_interface(
    checker: &mut TypeChecker,
    concrete: &ArType,
    iface_sym: SymbolId,
    _span: Span,
) -> bool {
    let Some(iface) = checker.type_info.interfaces.get(&iface_sym) else {
        return false;
    };
    let Some(type_name) = concrete_type_name(checker, concrete) else {
        return false;
    };

    let iface_subst = interface_subst_for_concrete(checker, iface_sym, concrete);

    // We can iterate and borrow interner mutably inside the loop
    for (method, required) in &iface.methods {
        let required_inst =
            substitute_type(required, &iface_subst, &mut checker.type_info.type_interner);
        let Some(provided) = lookup_method_type(checker, &type_name, method) else {
            return false;
        };
        let provided = strip_receiver(provided);
        if !method_types_compatible(&required_inst, &provided, &checker.type_info.type_interner) {
            return false;
        }
    }
    true
}

#[cold]
fn missing_interface_methods(
    checker: &mut TypeChecker,
    concrete: &ArType,
    iface_sym: SymbolId,
) -> Vec<String> {
    let Some(iface) = checker.type_info.interfaces.get(&iface_sym) else {
        return vec!["<interface not collected>".to_string()];
    };
    let Some(type_name) = concrete_type_name(checker, concrete) else {
        return vec!["<non-nominal type>".to_string()];
    };

    let iface_subst = interface_subst_for_concrete(checker, iface_sym, concrete);

    let mut missing = Vec::new();
    for (method, required) in &iface.methods {
        let required_inst =
            substitute_type(required, &iface_subst, &mut checker.type_info.type_interner);
        let Some(provided) = lookup_method_type(checker, &type_name, method) else {
            let mut similar = Vec::new();
            if let Some(methods) = checker.symbols.associated_members.get(&type_name) {
                let max_distance = if method.len() <= 4 { 2 } else { 3 };
                for prov_name in methods.keys() {
                    let dist = if prov_name.to_lowercase() == method.to_lowercase() {
                        0
                    } else {
                        strsim::levenshtein(method, prov_name)
                    };
                    if dist <= max_distance {
                        similar.push(prov_name.clone());
                    }
                }
            }
            if !similar.is_empty() {
                missing.push(format!(
                    "{method} (did you mean `{}`?)",
                    similar.join("`, `")
                ));
            } else {
                missing.push(method.to_string());
            }
            continue;
        };
        let provided = strip_receiver(provided);
        if !method_types_compatible(&required_inst, &provided, &checker.type_info.type_interner) {
            missing.push(format!("{method} (signature mismatch)"));
        }
    }
    missing
}

fn concrete_type_name(checker: &TypeChecker, ty: &ArType) -> Option<String> {
    match ty {
        ArType::Named(id, _) => Some(checker.symbols.get(*id).name.clone()),
        _ => None,
    }
}

fn interface_subst_for_concrete(
    checker: &TypeChecker,
    iface_sym: SymbolId,
    concrete: &ArType,
) -> GenericSubst {
    let Some(iface_params) = checker.type_info.generic_params.get(&iface_sym) else {
        return GenericSubst::default();
    };
    if iface_params.is_empty() {
        return GenericSubst::default();
    }
    if let ArType::Named(_, args) = concrete
        && args.len() == iface_params.len()
    {
        let resolved_args: Vec<ArType> = args
            .iter()
            .map(|&a| checker.type_info.type_interner.resolve(a).clone())
            .collect();
        return build_subst(iface_params, &resolved_args);
    }
    if iface_params.len() == 1 {
        return build_subst(iface_params, std::slice::from_ref(concrete));
    }
    GenericSubst::default()
}

fn lookup_method_type(checker: &TypeChecker, type_name: &str, method: &str) -> Option<ArType> {
    let sym = checker
        .symbols
        .lookup_associated_member(type_name, method)?;
    checker.decl_type(sym)
}

fn strip_receiver(ty: ArType) -> ArType {
    if let ArType::Func(params, ret) = ty {
        if !params.is_empty() {
            return ArType::Func(params[1..].to_vec(), ret);
        }
        return ArType::Func(params, ret);
    }
    ty
}

fn method_types_compatible(required: &ArType, provided: &ArType, interner: &TypeInterner) -> bool {
    match (required, provided) {
        (ArType::Func(req_params, req_ret), ArType::Func(prov_params, prov_ret)) => {
            if req_params.len() != prov_params.len() {
                return false;
            }
            req_params.iter().zip(prov_params.iter()).all(|(&a, &b)| {
                if a == b {
                    return true;
                }
                let ty_a = interner.resolve(a);
                let ty_b = interner.resolve(b);
                unify(ty_a, ty_b, interner)
            }) && (*req_ret == *prov_ret || {
                let ty_a = interner.resolve(*req_ret);
                let ty_b = interner.resolve(*prov_ret);
                unify(ty_a, ty_b, interner)
            })
        }
        _ => unify(required, provided, interner),
    }
}
