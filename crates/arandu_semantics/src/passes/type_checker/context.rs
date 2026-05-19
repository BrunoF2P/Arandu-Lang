use crate::SymbolId;

use super::types::ArType;

// ── TyCtx — typing context ─────────────────────────────────────────

/// Typing context that accumulates bindings as we walk the AST.
///
/// Unlike the SymbolTable (which tracks names in scopes), TyCtx maps
/// each `SymbolId` to its `ArType`. It also tracks the expected return
/// type of the current function and whether we're inside a loop.
#[derive(Debug, Clone)]
pub struct TyCtx {
    /// Map from SymbolId to its inferred/declared ArType.
    bindings: Vec<(SymbolId, ArType)>,

    /// Stack of expected return types for nested functions/lambdas.
    return_stack: Vec<ArType>,

    /// Depth of loop nesting (for validating break/continue).
    loop_depth: u32,
}

impl Default for TyCtx {
    fn default() -> Self {
        Self::new()
    }
}

impl TyCtx {
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
            return_stack: Vec::new(),
            loop_depth: 0,
        }
    }

    // ── Bindings ────────────────────────────────────────────────────

    /// Record that `symbol` has type `ty`.
    pub fn bind(&mut self, symbol: SymbolId, ty: ArType) {
        self.bindings.push((symbol, ty));
    }

    /// Look up the type for a symbol. Searches most-recent first, so
    /// shadowing is handled correctly.
    pub fn lookup(&self, symbol: SymbolId) -> Option<&ArType> {
        self.bindings
            .iter()
            .rev()
            .find(|(id, _)| *id == symbol)
            .map(|(_, ty)| ty)
    }

    // ── Return type stack ───────────────────────────────────────────

    /// Push an expected return type when entering a function body.
    pub fn push_return(&mut self, ty: ArType) {
        self.return_stack.push(ty);
    }

    /// Pop the return type when leaving a function body.
    pub fn pop_return(&mut self) {
        self.return_stack.pop();
    }

    /// Get the return type expected by the current function.
    pub fn current_return(&self) -> Option<&ArType> {
        self.return_stack.last()
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
    pub fn is_in_loop(&self) -> bool {
        self.loop_depth > 0
    }
}
