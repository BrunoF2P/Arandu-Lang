pub mod name_resolution;

pub use name_resolution::resolve;

pub use arandu_middle::{
    CodeReplacement, DiagCode, Diagnostic, DocCommentMap, Label, NodeKey, ResolutionResult,
    ResolvedNames, ScopeId, Severity, Symbol, SymbolId, SymbolKind, SymbolTable,
};
