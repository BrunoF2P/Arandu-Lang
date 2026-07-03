//! Compilation Session — Centralized Compilation Context
//!
//! `CompileSession` owns top-level compilation resources such as the `TypeInterner`.
//! Passing `&CompileSession` or `&mut CompileSession` explicitly down compiler passes
//! replaces thread-local global state and prepares the architecture for incremental
//! query-based compilation (Salsa).

use crate::parse_cache::ParseCache;
use crate::stdlib_cache::StdlibPathCache;
use crate::types::TypeInterner;

/// Represents an active compilation unit session, containing shared infrastructure.
///
/// In a future Salsa-based incremental engine this struct becomes the
/// `salsa::Database`, with `ParseCache` and friends absorbed into memoized queries.
#[derive(Debug)]
pub struct CompileSession {
    /// Canonical type interner for type identity deduplication.
    pub type_interner: TypeInterner,
    /// Parse-result cache keyed by file path (avoids re-parsing stdlib).
    pub parse_cache: ParseCache,
    /// Memoized stdlib file-system path resolution (avoids repeated `stat`).
    pub stdlib_cache: StdlibPathCache,
}

impl CompileSession {
    /// Creates a new empty compilation session.
    #[must_use]
    pub fn new() -> Self {
        Self {
            type_interner: TypeInterner::new(),
            parse_cache: ParseCache::new(),
            stdlib_cache: StdlibPathCache::new(),
        }
    }
}

impl Default for CompileSession {
    fn default() -> Self {
        Self::new()
    }
}
