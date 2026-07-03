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
    BitMatrix, BitSet, BumpArena, CodeReplacement, CodegenBackend, CompileSession, CompiledCode,
    DenseRange, DenseSlotMap, DiagCode, Diagnostic, DocCommentMap, GenerationalId, Hint, Label,
    NodeKey, ResolutionResult, ResolvedNames, ScopeId, Severity, SlotMap, SsoString, StableHandle,
    StringId, StringPool, Symbol, SymbolId, SymbolKind, SymbolTable, VmReservation, amir,
    amir_validate, arena, bitset, cfg, diagnostics, hir, index_vec, layout, literal_pool,
    newtype_index, ops, resolved, stable_id, string_pool, symbol_table, types, validate_amir_program,
    vm,
};

pub use arandu_middle::ops::{BinaryOp, SetOp, UnaryOp};

pub mod parallel;
pub mod passes;

pub use arandu_mir::{check_moves, lower_to_amir, optimize_amir};
pub use arandu_resolve::{resolve, resolve_with_cache};
pub use arandu_typeck::{SessionMode, TypeCheckResult, TypeInfo, type_check, type_check_with_session};
pub use parallel::compile_parallel;
pub use passes::lower_hir::lower_to_hir;
