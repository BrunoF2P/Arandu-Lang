use arandu_parser::{ResultType, TypeExpr, TypeName};

use crate::{DiagCode, Diagnostic, ScopeId};

use super::Resolver;

impl<'a> Resolver<'a> {
    pub(crate) fn resolve_result_type(&mut self, scope: ScopeId, result: &ResultType) {
        match result {
            ResultType::Single { ty, .. } => self.resolve_type_expr(scope, ty),
            ResultType::Multi { types, .. } => {
                for ty in types {
                    self.resolve_type_expr(scope, ty);
                }
            }
        }
    }

    pub(crate) fn resolve_type_expr(&mut self, scope: ScopeId, ty: &TypeExpr) {
        match ty {
            TypeExpr::Primitive { .. } => {}
            TypeExpr::Named { name, args, .. } => {
                self.resolve_type_name(scope, name);
                for arg in args {
                    self.resolve_type_expr(scope, arg);
                }
            }
            TypeExpr::Nullable { inner, .. }
            | TypeExpr::Pointer { inner, .. }
            | TypeExpr::Slice { inner, .. }
            | TypeExpr::Group { inner, .. } => self.resolve_type_expr(scope, inner),
            TypeExpr::Array { elem, .. } => self.resolve_type_expr(scope, elem),
            TypeExpr::Func { params, result, .. } => {
                for param in params {
                    self.resolve_type_expr(scope, param);
                }
                if let Some(result) = result {
                    self.resolve_result_type(scope, result);
                }
            }
        }
    }

    pub(crate) fn resolve_type_name(&mut self, scope: ScopeId, name: &TypeName) -> bool {
        let Some(root) = name.path.first() else {
            return false;
        };
        if name.path.len() > 1 && self.symbols.lookup_module(scope, root).is_some() {
            let member = &name.path[1];
            if let Some(symbol) = self.symbols.lookup_module_member(root, member) {
                self.resolved.type_ref(name.span, symbol);
                return true;
            }
            self.diagnostics.push(Diagnostic::error(
                DiagCode::M002UndefinedNamespaceMember,
                format!("namespace member '{root}.{member}' is not declared"),
                name.span,
            ));
            return false;
        }
        if let Some(symbol) = self.symbols.lookup_type(scope, root) {
            self.resolved.type_ref(name.span, symbol);
            return true;
        }
        if let Some(symbol) = self.symbols.lookup_any(scope, root)
            && self.symbols.get(symbol).kind.is_value()
        {
            self.diagnostics.push(Diagnostic::error(
                DiagCode::N005ValueUsedAsType,
                format!("value '{root}' cannot be used as a type"),
                name.span,
            ));
            return false;
        }
        if matches!(root.as_str(), "Result" | "Option" | "void") {
            return true;
        }
        let mut diagnostic = Diagnostic::error(
            DiagCode::N002UndefinedType,
            format!("type '{root}' is not declared"),
            name.span,
        );
        if let Some(suggestion) = self.suggest_type(scope, root) {
            diagnostic = diagnostic.with_hint(format!("did you mean '{suggestion}'?"));
        }
        self.diagnostics.push(diagnostic);
        false
    }
}
