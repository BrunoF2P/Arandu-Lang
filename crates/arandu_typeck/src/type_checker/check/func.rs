use arandu_parser::FuncDecl;

use super::super::TypeChecker;
use super::super::constraints::ConstraintOrigin;
use super::super::types::ArType;
use super::block::check_block;

fn func_name_key(decl: &FuncDecl) -> crate::NodeKey {
    let name_span = match &decl.name {
        arandu_parser::FuncName::Free { span, .. } => *span,
        arandu_parser::FuncName::Method { span, .. } => *span,
    };
    crate::NodeKey::from(name_span)
}

fn validate_method_receiver(checker: &mut TypeChecker<'_>, decl: &FuncDecl) {
    let arandu_parser::FuncName::Method { receiver, span, .. } = &decl.name else {
        return;
    };
    let mut recv_ty =
        checker.lower_named_type(receiver.span, receiver, &[], checker.symbols.global_scope());
    if let ArType::Named(struct_id, ref args) = recv_ty
        && args.is_empty()
    {
        let func_key = func_name_key(decl);
        if let Some(method_sym) = checker.resolved.definitions.get(&func_key).copied()
            && let Some(method_params) = checker.type_info.generic_params.get(&method_sym).cloned()
        {
            let mut new_args = Vec::new();
            for &param_sym in &method_params {
                let arg_ty = ArType::Named(param_sym, vec![]);
                new_args.push(checker.intern(arg_ty));
            }
            recv_ty = ArType::Named(struct_id, new_args);
        }
    }
    let Some(first) = decl.params.first() else {
        checker.diagnostics.push(crate::Diagnostic::error(
            crate::DiagCode::T021MethodSelfRequired,
            "method must declare a receiver parameter `self`",
            *span,
        ));
        return;
    };
    if !first.is_receiver {
        checker.diagnostics.push(crate::Diagnostic::error(
            crate::DiagCode::T021MethodSelfRequired,
            "first parameter of a method must be `self`",
            first.span,
        ));
        return;
    }
    if first.ownership.is_none() {
        checker.diagnostics.push(crate::Diagnostic::error(
            crate::DiagCode::T021MethodSelfRequired,
            "receiver `self` requires an ownership qualifier (`shared`, `mut`, or `own`)",
            first.span,
        ));
    }
    let mut self_ty = checker.lower_type_expr(first.ty, checker.symbols.global_scope());
    if let ArType::Named(struct_id, ref args) = self_ty
        && args.is_empty()
    {
        let func_key = func_name_key(decl);
        if let Some(method_sym) = checker.resolved.definitions.get(&func_key).copied()
            && let Some(method_params) = checker.type_info.generic_params.get(&method_sym).cloned()
        {
            let mut new_args = Vec::new();
            for &param_sym in &method_params {
                let arg_ty = ArType::Named(param_sym, vec![]);
                new_args.push(checker.intern(arg_ty));
            }
            self_ty = ArType::Named(struct_id, new_args);
        }
    }
    if !super::super::types::unify(&recv_ty, &self_ty, &checker.type_info.type_interner) {
        checker.add_constraint(
            recv_ty,
            self_ty,
            ConstraintOrigin::Assignment {
                lhs_span: first.span,
                rhs_span: receiver.span,
            },
        );
    }
}

fn func_type_scope(checker: &TypeChecker<'_>, decl: &FuncDecl) -> crate::ScopeId {
    if let Some(param) = decl.params.first() {
        let param_key = crate::NodeKey::from(param.span);
        if let Some(symbol_id) = checker.resolved.definitions.get(&param_key) {
            return checker.symbols.get(*symbol_id).scope;
        }
    }
    let func_key = func_name_key(decl);
    if let Some(symbol_id) = checker.resolved.definitions.get(&func_key) {
        return checker.symbols.get(*symbol_id).scope;
    }
    checker.symbols.global_scope()
}

pub fn check_func_body(checker: &mut TypeChecker<'_>, decl: &FuncDecl) {
    if matches!(decl.name, arandu_parser::FuncName::Method { .. }) {
        validate_method_receiver(checker, decl);
    }

    let type_scope = func_type_scope(checker, decl);
    checker.type_scope_id = Some(type_scope);

    let (ret_ty, return_decl_span) = if let Some(result) = &decl.result {
        (
            checker.lower_result_type(result, type_scope),
            super::super::types::result_type_decl_span(result),
        )
    } else {
        (ArType::Void, decl.span)
    };

    for param in &decl.params {
        let mut param_ty = checker.lower_type_expr(param.ty, type_scope);

        if param.is_receiver
            && let ArType::Named(struct_id, ref args) = param_ty
            && args.is_empty()
        {
            let func_key = func_name_key(decl);
            if let Some(method_sym) = checker.resolved.definitions.get(&func_key).copied()
                && let Some(method_params) =
                    checker.type_info.generic_params.get(&method_sym).cloned()
            {
                let mut new_args = Vec::new();
                for &param_sym in &method_params {
                    let arg_ty = ArType::Named(param_sym, vec![]);
                    new_args.push(checker.intern(arg_ty));
                }
                param_ty = ArType::Named(struct_id, new_args);
            }
        }

        let param_ty_id = checker.intern(param_ty);
        let param_key = crate::NodeKey::from(param.span);
        if let Some(&symbol_id) = checker.resolved.definitions.get(&param_key) {
            checker.ctx.bind(symbol_id, param_ty_id);
            checker.record_decl_type(symbol_id, param_ty_id);
        }
    }

    let ret_id = checker.intern(ret_ty);
    checker.ctx.push_return(ret_id, return_decl_span);
    check_block(checker, checker.pool, &decl.body);
    checker.ctx.pop_return();
    checker.type_scope_id = None;
}
