# Arandu — Async Runtime Design (SL_R) v0.1

**Status:** design lock for SL_R; **not implemented**. Consumes A3 compiler semantics.

| Layer | Owns | Does not own |
|-------|------|----------------|
| **Compiler (A3)** | `async`/`await`, `Coroutine[T]`, `Poll[T]`, suspend CFG, pin-free RelativeBorrow, single-task `block_on` | threads, epoll, spawn queues |
| **`std.core.future`** | `Poll` enum; Future surface for libraries | OS reactor |
| **`std.runtime` (SL_R)** | Executor, reactor, multi-task scheduling, async I/O | language syntax / type of `await` |

This split is intentional and **correct** relative to the Rust and Go extremes.

---

## 1. Why not Rust’s fracture, why not Go’s monopoly

**Rust pain:** Future/async live in the language, but executors are ecosystem libraries (tokio / async-std / smol). Spawn and I/O types are not interoperable without compatibility shims.

**Go extreme:** the runtime *is* the language; no no_std path, no alternate scheduler.

**Arandu (locked):**

1. **Semantics in core/compiler** — `Coroutine[T]` + `await` always exist; a program that never `spawn`s needs **no** `std.runtime` import. Drive one coroutine with block_on (A3.6) or a test “sync executor”.
2. **Concurrency as library** — multi-task queues, timers, sockets live in `arandu_std::runtime`, optional like allocators.
3. **No implicit global executor** — see §2 (effect-handler / explicit Executor).

---

## 2. Gold decision 1 — Effect handler, not singleton

**Rule:** `spawn` / `block_on_multi` take an **explicit** `Executor` (value or type param), analogous to `Vec<T, A = GlobalAllocator>`.

```text
// Conceptual surface (SL_R implementation later)
func spawn<E: Executor, T>(ex: E, job: Coroutine[T]): TaskHandle[T]
func block_on<E: Executor, T>(ex: E, job: Coroutine[T]): T
```

Consequences:

| Goal | How |
|------|-----|
| Unit tests without OS reactor | Pass `SyncExecutor` (run one task to completion, no threads) |
| Multiple runtimes in one process | Different `Executor` values; no process-wide “the” runtime |
| Avoid Tokio-vs-async-std split | Libraries depend on `Executor` / `Reactor` **traits** in core or std.runtime.api, not a concrete global |

**Anti-pattern (forbidden):** `static RUNTIME: Tokio = …` assumed by every `spawn()`.

`stdlib/std/runtime.aru` today only scaffolds types so imports typecheck; no spawn yet.

---

## 3. Gold decision 2 — Reactor backends + runtime io_uring detect

Roadmap layout remains valid:

```text
std.runtime.reactor
  trait Reactor { … }
  epoll | kqueue | iocp | io_uring
```

**Decision:** prefer **io_uring when the running kernel supports it**, else epoll (Linux). Selection is **runtime**, not a single compile-time target that freezes an old binary to epoll-only forever *or* fails on old kernels.

Windows/macOS keep iocp/kqueue. Same pattern as CPU feature dispatch (A7).

---

## 4. Gold decision 3 — Task isolation vs abort

Language policy: **abort** (UD2/BRK), no stack unwinding (PAN). Therefore **`catch_unwind`-style recovery inside one process is not the model**.

Production promise (“one bad request does not kill the API”) maps to:

| Mechanism | Role |
|-----------|------|
| **SL_R.1 Supervisor** | Worker processes (or sandboxed threads with process restart policy) supervised by a parent; dead worker → restart |
| Generational abort | Still process-fatal **inside that worker** — correct, bounded blast radius |
| SyncExecutor tests | No isolation needed; single-threaded drive |

Name: **SL_R.1 — supervised worker isolation** (design item under SL_R, not A3).

---

## 5. Integration with A3 (checklist)

| A3 artifact | Runtime consumption |
|-------------|---------------------|
| `Coroutine[T]` state machine | Polled by Executor until `Poll.Ready` |
| `Suspend` / resume | Yield to scheduler; register waker with Reactor |
| `RelativeBorrow` / pin-free | Safe to move coroutine heap blob between queues |
| `block_on` (single) | Remains valid **without** `std.runtime` |
| `Future[T]` auto-impl (stdlib vision) | Trait for generic spawn bounds; compiler-generated for coroutines |

**Order constraint (unchanged):** SL_R **after** A3 — the runtime schedules objects A3 already produces; it does not define `await`.

---

## 6. Honesty — what is not SL_R yet

- No real `spawn`, no thread pool, no epoll loop in-tree.
- Multi-file **bodies** of imported std modules are not yet linked into JIT (signatures typecheck; pure path helpers need same-crate or body merge).
- `std.core.future` only defines `Poll` today; Waker/Context land with SL_R.

---

## 7. Recommended implementation slices (when coding SL_R)

1. **SL_R.0** — `Executor` + `SyncExecutor` + `block_on(ex, coro)` multi-poll API in std (still no OS).
2. **SL_R.2** — Linux epoll reactor + async sleep/demo.
3. **SL_R.3** — io_uring detect + fallback.
4. **SL_R.1** — supervisor process model (can parallelize with 2/3 as ops design).
