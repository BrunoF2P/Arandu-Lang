pub(crate) mod dce;
pub mod definite_init;
pub mod liveness;
pub mod lower_amir;
pub mod move_checker;
pub mod optimize;
pub(crate) mod sccp;
pub(crate) mod simplify_cfg;

pub use lower_amir::lower_to_amir;
pub use move_checker::check_moves;
pub use optimize::optimize_amir;

pub use arandu_middle::{
    BitMatrix, BitSet, CodeReplacement, DiagCode, Diagnostic, DocCommentMap, Label, NodeKey,
    ResolvedNames, ScopeId, Severity, Span, SymbolId, SymbolKind, SymbolTable, amir, amir_validate,
    cfg, diagnostics, hir, layout, literal_pool, ops, types,
};

pub use arandu_typeck::TypeCheckResult;

pub mod passes {
    pub mod type_checker {
        pub use arandu_middle::types;
        pub use arandu_typeck::EnumPayloadShape;
    }
}
