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
| S2 | `path.is_absolute` host-backed | was medium | [x] `ar_path_is_absolute` + m10 gold |
| S3 | `path.join` / `file_name` stubs return input | low for Minimal | documented stub; `PROMOTE-L4` |
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

## 13. Why limits exist & why OUT is “experimental”

This section is the **product rationale**. Use it when promoting items later or answering “why isn’t X in Minimal?”.

### 13.1 Two lists (do not conflate)

| List | Meaning |
|------|---------|
| **IN (Minimal)** | Install + tutorial + default template **promise** this works. Guarded by gold suite / CI. |
| **OUT experimental** | Code **may** live in-tree and have tests, but must **not** appear in `arandu new` defaults, install docs, or public “stable” claims. |

Without this fence, a bug in TCP/reactor/alloc becomes “Arandu is broken” on day one of the site.

This is the same idea as **stable vs nightly** in other languages — here named **Minimal 0.1 IN** vs **experimental**.

### 13.2 Why not delete experimental code?

| Delete | Keep experimental |
|--------|-------------------|
| Loses work and tests | Keeps evolving in-repo |
| Reimplement later | Install/docs simply **ignore** |
| Falsely implies “does not exist” | Honest: “exists, no product guarantee” |

**Rule:** experimental may ship in the git tree and even in release tarballs for power users; **templates and Minimal docs never depend on it.**

### 13.3 Rationale for each major limit (track + promote later)

#### L1 — Free generics yes; method call through `T: I` not in gold

| | |
|--|--|
| **Symptom** | `func f<T: I>(shared x: T) { x.m() }` → **T033** (indirect / non-direct call) |
| **Root** | Typeck understands bounds; mono/codegen does not yet materialize stable **direct** method dispatch via type params |
| **Minimal policy** | Gold shows free-function mono (`identity<T>`). Bounds in typeck OK; method-via-param **OUT of gold** |
| **Promote when** | Direct call / mono path for interface methods through type params is green + gold example |
| **Track ID** | `PROMOTE-L1` |

#### L2 — Multi-file “real package” not in Minimal

| | |
|--|--|
| **Symptom** | `import my_app.util as u` from `src/util.aru` does not resolve like Cargo |
| **Root** | `canonicalize_import_path` only rewrites **stdlib** (`std.core.*` → `stdlib/core/…`, `std.*` → `stdlib/std/…`). No `Arandu.toml` package graph yet |
| **Minimal policy** | Gold multi-file = import **stdlib** modules (path, runtime). Local multi-module apps = **P2 installer** |
| **Promote when** | Package CLI resolves package-local modules + gold `m08`-style under `src/` |
| **Track ID** | `PROMOTE-L2` |

#### L3 — Async language + SyncExecutor IN; reactor/TCP/Waker/supervisor experimental

| Layer | Minimal? | Why |
|-------|----------|-----|
| A3 `async` / `await` / `Coroutine` | **IN** | Compiler contract; e2e gold |
| `SyncExecutor` + spawn/join/block_on | **IN** | Explicit executor; multi-file tested |
| EpollReactor / io_uring / sleep | experimental | Host MVP; OS-specific; API still moving |
| TCP + wait/wake + async I/O | experimental | Ports, nonblocking, not needed for hello |
| Waker / Context handles | experimental | Useful for later Future; not required for Minimal promise |
| Supervisor processes | experimental | Ops isolation model; not install-critical |
| `Future.poll` trait on Coroutine | **not done** | Needs richer interface/Self story |

**Async Minimal promise (install):** coroutines are language; multi-task needs **explicit** executor; host payload is **i64-shaped**; **no** global runtime.

**Promote when:** each surface has gold e2e + stable API note in this doc; then move row from §3.4 → §3.3.

| Track ID | Item |
|----------|------|
| `PROMOTE-L3a` | Reactor (sleep/poll) → optional Minimal “async-io” profile |
| `PROMOTE-L3b` | TCP wait/read/write |
| `PROMOTE-L3c` | Waker integrated with spawn |
| `PROMOTE-L3d` | Supervisor |
| `PROMOTE-L3e` | Future trait |

#### L4 — `path.join` / `file_name` stubs

| | |
|--|--|
| **Root** | No first-class stable str concat / split in the language yet |
| **Policy** | `is_empty` + host `is_absolute` **IN optional**; join/file_name documented **stub** |
| **Promote when** | str ops exist + real join/file_name e2e |
| **Track ID** | `PROMOTE-L4` |

#### L5 — `std.env` / `fs` / `process` / `time` / module `std.io`

| | |
|--|--|
| **Root** | Declarations or scaffold; host incomplete or no Minimal e2e |
| **Policy** | Experimental banners in source. Prelude **`import io`** remains **IN** (println wired) |
| **Promote when** | Host symbols + gold + not required by default template |
| **Track ID** | `PROMOTE-L5-*` (env, fs, process, time, std.io) |

#### L6 — Vec / allocator_api / GenArena experimental for install

| | |
|--|--|
| **Root** | API exists; alloc **body** typeck can noise if linked as default deps; GenArena typed tables still host i64 MVP |
| **Policy** | Do not put Vec in default `arandu new` template until path is check-clean end-to-end. GenArena advanced/experimental |
| **Promote when** | `cli_vec_defaults` + alloc module self-check clean; optional `vec-hello` gold |
| **Track ID** | `PROMOTE-L6` |

#### L7 — Language OUT by design or later phase

| Item | Why OUT | Promote track |
|------|---------|----------------|
| `unsafe` **expression** (`let x = unsafe { … }`) | AMIR U001; stmt form works | `PROMOTE-L7-unsafe-expr` |
| Indirect calls / fn pointers | T033 intentional until call story complete | `PROMOTE-L7-indirect` |
| `dyn` / existential interfaces | TYP.1 residual | `PROMOTE-L7-dyn` |
| Effects (A2) | not started | roadmap |
| User `Display` / custom to_str | ToStr v0.1 scalars only | `PROMOTE-L7-display` |
| LLVM / product AOT | post-install | roadmap Fase 5 |
| Package registry | post-Minimal | after P2 |
| Self-host | Fase 6 | roadmap |

#### L8 — JIT as `run` runtime

| | |
|--|--|
| **Policy** | Minimal install may ship **JIT runner** + later `build` via emit-c+cc |
| **Not a bug** | Cranelift host is dev/debug backend by design |
| **Promote** | Native object / LLVM when product needs it |

### 13.4 Promotion checklist (do this when moving experimental → IN)

For each `PROMOTE-*` item:

1. [ ] Root cause fixed (not a workaround only)  
2. [ ] Gold example under `examples/minimal/` (or new profile e.g. `examples/minimal-io/`)  
3. [ ] `cli_minimal_gold` (or dedicated CI job) green  
4. [ ] Move row in §3 (OUT → IN) and update §4 std inventory  
5. [ ] Remove or narrow “experimental” banner in source  
6. [ ] Installer / template: only if default template needs it  
7. [ ] Decision log entry (§11)  

### 13.5 Order suggested for later (after installer)

```text
P2 installer/CLI  →  PROMOTE-L2 package multi-file
                  →  PROMOTE-L6 Vec (if template needs collections)
                  →  PROMOTE-L4 path join
                  →  PROMOTE-L5 process/time/env as needed
                  →  PROMOTE-L3a/b async-io profile (optional second template)
                  →  L1 / L7 language deep features
                  →  site/playground on Minimal (+ optional profiles)
```

Do **not** expand Minimal by dumping all experimental into IN at once.

---

## 14. One-line summary

**Minimal 0.1 = language + async coroutine + SyncExecutor spawn/join + thin core/prelude + honest experimental fence — then install and project CLI; site last. Limits are product promises, not abandoned code; promote via §13.4.**