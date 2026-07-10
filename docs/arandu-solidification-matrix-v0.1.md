# Solidification matrix (pré–Fase 3 de linguagem)

Status snapshot after root-cause commits (2026-07). Cranelift is **host-only** (typically 64-bit); 32-bit/embedded validation is **layout + C**, not Cranelift.

## Backend roles

| Backend | Targets | Notes |
|---------|---------|--------|
| Cranelift | Host (64-bit today) | `run` / JIT; **no 32-bit Cranelift** |
| C | 32 + 64 + exotic | Portability / embedded path |
| LLVM | later | Out of solidification scope |

## Inventory (S0)

### Dual resolve (`resolve_for_test` callers)

Still used widely as a thin wrapper over `resolve_imports_and_bodies` + `EmptyModuleLoader` (good). Remaining debt: ensure **all** new tests use this or Salsa; avoid hand-rolled import collection.

| Crate / file | Usage |
|--------------|--------|
| `arandu_semantics/tests/*` | type_checker, name_resolution, recovery, root_cause_frontend, hir, amir |
| `arandu_backend_c/tests/parity_tests.rs` | host parity |
| `arandu_backend_cranelift/tests/jit_tests.rs` | host JIT |

### Magic offsets / fixed ABI (multi-target risk)

| Location | Issue |
|----------|--------|
| `backend_c` `ArStr` | `int64_t len` always (host 64 assumption) |
| `backend_c` `Len` on slice | hardcoded `+ 8` |
| Cranelift | fat-ptr len often `I64` / `ptr_type.bytes()` — OK for host; not a 32 JIT |

### Spans

| Area | Issue |
|------|--------|
| `AmirLocal.use_span` | Was always `None` at lower (diags fell back to decl span) |
| Unit-test AMIR fixtures | Often `Span::new(0,0,0)` (OK for synthetic) |
| ICE / some lower diags | Still zero span |

### Done earlier (do not reopen)

- SET / GUARD / NEST / F64 / ERR-NIL / INTERP reject non-str
- Path canonicalize + ModuleLoader + structural stable_hash
- `println(str)`, CLI warn≠error
- Shared AMIR rvalue visitor; C ArStr `{ptr,len}` fields; Len/Alloc stubs filled

## Solidification order

1. **S1** — populate `use_span`; dual-resolve policy docs/tests — **done**  
   - Extended: `with_span` on stmts/places, note origin on consume/free, temp spans from current_span, O* fallback use→decl→symbol  

2. **S2** — fat-pointer `usize` len, no magic `+8`, layout W=4/8 — **done**  
   - Extended: `DataLayout`/`SizeAlign`, `host()`/`i686_sysv()`, float always f64, i64 abi_align on i686  

3. **S3** — host C↔Cranelift parity expand + C ArStr audit — **done** (parity quiet + control_flow + audit)  
4. **S4** — AMIR `TypeId` on locals/temps/return + denormalized `is_copy`/`is_memory` — **done**  
5. **S5** — gate before language Fase 3 features (remaining: clippy, optional TypeId on more IR)  

### AMIR DoD (S4)

| Field | Representation |
|-------|----------------|
| `AmirLocal.ty` / `AmirTemp.ty` / `BlockParam.ty` / `AmirFunc.return_type` | dense `TypeId` |
| `AmirTemp.is_copy` | denormalized bool (move checker needs no interner) |
| `AmirLocal.is_memory` | denormalized bool (prune/dummy load without interner) |
| Resolve at codegen | `TypeInterner::resolve` / `with_type` / `is_copy_v01` |

## Test policy

- Prefer `resolve_for_test` (unified imports) or full Salsa CLI path.  
- Layout tests must cover `pointer_width` 4 and 8 in **middle**.  
- Parity C↔Cranelift only on **host**.  
- New tests must not hardcode `+8` for fat pointers.
