pub mod type_checker;

pub use type_checker::{
    EnumPayloadShape, TypeCheckResult, TypeChecker, TypeInfo, check::check_bodies,
    check::check_signatures, check_bodies_only, check_signatures_only, type_check,
};

pub use arandu_middle::{
    CodeReplacement, DiagCode, Diagnostic, Hint, Label, NodeKey, ResolutionResult, ResolvedNames,
    ScopeId, Severity, Span, SymbolId, SymbolKind, SymbolTable,
};

pub mod passes {
    pub use crate::type_checker;
}
