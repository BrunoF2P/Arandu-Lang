# Arandu Minimal 0.1 — Freeze & Tracking

**Status:** **P0 + P2 project CLI gold** — installer packaging remains (tarball/prefix layout)  
**Date:** 2026-07-11 (updated 2026-07-19)  
**Goal:** define a **stable, installable language surface** before public site.  
**Out of scope for this freeze:** beautiful marketing site (last), LLVM, self-host, package registry.

---

## 1. Purpose

After Minimal 0.1 is **green**:

1. Ship **installer packaging** (prefix layout: `bin/` + `share/arandu/stdlib`) — resolution cascade already implemented  
2. ~~Ship **project CLI**~~ — **[x]** `new` / `build` / `run` / `check` / `doctor` / `fmt` (see §14)  
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
| `std.path` | `is_empty` + host `is_absolute` / `join` / `file_name` | **IN optional** — **PROMOTE-L4 closed** |
| `std.env` | `args_len` + `var_is_set` host (read-only) | **IN optional** thin; no setenv |
| `std.fs` | exists() scaffold | OUT / experimental |
| `std.io` module | write/eprint scaffold (prelude io is separate) | OUT / experimental |
| `std.process` | `exit` host-backed | **IN optional** thin |
| `std.time` | `monotonic_ns` host-backed | **IN optional** thin |
| `std.alloc.vec` | pure-buffer `Vec<T>` free-func API (new/with_capacity/push/…/destroy) | **IN optional** — **PROMOTE-L6 closed**; not in default template |
| `std.alloc.allocator_api` | free-func global/bump thin (`ar_vec_*`, OOM abort) | **IN optional** — not in default template |
| `std.core.str` | concat / starts_with / ends_with / split_last / len | **IN optional** — host fat-str |
| `std.alloc.gen_arena` | pure-buffer free-func (`new`/`insert`/`get`/`remove`/`len`/`destroy`) | **IN optional** — GenArena thin closed; not in default template; compiler `ar_gen_*` i64 remains for AMIR promote |

### 4.3 Std half-done / bugs to track before freeze green

| ID | Issue | Severity for Minimal | Suggested fix |
|----|--------|----------------------|---------------|
| S1 | `std.core.ptr` broken twin | was high | [x] fixed as compat shim → `ptrOffset` |
| S2 | `path.is_absolute` host-backed | was medium | [x] `Path::is_absolute` + m10 gold (P1.2) |
| S3 | `path.join` / `file_name` stubs return input | was low | **[x]** host `Path::join` / `file_name` + m10 gold (PROMOTE-L4) |
| S4 | alloc body typeck noise if linked as dependency | was medium | **[x]** vec + allocator_api thin check-clean (free-func; OOM abort) |
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

| # | Task | Done when | Status |
|---|------|-----------|--------|
| P1.1 | Wire or explicitly exclude `std.process.exit` / `std.time` / `std.env` host | either e2e or OUT table final | **[x]** hosts `ar_process_exit` / `ar_time_monotonic_ns` / `ar_env_*` + gold m11/m12 |
| P1.2 | `path.is_absolute` host-backed or documented + test only documented cases | path e2e stable | **[x]** `Path::is_absolute` + expanded m10 |
| P1.3 | Mark experimental modules in source (`/// experimental — not Minimal 0.1`) | runtime reactor/tcp/supervisor headers | **[x]** runtime sections + fs/io banners |
| P1.4 | CI job `minimal-gold` running only §8 | CI green | **[x]** job `minimal-gold` → `cli_minimal_gold` |

### P2 — post-freeze (installer phase)

| # | Task | Status |
|---|------|--------|
| P2.1 | `Arandu.toml` schema + Salsa `ProjectManifest` input (BLAKE3 hash in key) | [x] |
| P2.2 | `arandu new` / package `check`/`run`/`build` | [x] |
| P2.3 | Stdlib cascade (`--stdlib-path` > `ARANDU_STDLIB` > `current_exe` **canonicalize**) + `doctor` | [x] |
| P2.4 | Beautiful site + playground on Minimal | pending |
| P2.5 | Versioned atomic install + tarball BLAKE3 + isolated smoke CI | [x] scripts + `install-smoke` job |

### P3 — post-Minimal language (roadmap)

| # | Task |
|---|------|
| Future trait, dyn, effects, LLVM, full OS std, GenArena typed self-host | roadmap v0.35–v1.0 |

---

## 8. Gold suite target (`examples/minimal/`)

Create these files (names fixed for tracking):

| File | Exit | Covers |
|------|------|--------|
| `m01_hello.aru` | 0 | println, main |
| `m02_structs_enums.aru` | 3 | types, match |
| `m03_result_option.aru` | 7 | Result + `?` |
| `m04_generics_bounds.aru` | 10 | free-function generics (not method-via-T; see L1) |
| `m05_borrow_shared.aru` | 5 | shared self method |
| `m06_async_await.aru` | 42 | async func + await |
| `m07_async_spawn_join.aru` | 42 | std.runtime spawn/join |
| `m08_modules/main.aru` | 9 | multi-file via **stdlib** (not package-local; see L2) |
| `m09_interp_tostr.aru` | 0 | string interp |
| `m10_path_empty.aru` | 0 | path thin IN (`is_empty` / `is_absolute` / `join` / `file_name`; PROMOTE-L4) |
| `m11_process_exit.aru` | 17 | `std.process.exit` host (P1.1) |
| `m12_time_env.aru` | 0 | `std.time` + `std.env` hosts (P1.1) |
| `m13_vec.aru` | 78 | `std.alloc.vec` pure-buffer free-func API (PROMOTE-L6 complete) |
| `m14_mem_intrinsics.aru` | 46 | mem sizeOf/ptrOffset/Read/Write (L6.1) |
| `m15_vec_capacity.aru` | 21 | with_capacity / capacity / reserve / clear / is_empty |
| `m16_gen_arena.aru` | 83 | `std.alloc.gen_arena` pure-buffer free-func (insert/get/remove/recycle) |
| `m17_pod_copy.aru` | 60 | structural POD auto-copy (named scalar structs by value) |
| `m18_vec_methods.aru` | 78 | method-style `v.push` (receiver mono) |
| `m19_allocator.aru` | 112 | `std.alloc.allocator_api` Result+AllocError + bump align |
| `m20_str.aru` | 0 | `std.core.str` concat/starts/ends/split_last |
| `m21_result_custom_e.aru` | 7 | `Result.Ok/Err` with custom `E` (return context) |
| `m22_iface_param.aru` | 42 | method via `T: Show` (PROMOTE-L1) |
| `m23_match_result.aru` | 13 | bare `Ok`/`Err` on call scrutinee (`match f() { … }`) |
| `m24_expect_or_abort.aru` | 13 | `Result.expectOrAbort` (import + method mono + shared-self match) |
| `TEMPLATE_main.aru` | 0 | default installer template |

**Command contract:**

```bash
arandu_cli check examples/minimal/...
arandu_cli run   examples/minimal/...   # exit code asserted in tests
```

Status: **[x] suite lives in `examples/minimal/`** — guarded by `cli_minimal_gold`.

---

## 9. Default project template (`arandu_cli new`)

```text
my_app/
  Arandu.toml          # name = "my_app", version = "0.0.1", entry = "src/main.aru"
  src/
    main.aru
```

```aru
// src/main.aru — Minimal 0.1 template (IN surface only)
module my_app

import io

func main(): int {
    io.println("hello, arandu")
    return 0
}
```

**Do not** import experimental runtime/tcp/supervisor in the default template.

**CI:** `examples/minimal/TEMPLATE_main.aru` is covered by `test_new_project_template_parses_cleanly` (same pipeline as stdlib parse cleanliness).

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
| 2026-07-11 | P0 implemented; D6 unblocked for installer |
| 2026-07-11 | §13 rationale + `PROMOTE-*` tracks documented for post-install work |
| 2026-07-19 | P2 gold bars: manifest Salsa input, stdlib `current_exe` cascade, `doctor`, template in parse CI, run `[cached]`/`[rebuilt]` |
| 2026-07-19 | PROMOTE-L2: dual ModuleRoots + DirectoryListing VFS; package `import my_app.util` |
| 2026-07-19 | Watch mode: shared `DebouncedMap`/`EditVfs` with LSP; `arandu watch` + notify-debouncer-full |
| 2026-07-20 | Workspace crate / installer / extension version set to **0.0.1** (honest pre-0.1; first installable profile will be 0.1.0) |
| 2026-07-20 | DiagCode ↔ docs/errors via xtask (single source); CI jobs split; install-smoke matrix ubuntu+macos early |
| 2026-07-20 | **P1 quality:** wire `process`/`time`/`env` hosts; `Path::is_absolute`; experimental banners; CI `minimal-gold` |
| 2026-07-20 | **PROMOTE-L6:** pure-buffer `std.alloc.vec` + free-func API, gold m13 exit 78 |
| 2026-07-20 | **L6.1:** mem intrinsics; mut-ref stores; nested free-func mono worklist; generic `push<T>`; gold m13/m14 |
| 2026-07-20 | **PROMOTE-L6 closed (Vec thin):** `with_capacity`/`capacity`/`is_empty`/`reserve`; m15 gold; nested mono + auto-ref infer; DCE multi-path return slot; C mem intrinsics |
| 2026-07-20 | **PROMOTE-L4 closed:** host `path.join` / `file_name` (fat-str); m10 gold real join/file_name; C path helpers |
| 2026-07-20 | **GenArena thin closed:** pure-buffer free-func + recycle gen bump; gold m16=83; `allocator_api` still experimental |
| 2026-07-20 | **POD auto-copy:** `TypeInfo::is_copy` structural (named structs of scalars); GenRef/TaskHandle by value; gold m17=60; Vec-with-ptr not copy |
| 2026-07-20 | **ABCD promote batch:** docs hygiene; allocator_api thin (m19=112); std.core.str (m20); Vec methods + method mono dedupe (m18=78) |
| 2026-07-20 | **Residuals batch:** Result.Ok/Err bidirectional custom `E` (m21); peel Ref for `T: I` methods typeck+AMIR (m22/L1); allocator Result+align; installer scripts already P2.5 |
| 2026-07-20 | **Residuals close:** finish_call trailing-block gated (`match f(){Ok…}`); pattern Ref peel typeck+AMIR (type-only peel for Discriminant ABI); import re-link builtin Result methods; Result/Option method mono; gold m23=13 m24=13; dyn Allocator + `ar_gen_*` escape remain L7/MVP residuals (honest) |

---

## 12. How to use this doc

1. **Installer packaging (next):** P2.5 — prefix layout matching §14 cascade; site still last.  
2. **Promoting features later:** follow §13.4 checklist and `PROMOTE-*` IDs.  
3. **Do not** expand Minimal by dumping all experimental into IN at once (§13.5 order).  
4. Roadmap long-form: `arandu-compiler-roadmap-v0.1.md`. This file = **product freeze + promotion backlog**.  
5. Gold suite: `examples/minimal/` + `cli_minimal_gold` + `cli_project_gold`.

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

#### L1 — Free generics + receiver mono + interface-via-`T: I` **CLOSED (2026-07-20)**

| | |
|--|--|
| **Root fix** | (1) method generic_params dedupe restated `T`; (2) peel `Ref`/`RefMut` on receivers in typeck + AMIR `resolve_method_target` so `shared value: T` resolves `T: I` methods |
| **Gold** | m18=78 Vec methods; **m22=42** `emit<T: Show>(value.show())` |
| **Track ID** | `PROMOTE-L1` **[x] closed** |

#### L2 — Multi-file package modules — **PROMOTED (2026-07-19)**

| | |
|--|--|
| **Symptom (was)** | `import my_app.util as u` from `src/util.aru` did not resolve like Cargo |
| **Root fix** | Same `resolve_module_path`: dual [`ModuleRoots`](../crates/arandu_query/src/vfs.rs) (package listing + stdlib). Keys still from `canonicalize_import_path` (`my_app/util.aru` / `stdlib/…`) |
| **Policy now** | Package mode registers `DirectoryListing` for entry dir; existence is Salsa input (not bare `fs::exists`). Reserved package names (`std`, `io`, …) rejected at manifest parse. N006 on alias clash local↔stdlib. Cycles reuse `cycle_recover` |
| **Gold** | `package_modules_l2` + `cli_project_gold::package_local_multi_file_check_and_run` |
| **Track ID** | `PROMOTE-L2` **[x]** |

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

#### L4 — `path.join` / `file_name` — **CLOSED (2026-07-20)**

| | |
|--|--|
| **Root fix** | Host `Path::join` / `Path::file_name` fat-str returns (`ar_path_join` / `ar_path_file_name`); same family as `is_absolute` |
| **Policy** | **`std.path` IN optional** — not in default template; Unix gold on Linux CI |
| **Gold** | m10 exit 0 (join `/tmp`+`x`, file_name leaf, absolute replace) |
| **C backend** | static path helpers when `ArStr` runtime is emitted |
| **Residual** | pure-language join via str concat/split (not required for thin) |
| **Track ID** | `PROMOTE-L4` **[x] closed** |

#### L5 — `std.env` / `fs` / `process` / `time` / module `std.io`

| | |
|--|--|
| **Root** | Declarations or scaffold; host incomplete or no Minimal e2e |
| **Policy** | Experimental banners in source. Prelude **`import io`** remains **IN** (println wired) |
| **Promote when** | Host symbols + gold + not required by default template |
| **Track ID** | `PROMOTE-L5-*` (env, fs, process, time, std.io) |

#### L6 — Vec / allocator_api / GenArena — **CLOSED for Vec thin + GenArena thin (2026-07-20)**

| | |
|--|--|
| **Root fix** | Pure-buffer (`ar_vec_malloc/realloc/buf_free` + mem); generic free-func API; nested mono worklist; auto-ref type-param infer |
| **Policy** | **`std.alloc.vec` + `gen_arena` + `allocator_api` IN optional** — not in default `arandu new` |
| **Public API (Vec)** | free-func + **methods** `v.push` / `pop` / `get` / … (receiver mono) |
| **Public API (GenArena)** | `new`, `insert`, `get`, `remove`, `len`, `is_empty`, `destroy`; `GenRef` by value (POD auto-copy) |
| **Public API (allocator)** | `global_alloc`/`dealloc`/`realloc`; `bump_new`/`alloc`/`reset`/`remaining`/`destroy` |
| **Gold** | m13=78, m15=21, m16=83, m18=78, m19=112; module check-clean |
| **L6.1** | **[x]** mem intrinsics; mut-ref materialize; while SSA; DCE jump-args + multi-path `_0`; C mem emit |
| **Checklist §13.4 (Vec)** | **[x]** root fixed · **[x]** gold · **[x]** CI gold · **[x]** IN optional · **[x]** methods m18 · **[x]** not in default template |
| **Checklist (GenArena thin)** | **[x]** pure-buffer · **[x]** recycle · **[x]** gold m16 · **[x]** POD GenRef |
| **Checklist (allocator thin)** | **[x]** free-func · **[x]** gold m19 · **[x]** check-clean · **[x]** not default template |
| **Residual** | **dyn `Allocator` trait object = L7 OUT** (not thin-close); **`ar_gen_*` i64 AMIR escape promote only** (stdlib GenArena free-func is the user path; host `ar_gen_*` stays for compiler escape promote MVP) |
| **Track ID** | `PROMOTE-L6` **[x]**; methods **[x]**; allocator thin **[x]**; Result custom E **[x]**; bare Ok match **[x]**; expectOrAbort **[x]** |

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
                  →  PROMOTE-L6 Vec thin [x] + GenArena thin [x]
                  →  PROMOTE-L4 path join [x]
                  →  PROMOTE-L5 process/time/env as needed
                  →  PROMOTE-L3a/b async-io profile (optional second template)
                  →  L1 / L7 language deep features
                  →  site/playground on Minimal (+ optional profiles)
```

Do **not** expand Minimal by dumping all experimental into IN at once.

---

## 14. One-line summary

**Minimal 0.1 = language + async coroutine + SyncExecutor spawn/join + thin core/prelude + honest experimental fence — then install and project CLI; site last. Limits are product promises, not abandoned code; promote via §13.4.**

---

## 15. Project CLI gold (P2 summary)

Implemented in-tree (see also `docs/arandu-project-cli-gold-v0.1.md`):

| Gold | Mechanism |
|------|-----------|
| Stdlib never cwd | `resolve_stdlib_root`: flag → `ARANDU_STDLIB` → `current_exe` install layout / walk |
| `arandu doctor` | Flutter-style categories; binary, stdlib, manifest, Cranelift `try_new` |
| Manifest Salsa input | `ProjectManifest` + BLAKE3 `content_hash` + `manifest_fingerprint` |
| Template in parse CI | `TEMPLATE_main.aru` + `test_new_project_template_parses_cleanly` |
| Run status line | DX.5 `RebuildLog::status_line` → `[cached]` / `[rebuilt: N queries]` |
| Backend flags | `build` = Cranelift; `build --release` = LLVM reserved (exit 2 until ready) |

**Next:** P2.5 packaging script that installs into `$PREFIX/{bin,share/arandu/stdlib}`; then site.

