# Solidification matrix (pré–Fase 3 de linguagem)

**Status: S5 GATE CLOSED** (2026-07) — compiler foundation solid enough to resume language-level Fase 3 work.

Cranelift is **host-only** (typically 64-bit). 32-bit / embedded validation is **layout + C emit**, not Cranelift.

## Backend roles

| Backend | Targets | Notes |
|---------|---------|--------|
| Cranelift | Host (64-bit today) | `run` / JIT; **no 32-bit Cranelift** |
| C | 32 + 64 (+ i686 layout) | `emit-c --layout=host\|ptr4\|i686`; portable path |
| LLVM | later | Out of solidification scope |

## S5 gate checklist (closed)

| Criterion | Status |
|-----------|--------|
| Critical resolve/typeck on unified import path (`resolve_for_test` = thin wrapper) | done |
| O008/move diags: use → decl → symbol span; lower records `use_span` | done |
| Layout tests W=4 and W=8; fat ptr `len` = usize; no magic `+8` for fat len | done |
| `DataLayout` + `SizeAlign`; `host` / `ptr_width` / `i686_sysv` | done |
| Host C↔Cranelift parity suite (incl. control flow, str audit) | done |
| C emit smoke for layout 32 / i686 ArStr | done |
| AMIR `TypeId` on locals/temps/params/return; `is_copy` / `is_memory` | done |
| CLI `emit-c` with `--layout=` | done |
| Clippy on solidification crates (`-D warnings`) | gate verify |
| Doc ABI aligned with code | done |

### Explicitly **out** of this gate (honest backlog)

- Full Salsa orchestration of AMIR/analyses/codegen  
- Display / `to_str` / non-str `println`  
- C quality beyond “compiles + correct” (copy-prop, named struct fields, freestanding RT)  
- LLVM, gen-fallback, ownership surface syntax  
- TypeId on every IR node outside AMIR locals/temps  

## What was fixed (do not reopen as “workarounds”)

- SET / GUARD / NEST / F64 / ERR-NIL / INTERP reject non-str  
- Path canonicalize + ModuleLoader + structural stable_hash  
- `println(str)`, CLI warn ≠ exit failure  
- Shared AMIR rvalue visitor; Len/Alloc; real ArStr fat pointer  
- Spans threaded through lower; TYP-1 Error via interner  

## Test policy (standing)

- Prefer `resolve_for_test` (unified imports) or full Salsa CLI path.  
- Layout tests cover `pointer_width` 4 and 8 in **middle**.  
- Parity C↔Cranelift **host only**.  
- No hardcoding `+8` for fat-pointer len.  
- New features: fail on product path first (CLI/Salsa), then fix root cause.  

## After the gate

Language roadmap Fase 3 (ownership surface, Display, OS runtime, …) may proceed **without** reopening solidification items above unless a regression appears.

C “portability quality” (named structs, less SSA noise, embedded RT) is a **separate** backend track.
