pub mod type_checker;

pub use type_checker::{TypeCheckResult, TypeInfo, type_check, EnumPayloadShape};

pub use arandu_middle::{
    SymbolId, ScopeId, SymbolTable, ResolvedNames, NodeKey, Diagnostic, Severity, SymbolKind,
    DiagCode, Label, CodeReplacement, Span, Hint, ResolutionResult,
};

pub mod passes {
    pub use crate::type_checker;
}
