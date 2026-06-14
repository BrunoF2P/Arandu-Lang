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
pub use arandu_base::arena;
pub use arandu_base::bitset;
mod cfg;
mod diagnostics;
pub mod hir;
pub use arandu_base::index_vec;
pub mod layout;
pub mod literal_pool;
pub mod ops;
pub mod passes;
pub use arandu_base::stable_id;
pub use arandu_base::string_pool;
pub use arandu_base::vm;
pub mod parallel;

pub use ops::{BinaryOp, SetOp, UnaryOp};
mod resolved;
mod symbol_table;

pub use arandu_base::arena::BumpArena;
pub use arandu_base::bitset::{BitSet, BitMatrix};
pub use layout::DenseRange;
pub use arandu_base::stable_id::{GenerationalId, SlotMap, DenseSlotMap, StableHandle};
pub use arandu_base::newtype_index;
pub use arandu_base::string_pool::{SsoString, StringPool, StringId};
pub use arandu_base::vm::VmReservation;

pub use amir_validate::validate_amir_program;
pub use diagnostics::{DiagCode, Diagnostic, Label, Severity, Hint, CodeReplacement};
pub use passes::lower_amir::lower_to_amir;
pub use passes::lower_hir::lower_to_hir;
pub use passes::move_checker::check_moves;
pub use passes::name_resolution::resolve;
pub use passes::optimize::optimize_amir;
pub use passes::type_checker::{TypeCheckResult, TypeInfo, type_check};
pub use resolved::{DocCommentMap, NodeKey, ResolvedNames};
pub use symbol_table::{ScopeId, Symbol, SymbolId, SymbolKind, SymbolTable};
pub use parallel::compile_parallel;


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
