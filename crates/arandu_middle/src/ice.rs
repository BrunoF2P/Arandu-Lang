//! Internal compiler errors for dense IR invariants.
//!
//! Prefer returning [`crate::Diagnostic`] on fallible paths. These helpers exist
//! only for dense-id pool accessors where a missing id is a **compiler bug**
//! (ids are only minted by the same pool) and there is no `Result` in the API.

/// Fatal ICE for an out-of-range dense pool id (Hir/AMIR tables).
///
/// # Panics
/// Always panics with a stable `ICE:` prefix so it is never a silent failure.
#[inline(never)]
#[cold]
pub fn invalid_dense_id(kind: &str, index: usize) -> ! {
    panic!("ICE: invalid {kind} index {index} (dense pool invariant broken)");
}

/// Fatal ICE for an invariant that has no recovery path and no Diagnostic context.
#[inline(never)]
#[cold]
pub fn bug(message: &str) -> ! {
    panic!("ICE: {message}");
}
