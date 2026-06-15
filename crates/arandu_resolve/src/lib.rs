pub mod name_resolution;

pub use name_resolution::resolve;

pub use arandu_middle::{
    SymbolId, ScopeId, SymbolTable, ResolvedNames, NodeKey, Diagnostic, Severity, SymbolKind,
    DiagCode, Label, CodeReplacement, DocCommentMap, ResolutionResult, Symbol,
};
