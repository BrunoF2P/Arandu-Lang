use arandu_lexer::Span;
use rustc_hash::FxHashMap;

use crate::SymbolId;

use super::types::TypeId;

// ── TyCtx — typing context ─────────────────────────────────────────

/// Typing context that accumulates bindings as we walk the AST.
///
/// Unlike the `SymbolTable` (which tracks names in scopes), `TyCtx` maps
/// each `SymbolId` to its `TypeId`. It also tracks the expected return
/// type of the current function and whether we're inside a loop.
///
/// ## Multi-file invariant
///
/// Bindings **must** key on the full [`SymbolId`] (`file_id` + `local_id`).
/// Indexing only by `local_id` collides imported functions with locals that
/// happen to share a dense index (e.g. `let p = f()` then `rt.spawn_i64(...)`
/// saw the local's type as the callee). That is a multi-module root bug, not
/// a surface symptom.
#[derive(Debug)]
pub struct TyCtx {
    /// Map from full `SymbolId` → inferred/declared `TypeId`.
    bindings: FxHashMap<SymbolId, TypeId>,

    /// Stack of expected return types for nested functions/lambdas.
    return_stack: Vec<TypeId>,

    /// Span of the return type in each nested function declaration.
    return_decl_span_stack: Vec<Span>,

    /// Depth of loop nesting (for validating break/continue).
    loop_depth: u32,

    /// Depth of unsafe block nesting.
    unsafe_depth: u32,
}

impl Default for TyCtx {
    fn default() -> Self {
        Self::new()
    }
}

impl TyCtx {
    #[must_use]
    pub fn new() -> Self {
        Self {
            bindings: FxHashMap::default(),
            return_stack: Vec::new(),
            return_decl_span_stack: Vec::new(),
            loop_depth: 0,
            unsafe_depth: 0,
        }
    }

    // ── Bindings ────────────────────────────────────────────────────

    /// Record that `symbol` has type `ty`.
    pub fn bind(&mut self, symbol: SymbolId, ty: TypeId) {
        self.bindings.insert(symbol, ty);
    }

    /// Look up the type for a symbol.
    ///
    /// Reports to the global perf counters when `-Zprofile-queries` is active.
    #[must_use]
    pub fn lookup(&self, symbol: SymbolId) -> Option<TypeId> {
        let result = self.bindings.get(&symbol).copied();
        if result.is_some() {
            arandu_base::perf::track_query_hit();
        } else {
            arandu_base::perf::track_query_miss();
        }
        result
    }

    // ── Return type stack ───────────────────────────────────────────

    /// Push an expected return type when entering a function body.
    pub fn push_return(&mut self, ty: TypeId, decl_span: Span) {
        self.return_stack.push(ty);
        self.return_decl_span_stack.push(decl_span);
    }

    /// Pop the return type when leaving a function body.
    pub fn pop_return(&mut self) {
        self.return_stack.pop();
        self.return_decl_span_stack.pop();
    }

    /// Get the return type expected by the current function.
    #[must_use]
    pub fn current_return(&self) -> Option<TypeId> {
        self.return_stack.last().copied()
    }

    /// Span of the declared return type for the current function.
    #[must_use]
    pub fn current_return_decl_span(&self) -> Option<Span> {
        self.return_decl_span_stack.last().copied()
    }

    // ── Loop tracking ───────────────────────────────────────────────

    /// Enter a loop scope.
    pub fn enter_loop(&mut self) {
        self.loop_depth += 1;
    }

    /// Leave a loop scope.
    pub fn exit_loop(&mut self) {
        self.loop_depth = self.loop_depth.saturating_sub(1);
    }

    /// Returns true if we're inside a loop.
    #[must_use]
    pub fn is_in_loop(&self) -> bool {
        self.loop_depth > 0
    }

    // ── Unsafe tracking ─────────────────────────────────────────────

    /// Enter an unsafe scope.
    pub fn enter_unsafe(&mut self) {
        self.unsafe_depth += 1;
    }

    /// Leave an unsafe scope.
    pub fn exit_unsafe(&mut self) {
        self.unsafe_depth = self.unsafe_depth.saturating_sub(1);
    }

    /// Returns true if we're inside an unsafe block.
    #[must_use]
    pub fn is_in_unsafe(&self) -> bool {
        self.unsafe_depth > 0
    }
}
