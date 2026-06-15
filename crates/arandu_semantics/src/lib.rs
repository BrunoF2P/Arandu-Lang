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

pub use arandu_middle::{
    amir, amir_validate, cfg, diagnostics, hir, layout, literal_pool, ops, resolved, symbol_table,
    types, BumpArena, BitSet, BitMatrix, DenseRange, GenerationalId, SlotMap, DenseSlotMap,
    StableHandle, SsoString, StringPool, StringId, VmReservation,
    validate_amir_program, DiagCode, Diagnostic, Label, Severity, Hint, CodeReplacement,
    DocCommentMap, NodeKey, ResolvedNames, ScopeId, Symbol, SymbolId, SymbolKind, SymbolTable, ResolutionResult,
    CodegenBackend, CompiledCode,
    arena, bitset, index_vec, stable_id, string_pool, vm, newtype_index,
};

pub use arandu_middle::ops::{BinaryOp, SetOp, UnaryOp};

pub mod passes;
pub mod parallel;

pub use arandu_resolve::resolve;
pub use arandu_typeck::{TypeCheckResult, TypeInfo, type_check};
pub use arandu_mir::{lower_to_amir, check_moves, optimize_amir};
pub use passes::lower_hir::lower_to_hir;
pub use parallel::compile_parallel;
