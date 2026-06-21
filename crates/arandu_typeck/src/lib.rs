pub mod type_checker;

pub use type_checker::{EnumPayloadShape, TypeCheckResult, TypeInfo, type_check};

pub use arandu_middle::{
    CodeReplacement, DiagCode, Diagnostic, Hint, Label, NodeKey, ResolutionResult, ResolvedNames,
    ScopeId, Severity, Span, SymbolId, SymbolKind, SymbolTable,
};

pub mod passes {
    pub use crate::type_checker;
}
