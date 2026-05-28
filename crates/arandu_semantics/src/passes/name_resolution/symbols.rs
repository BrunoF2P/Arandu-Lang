use arandu_lexer::Span;
use arandu_parser::ast_pool::ExprId;

use crate::{DiagCode, Diagnostic, ScopeId, SymbolKind};

use super::Resolver;

impl<'a> Resolver<'a> {
    pub(crate) fn resolve_value_name(
        &mut self,
        scope: ScopeId,
        name: &str,
        expr: ExprId,
        span: Span,
    ) {
        if let Some(symbol) = self.symbols.lookup_value(scope, name) {
            self.resolved.expr_ref(expr, symbol);
            return;
        }
        if self.symbols.lookup_module(scope, name).is_some() {
            self.diagnostics.push(Diagnostic::error(
                DiagCode::N008NamespaceUsedAsValue,
                format!("namespace '{name}' cannot be used as a value"),
                span,
            ));
            return;
        }
        if let Some(symbol) = self.symbols.lookup_any(scope, name)
            && self.symbols.get(symbol).kind.is_type()
        {
            self.diagnostics.push(Diagnostic::error(
                DiagCode::N004TypeUsedAsValue,
                format!("type '{name}' cannot be used as a value"),
                span,
            ));
            return;
        }
        let mut diagnostic = Diagnostic::error(
            DiagCode::N001UndefinedValue,
            format!("value '{name}' is not declared"),
            span,
        );
        if let Some(suggestion) = self.suggest_value(scope, name) {
            diagnostic = diagnostic.with_hint(format!("did you mean '{suggestion}'?"));
        }
        self.diagnostics.push(diagnostic);
    }

    pub(crate) fn resolve_assignment_target(&mut self, scope: ScopeId, name: &str, span: Span) {
        if let Some(symbol) = self.symbols.lookup_value(scope, name) {
            self.resolved.value_ref(span, symbol);
            return;
        }
        let diagnostic = Diagnostic::error(
            DiagCode::N007UndefinedAssignmentTarget,
            format!("assignment target '{name}' is not declared"),
            span,
        )
        .with_hint(format!(
            "declare it first with `{name} = ...`, then mutate with `set {name} = ...`"
        ));
        self.diagnostics.push(diagnostic);
    }

    pub(crate) fn is_namespace(&self, scope: ScopeId, name: &str) -> bool {
        if self.symbols.lookup_module(scope, name).is_some() {
            return true;
        }
        self.symbols.lookup_value(scope, name).is_some_and(|id| {
            matches!(
                self.symbols.get(id).kind,
                SymbolKind::ImportValue | SymbolKind::Module
            )
        })
    }

    pub(crate) fn resolve_namespace_member(
        &mut self,
        scope: ScopeId,
        namespace: &str,
        member: &str,
        expr: ExprId,
        span: Span,
    ) -> bool {
        if !self.is_namespace(scope, namespace) {
            return false;
        }
        if let Some(symbol) = self.symbols.lookup_module_member(namespace, member) {
            self.resolved.expr_ref(expr, symbol);
        } else {
            self.diagnostics.push(Diagnostic::error(
                DiagCode::N009UndefinedNamespaceMember,
                format!("namespace member '{namespace}.{member}' is not declared"),
                span,
            ));
        }
        true
    }

    pub(crate) fn define(&mut self, scope: ScopeId, name: &str, kind: SymbolKind, span: Span) {
        match self.symbols.define(scope, name, kind, span) {
            Ok(symbol) => self.resolved.define(span, symbol),
            Err(previous) => {
                let previous_symbol = self.symbols.get(previous);
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagCode::N003RedefinedName,
                        format!("name '{name}' is already declared in this scope"),
                        span,
                    )
                    .with_label(previous_symbol.span, "previous declaration is here"),
                );
            }
        }
    }

    pub(crate) fn suggest_value(&self, scope: ScopeId, name: &str) -> Option<String> {
        self.suggest_from(name, self.symbols.value_candidates(scope))
    }

    pub(crate) fn suggest_type(&self, scope: ScopeId, name: &str) -> Option<String> {
        self.suggest_from(name, self.symbols.type_candidates(scope))
    }

    pub(crate) fn suggest_from(
        &self,
        name: &str,
        candidates: impl IntoIterator<Item = &'a crate::Symbol>,
    ) -> Option<String> {
        let max_distance = if name.len() <= 4 { 2 } else { 3 };

        candidates
            .into_iter()
            .map(|symbol| (symbol, strsim::levenshtein(name, &symbol.name)))
            .filter(|(_, dist)| *dist <= max_distance)
            .min_by_key(|(_, dist)| *dist)
            .map(|(symbol, _)| symbol.name.clone())
    }
}
