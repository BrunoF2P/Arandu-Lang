mod diagnostics;
pub mod passes;
mod resolved;
mod symbol_table;

pub use diagnostics::{DiagCode, Diagnostic, Label, Severity};
pub use passes::name_resolution::resolve;
pub use passes::type_checker::{TypeCheckResult, TypeInfo, type_check};
pub use resolved::{DocCommentMap, NodeKey, ResolvedNames};
pub use symbol_table::{ScopeId, Symbol, SymbolId, SymbolKind, SymbolTable};

#[derive(Debug, Clone)]
pub struct ResolutionResult {
    pub symbols: SymbolTable,
    pub resolved: ResolvedNames,
    pub docs: DocCommentMap,
    pub diagnostics: Vec<Diagnostic>,
}

impl ResolutionResult {
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }
}
