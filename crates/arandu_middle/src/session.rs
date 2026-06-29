//! Compilation Session — Centralized Compilation Context
//!
//! `CompileSession` owns top-level compilation resources such as the `TypeInterner`.
//! Passing `&CompileSession` or `&mut CompileSession` explicitly down compiler passes
//! replaces thread-local global state and prepares the architecture for incremental
//! query-based compilation (Salsa).

use crate::types::TypeInterner;

/// Represents an active compilation unit session, containing shared infrastructure.
#[derive(Debug, Default)]
pub struct CompileSession {
    /// Canonical type interner for type identity deduplication.
    pub type_interner: TypeInterner,
}

impl CompileSession {
    /// Creates a new empty compilation session.
    #[must_use]
    pub fn new() -> Self {
        Self {
            type_interner: TypeInterner::new(),
        }
    }
}
