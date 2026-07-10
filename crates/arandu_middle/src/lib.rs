//! Shared compiler types and intermediate representations for Arandu.
//!
//! This crate is the central dependency hub. It owns:
//! - **AMIR** (`amir`): the Arandu Mid-level IR (SSA-like, typed basic blocks).
//! - **HIR** (`hir`): the High-level IR produced by the lowering pass.
//! - **Type system** (`types`): [`ArType`], [`TypeInterner`], and primitives.
//! - **Layout engine** (`layout`): struct/enum memory layout computation.
//! - **Symbol table** (`symbol_table`): scoped identifier registry.
//! - **Diagnostics** (`diagnostics`): re-exported from `arandu_diagnostics`.
//! - **Parse cache** and **stdlib path cache**: incremental compilation helpers.
//!
//! Re-exports from `arandu_base` (bitset, index_vec, span, etc.)
//! are provided so downstream crates need only depend on this crate.

pub mod amir;
pub mod amir_validate;
pub mod cfg;
pub mod codegen;
pub mod db;
pub mod diagnostics;
pub mod hir;
pub mod layout;
pub mod literal_pool;
pub mod ops;
pub mod resolved;
pub mod session;
pub mod symbol_table;
pub mod types;

pub use session::CompileSession;

pub use arandu_base::bitset;
pub use arandu_base::index_vec;
pub use arandu_base::span::Span;

pub use arandu_base::bitset::{BitMatrix, BitSet};
pub use arandu_base::newtype_index;
pub use layout::{
    DataLayout, DenseRange, EnumPayloadShape, LayoutEngine, SizeAlign, StructLayoutProvider,
    TypeLayout,
};

pub use amir_validate::validate_amir_program;
pub use codegen::{CodegenBackend, CompiledCode, JitError};
pub use diagnostics::{CodeReplacement, DiagCode, Diagnostic, Hint, Label, Severity};
pub use resolved::{DocCommentMap, NodeKey, ResolvedNames};
pub use symbol_table::{ScopeId, Symbol, SymbolId, SymbolKind, SymbolTable};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExportedSymbolTable {
    pub symbols: std::collections::BTreeMap<String, (SymbolId, SymbolKind)>,
}

#[derive(Debug, Clone)]
pub struct ResolutionResult {
    pub symbols: SymbolTable,
    pub resolved: ResolvedNames,
    pub docs: DocCommentMap,
    pub diagnostics: Vec<Diagnostic>,
    pub is_cycle_fallback: bool,
}

impl ResolutionResult {
    #[must_use]
    pub fn cycle_fallback() -> Self {
        Self {
            symbols: SymbolTable::new(0),
            resolved: ResolvedNames::default(),
            docs: DocCommentMap::default(),
            diagnostics: Vec::new(),
            is_cycle_fallback: true,
        }
    }

    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }
}
