# Arandu Minimal 0.1 — Freeze & Tracking

**Status:** **P0 implemented** (gold suite green) — ready for installer design  
**Date:** 2026-07-11 (updated)  
**Goal:** define a **stable, installable language surface** before installer / project CLI.  
**Out of scope for this freeze:** beautiful marketing site (last), LLVM, self-host, package registry.

---

## 1. Purpose

After Minimal 0.1 is **green**:

1. Ship **installer** (binaries + stdlib path)  
2. Ship **project CLI** (`new` / `build` / `run` / `check` / `fmt`)  
3. Only then: public site + playground on this profile  

Anything not in **IN** is either **OUT (experimental)** or **BLOCKER** until fixed.

---

## 2. Definition of Done (Minimal 0.1)

| # | Criterion | Status |
|---|-----------|--------|
| D1 | Document exists and lists IN / OUT / BLOCKERS | [x] this file |
| D2 | Every **IN** language feature has ≥1 gold example under `examples/minimal/` (or tagged stable) that `check` + `run` pass on CI | [x] `cli_minimal_gold` |
| D3 | **Std thin** modules marked IN are check-clean **or** explicitly “signatures-only, no body link required” | [x] ptr fixed; OUT scaffolds labeled experimental |
| D4 | **Async minimal** contract written and e2e examples pass | [x] m06 + m07 |
| D5 | No silent half-done paths in IN surface (or they are documented + diagnostics honest) | [x] path stubs + experimental banners |
| D6 | Installer + project CLI may start only when D2–D5 are green | [x] **unblocked** — implement next |

---

## 3. Surface matrix

### 3.1 Language — **IN** (install guarantees)

| Area | Included | Notes / known limits |
|------|----------|----------------------|
| Modules + `import path as alias` | yes | multi-file HIR link for bodies |
| Free funcs, methods, `shared`/`mut`/`own self` | yes | formals as `&T`/`&mut T` |
| Structs, enums, match | yes | patterns: bind, `_`, range, or |
| Generics + defaults + constraints (`T: I`, `where`) | yes | interfaces structural (duck) |
| `Result` / `Option` + sugar (`.Ok`/`.Some`/`.None`/…) | yes | expected-type driven |
| Implicit tail return | yes | SYN.1 |
| String interp + ToStr v0.1 (scalars) | yes | not user Display |
| Nullable / `?.` / `??` (as language) | yes | `??` is CFG in AMIR |
| `&` / `&mut`, borrow/move checks | yes | OSSA intraprocedural |
| `async func` / `async {}` / `await` | yes | see §4 async |
| `unsafe { stmt… }` (statement form) | yes | required for extern host |
| Extern `"C"` / host symbols via JIT | yes | for std thin + runtime |

### 3.2 Language — **OUT** (experimental / post-Minimal)

| Item | Why out |
|------|---------|
| `unsafe` **expression** form (`let x = unsafe { … }`) | U001 AMIR; use stmt form |
| Indirect calls / function pointers | T033 by design for now |
| `dyn` / existential interface values | TYP.1 residual |
| Effects system (A2) | not started |
| Full user `Display` / custom `to_str` for structs | later |
| LLVM / AOT product binary | post-install; Minimal may use JIT + emit-c |
| Package registry / remote deps | post-Minimal |
| Self-host | Fase 6 |

### 3.3 Async — **IN** (contract of Minimal)

| API | Status | Install promise |
|-----|--------|-----------------|
| `async func` / `async {}` → `Coroutine[T]` | done (A3) | yes |
| `await` (single-task drive) | done | yes |
| `Poll<T>` in `std.core.future` | done | yes |
| `std.runtime`: `SyncExecutor`, `new_sync_executor` | done | yes |
| `spawn` / `join` / `block_on` generic + `*_int` | done | yes (multi-file infer OK) |
| `TaskHandle` | done | yes |

**Async Minimal promise (one paragraph):**  
Coroutines are language. Multi-task needs **explicit** `SyncExecutor`. Payload host is **i64-shaped** (int and pointer-sized values). No global runtime.

### 3.4 Async — **OUT** of Minimal install (ship code OK, not guaranteed)

| Item | Reality | Track as |
|------|---------|----------|
| `EpollReactor` + sleep/arm/poll | implemented | experimental |
| `reactor_backend` / io_uring detect + sleep | implemented | experimental |
| `tcp_*` / `tcp_wait` / `*_async` | implemented host | experimental |
| `Waker` / `Context` handles | implemented | experimental |
| `Supervisor` process model | implemented | experimental |
| `Future.poll` trait on Coroutine | **not** done | post-Minimal |
| Full multi-wake scheduler / thread pool | not done | post-Minimal |

> **Rule:** experimental modules may live in the tree and tests, but **installer docs and `arandu new` templates must not require them.**

---

## 4. Std thin — inventory & freeze policy

### 4.1 **IN** for Minimal (must be honest + usable)

| Module | Role | Body quality | Host deps | Freeze action |
|--------|------|--------------|-----------|---------------|
| `std.core.prelude` | re-exports | thin | — | keep |
| `std.core.result` | `Result` + expectOrAbort | OK | abort | keep; fix abort linkage consistency |
| `std.core.option` | `Option` + expectOrAbort | OK | abort | keep |
| `std.core.future` | `Poll<T>` only | OK | — | keep; no Future trait in Minimal |
| `std.core.intrinsics` | abort traps | OK | intrinsic | keep |
| `std.core.mem` | sizeOf/alignOf/ptr* | extrinsic | intrinsic | keep |
| `std.core.pointer` | `offset` wrapper | OK | mem | keep as canonical |
| Prelude `io` / `err` | println / err.new | host | JIT | keep for demos |
| `std.runtime` (**subset**) | SyncExecutor + spawn/join/block_on | OK | `ar_rt_*` | **export only Minimal surface in docs**; rest experimental |

### 4.2 **Thin / scaffold** — OUT of Minimal templates

| Module | Reality | Freeze action |
|--------|---------|---------------|
| `std.path` | pure helpers (`is_empty`, limited `is_absolute`) | **IN optional**: only functions that pass e2e; document limits |
| `std.env` | extern decls only | OUT Minimal (or signatures-only experimental) |
| `std.fs` | exists() scaffold | OUT |
| `std.io` module | write/eprint scaffold (prelude io is separate) | OUT until host wired |
| `std.process` | exit scaffold | OUT (or wire host + e2e) |
| `std.time` | monotonic_ns scaffold | OUT until host + e2e |
| `std.alloc.vec` | full-ish API; typeck residuals in alloc bodies | **IN only if** `Vec` defaults path stays green (`cli_vec_defaults`); else signatures-only |
| `std.alloc.allocator_api` | GlobalAllocator + Bump; residual body diags | experimental for install |
| `std.alloc.gen_arena` | typed API + i64 host MVP | experimental (GenRef path OK for advanced) |

### 4.3 Std half-done / bugs to track before freeze green

| ID | Issue | Severity for Minimal | Suggested fix |
|----|--------|----------------------|---------------|
| S1 | `std.core.ptr` broken twin | was high | [x] fixed as compat shim → `ptrOffset` |
| S2 | `path.is_absolute` is whitelist heuristic (`/`, `/tmp`) not real path | medium | host `ar_path_*` wrapper **or** document as limited; add e2e |
| S3 | `path.join` / `file_name` stubs return input | low for Minimal | mark experimental or stub-doc |
| S4 | alloc body typeck noise if linked as dependency | medium | keep entry-only check policy; don’t put Vec in default template until clean |
| S5 | runtime i64 payload honesty | low if documented | doc in install + Minimal async § |
| S6 | prelude vs `std.io` dual story | low | Minimal uses prelude `io` only |

---

## 5. Compiler / pipeline half-done (small things)

| ID | Item | IN Minimal? | Action |
|----|------|-------------|--------|
| C1 | `unsafe` expr form | no | stay U001; doc stmt form |
| C2 | Indirect calls | no | T033 |
| C3 | JIT = dev backend only | yes as runtime for `run` | installer ships JIT runner; AOT later |
| C4 | emit-c = portable dump | optional for build later | project CLI may use emit-c+cc as build path |
| C5 | Multi-file HIR link skips modules with body errors | yes | keep; don’t pull broken alloc into default projects |
| C6 | Namespace generic infer (`rt.spawn`) | yes | done; gold test required |
| C7 | Local inferred `join` mono | yes | done; gold test required |
| C8 | TyCtx file_id isolation | yes | done |
| C9 | UTF-8 item fingerprint char boundaries | yes | done |
| C10 | Cranelift layout residuals for some named types | case-by-case | only block if Minimal examples hit them |
| C11 | `??` BinaryOp JIT path | no if only CFG used | keep honesty (CFG path) |
| C12 | GenArena typed-T self-host | no | post-Minimal |

---

## 6. Tooling freeze (what installer/CLI will assume)

### 6.1 Today (pre-installer)

```text
arandu_cli  lex|parse|check|hir|amir|run|emit-c|graph|fmt  <file>
arandu-lsp  (stdio)
```

### 6.2 Target after Minimal green (next phase — not this freeze’s code)

| Command | Role |
|---------|------|
| `arandu new <name>` | scaffold `Arandu.toml` + `src/main.aru` |
| `arandu check` | whole package |
| `arandu build` | package → artifact (emit-c+cc **or** package run blob) |
| `arandu run` | build + exec |
| `arandu fmt` | package format |
| install script / release tarball | `arandu` + `arandu-lsp` + stdlib root |

**Site (beautiful):** after installer + CLI of Minimal. Not tracked as blocker of freeze.

---

## 7. Work backlog to reach freeze green

Ordered for closing **origin** issues before install.

### P0 — must close before installer

| # | Task | Owner track | Done when |
|---|------|-------------|-----------|
| P0.1 | Fix or remove `std.core.ptr` (S1) | std | [x] ptr → ptrOffset shim |
| P0.2 | Create `examples/minimal/` with gold suite | docs/CI | [x] + `cli_minimal_gold` |
| P0.3 | Async Minimal e2e: async func + await; spawn/join multi-file | async | [x] m06, m07 |
| P0.4 | Document IN/OUT in README pointer to this file | docs | [x] |
| P0.5 | Default template content draft (main only uses IN surface) | product | [x] `TEMPLATE_main.aru` |

### P1 — should close for quality (same freeze if cheap)

| # | Task | Done when |
|---|------|-----------|
| P1.1 | Wire or explicitly exclude `std.process.exit` / `std.time` / `std.env` host | either e2e or OUT table final |
| P1.2 | `path.is_absolute` host-backed or documented + test only documented cases | path e2e stable |
| P1.3 | Mark experimental modules in source (`/// experimental — not Minimal 0.1`) | runtime reactor/tcp/supervisor headers |
| P1.4 | CI job `minimal-gold` running only §8 | CI green |

### P2 — post-freeze (installer phase)

| # | Task |
|---|------|
| P2.1 | `Arandu.toml` schema |
| P2.2 | `arandu new` / package `check`/`run` |
| P2.3 | Release installer (std path env `ARANDU_STDLIB` or embed) |
| P2.4 | Beautiful site + playground on Minimal |

### P3 — post-Minimal language (roadmap)

| # | Task |
|---|------|
| Future trait, dyn, effects, LLVM, full OS std, GenArena typed self-host | roadmap v0.35–v1.0 |

---

## 8. Gold suite target (`examples/minimal/`)

Create these files (names fixed for tracking):

| File | Covers |
|------|--------|
| `m01_hello.aru` | println, main |
| `m02_structs_enums.aru` | types, match |
| `m03_result_option.aru` | Result/Option + sugar |
| `m04_generics_bounds.aru` | `<T: I>` / where |
| `m05_borrow_shared.aru` | shared/mut self, auto-ref |
| `m06_async_await.aru` | async func + await |
| `m07_async_spawn_join.aru` | import std.runtime, spawn/join infer |
| `m08_modules/` | multi-file local import |
| `m09_interp_tostr.aru` | string interp |
| `m10_path_empty.aru` | optional path thin |

**Command contract:**

```bash
arandu_cli check examples/minimal/...
arandu_cli run   examples/minimal/...   # exit code asserted in tests
```

Status: **[x] suite lives in `examples/minimal/`** — guarded by `cli_minimal_gold`.

---

## 9. Default project template (draft for installer)

```text
my_app/
  Arandu.toml          # name = "my_app", version = "0.1.0", entry = "src/main.aru"
  src/
    main.aru
```

```aru
// src/main.aru — Minimal 0.1 template (IN surface only)
module my_app

func main(): int {
    io.println("hello, arandu")
    return 0
}
```

**Do not** import experimental runtime/tcp/supervisor in the default template.

Optional second template later: `async-hello` with `std.runtime` spawn/join.

---

## 10. Async Minimal — freeze contract (detail)

### Guaranteed

```aru
async func f(): int { return 1 }
func main(): int {
    return await f()
}
```

```aru
import std.runtime as rt
async func f(): int { return 42 }
func main(): int {
    let ex = rt.new_sync_executor()
    let h = rt.spawn(ex, f())
    return rt.join(ex, h)
}
```

### Not guaranteed in Minimal install

- Payload types larger / non-i64-shaped without extra host support  
- Fair multi-task scheduling beyond cooperative queue  
- Production TCP/async IO without experimental labels  
- `Future` interface polymorphism  

---

## 11. Decision log

| Date | Decision |
|------|----------|
| 2026-07-11 | Freeze defined as **profile + tracking**, not full roadmap close |
| 2026-07-11 | Site after installer + project CLI |
| 2026-07-11 | Experimental runtime may stay in tree; templates/docs ignore it |
| 2026-07-11 | Installer blocked on P0 + gold suite green |

---

## 12. How to use this doc

1. Work **only** P0/P1 until D2–D5 green.  
2. Check boxes in §2 and §7 as items land.  
3. Do **not** start installer until D6.  
4. Roadmap long-form remains `arandu-compiler-roadmap-v0.1.md`; this file is the **product freeze**.

---

## 13. One-line summary

**Minimal 0.1 = language + async coroutine + SyncExecutor spawn/join + thin core/prelude + honest experimental fence — then install and project CLI; site last.**
