pub mod amir;
pub mod amir_validate;
pub mod cfg;
pub mod codegen;
pub mod diagnostics;
pub mod hir;
pub mod layout;
pub mod literal_pool;
pub mod ops;
pub mod resolved;
pub mod symbol_table;
pub mod types;

pub use arandu_base::arena;
pub use arandu_base::bitset;
pub use arandu_base::index_vec;
pub use arandu_base::span::Span;
pub use arandu_base::stable_id;
pub use arandu_base::string_pool;
pub use arandu_base::vm;

pub use arandu_base::arena::BumpArena;
pub use arandu_base::bitset::{BitMatrix, BitSet};
pub use arandu_base::newtype_index;
pub use arandu_base::stable_id::{DenseSlotMap, GenerationalId, SlotMap, StableHandle};
pub use arandu_base::string_pool::{SsoString, StringId, StringPool};
pub use arandu_base::vm::VmReservation;
pub use layout::DenseRange;

pub use amir_validate::validate_amir_program;
pub use codegen::{CodegenBackend, CompiledCode};
pub use diagnostics::{CodeReplacement, DiagCode, Diagnostic, Hint, Label, Severity};
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
