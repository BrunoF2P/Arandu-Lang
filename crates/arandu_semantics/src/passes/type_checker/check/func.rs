use arandu_parser::{FuncDecl, TypeExpr};

use super::super::TypeChecker;
use super::super::constraints::ConstraintOrigin;
use super::super::types::ArType;
use super::block::check_block;

fn validate_method_receiver(checker: &mut TypeChecker, decl: &FuncDecl) {
    let arandu_parser::FuncName::Method { receiver, span, .. } = &decl.name else {
        return;
    };
    let recv_ty = super::super::types::lower_type_expr(
        &TypeExpr::Named {
            span: receiver.span,
            name: receiver.clone(),
            args: Vec::new(),
        },
        &checker.symbols,
        checker.symbols.global_scope(),
        &checker.resolved,
    );
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
    let self_ty = super::super::types::lower_type_expr(
        &first.ty,
        &checker.symbols,
        checker.symbols.global_scope(),
        &checker.resolved,
    );
    if !super::super::types::unify(&recv_ty, &self_ty) {
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

fn func_type_scope(checker: &TypeChecker, decl: &FuncDecl) -> crate::ScopeId {
    if let Some(param) = decl.params.first() {
        let param_key = crate::NodeKey::from(param.span);
        if let Some(symbol_id) = checker.resolved.definitions.get(&param_key) {
            return checker.symbols.get(*symbol_id).scope;
        }
    }
    let func_key = crate::NodeKey::from(decl.span);
    if let Some(symbol_id) = checker.resolved.definitions.get(&func_key) {
        return checker.symbols.get(*symbol_id).scope;
    }
    checker.symbols.global_scope()
}

pub fn check_func_body(checker: &mut TypeChecker, decl: &FuncDecl) {
    if matches!(decl.name, arandu_parser::FuncName::Method { .. }) {
        validate_method_receiver(checker, decl);
    }

    let type_scope = func_type_scope(checker, decl);
    checker.type_scope_id = Some(type_scope);

    let (ret_ty, return_decl_span) = if let Some(result) = &decl.result {
        (
            super::super::types::lower_result_type(
                result,
                &checker.symbols,
                type_scope,
                &checker.resolved,
            ),
            super::super::types::result_type_decl_span(result),
        )
    } else {
        (ArType::Void, decl.span)
    };

    for param in &decl.params {
        let param_ty = super::super::types::lower_type_expr(
            &param.ty,
            &checker.symbols,
            type_scope,
            &checker.resolved,
        );

        let param_key = crate::NodeKey::from(param.span);
        if let Some(symbol_id) = checker.resolved.definitions.get(&param_key) {
            checker.ctx.bind(*symbol_id, param_ty.clone());
            checker.type_info.decl_types.insert(*symbol_id, param_ty);
        }
    }

    checker.ctx.push_return(ret_ty, return_decl_span);
    check_block(checker, &decl.body);
    checker.ctx.pop_return();
    checker.type_scope_id = None;
}
