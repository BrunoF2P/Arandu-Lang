//! Name resolution pass for Arandu.
//!
//! Resolves identifiers to [`SymbolId`]s, builds the [`SymbolTable`], and
//! populates [`ResolvedNames`] (the mapping from AST nodes to their symbols).
//! Entry points: [`resolve`], [`resolve_with_symbols`].

pub mod name_resolution;

pub use name_resolution::{resolve_for_test, resolve_imports_and_bodies, resolve_local};

pub use arandu_middle::{
    CodeReplacement, DiagCode, Diagnostic, DocCommentMap, Label, NodeKey, ResolutionResult,
    ResolvedNames, ScopeId, Severity, Symbol, SymbolId, SymbolKind, SymbolTable,
};
