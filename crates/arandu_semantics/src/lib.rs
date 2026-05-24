#![allow(clippy::result_large_err)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::format_push_string,
    clippy::match_same_arms,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::needless_pass_by_value,
    clippy::return_self_not_must_use,
    clippy::trivially_copy_pass_by_ref,
    clippy::unused_self,
    clippy::used_underscore_binding
)]

pub mod amir;
mod amir_validate;
mod cfg;
mod diagnostics;
pub mod hir;
pub mod index_vec;
pub mod literal_pool;
pub mod ops;
pub mod passes;
pub use ops::{BinaryOp, SetOp, UnaryOp};
mod resolved;
mod symbol_table;

pub use amir_validate::validate_amir_program;
pub use diagnostics::{DiagCode, Diagnostic, Label, Severity};
pub use passes::lower_amir::lower_to_amir;
pub use passes::lower_hir::lower_to_hir;
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
