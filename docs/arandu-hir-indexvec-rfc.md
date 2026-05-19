# RFC: HIR storage with IndexVec (future)

**Status:** Proposed (not implemented in v0.1)  
**Target:** v0.2+ when whole-crate or multi-file compilation needs lower memory churn

## Context

AHIR v0.1 uses an owned tree (`Box<HirExpr>`, nested `Vec`s) with `Clone` on most nodes. That is simple and sufficient for single-file batch lowering and golden tests.

Rustc moved THIR from a monolithic arena to [`IndexVec`](https://github.com/rust-lang/rust/pull/83842) per expression, block, and arm so that:

- IR nodes are small handles (better cache locality when walking the CFG builder)
- incremental/query backends can retain stable indices
- deep `Clone` of the whole tree is avoided

## Proposal

1. Introduce typed IDs: `ExprId`, `StmtId`, `BlockId`, `ArmId` (newtypes over `u32`).
2. Store `IndexVec<ExprId, HirExprData>` (flat enum without nested `Box` children — children are IDs).
3. Lowering allocates into one `HirBody` arena per function; module-level decls stay in `Vec<HirDecl>` with optional `body: Option<HirBody>`.
4. Pretty-print and `validate_invariants` walk by index instead of reference.
5. Keep `Span` and `ArType` on each expression record.

## Non-goals (v0.2)

- Incremental compilation / query system
- Removing `name: String` from decl nodes (separate cleanup)

## Migration

- Phase A: add parallel `hir_indexed` module behind feature flag; golden tests compare pretty output.
- Phase B: switch `lower_to_hir` to indexed storage; delete tree `HirExpr` when stable.
- Phase C: AMIR lowering reads `HirBody` by ID (no change to AMIR shape).

## Acceptance

- No behavioral change in `arandu hir` / `arandu amir` output for existing fixtures.
- `cargo test -p arandu_semantics` passes; memory profile on large synthetic file shows fewer allocations vs tree HIR.
