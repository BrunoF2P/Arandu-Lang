# Arandu — Async Runtime Design (SL_R) v0.1

**Status:** design lock + **SL_R.0 + SL_R.2 implemented** (typed Coroutine spawn/join/block_on, SyncExecutor, EpollReactor with timerfd). SL_R.1 supervisor and SL_R.3 io_uring still open. Consumes A3 compiler semantics.

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
2. **Concurrency as library** — multi-task queues, timers, sockets live in `std.runtime`, optional like allocators.
3. **No implicit global executor** — see §2 (effect-handler / explicit Executor).

---

## 2. Gold decision 1 — Effect handler, not singleton

**Rule:** `spawn` / `block_on` take an **explicit** executor (value), analogous to `Vec<T, A = GlobalAllocator>`.

```text
// Implemented surface (int payload MVP; full generic T follows mono/codegen)
func block_on_int(shared ex: SyncExecutor, job: Coroutine<int>): int
func spawn_int(shared ex: SyncExecutor, job: Coroutine<int>): TaskHandle
func join_int(shared ex: SyncExecutor, handle: TaskHandle): int
```

Consequences:

| Goal | How |
|------|-----|
| Unit tests without OS reactor | Pass `SyncExecutor` (run one task to completion, no threads) |
| Multiple runtimes in one process | Different `SyncExecutor` / `EpollReactor` values; no process-wide “the” runtime |
| Avoid Tokio-vs-async-std split | Libraries depend on explicit handles, not a concrete global |

**Anti-pattern (forbidden):** `static RUNTIME: Tokio = …` assumed by every `spawn()`.

**ABI bridge:** at runtime `Coroutine[T]` is a state-blob pointer (A3). Typeck allows `job as ptr[u8]` so host `ar_rt_*` receives the blob without erasing the language type at call sites in user code (stdlib does the cast once).

---

## 3. Gold decision 2 — Reactor backends + runtime io_uring detect

```text
std.runtime
  EpollReactor          // SL_R.2 — epoll + timerfd (Linux); portable sleep fallback
  // future: kqueue | iocp | io_uring (SL_R.3 runtime detect)
```

**Decision:** prefer **io_uring when the running kernel supports it**, else epoll (Linux). Selection is **runtime**, not a single compile-time target that freezes an old binary to epoll-only forever *or* fails on old kernels.

Windows/macOS keep iocp/kqueue. Same pattern as CPU feature dispatch (A7).

**SL_R.2 shipped:**
- Host: `ar_rt_reactor_create/destroy/sleep_ms/arm_timer_ms/poll_ms`
- Language: `EpollReactor`, `new_epoll_reactor`, `reactor_sleep_ms`, `reactor_arm_timer_ms`, `reactor_poll_ms`, `destroy_reactor`

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
| `Coroutine[T]` state machine | Polled by Executor until Ready (`block_on_int` / `join_int`) |
| `Suspend` / resume | Yield to scheduler; timer registration with Reactor (arm/poll) |
| `RelativeBorrow` / pin-free | Safe to move coroutine heap blob between queues |
| `block_on` (single) | Remains valid **without** `std.runtime` (`await`) |
| Typed `spawn_int` | Takes `Coroutine<int>` from `async func` / `async {}` |

**Order constraint (unchanged):** SL_R **after** A3 — the runtime schedules objects A3 already produces; it does not define `await`.

---

## 6. Honesty — what is done vs what remains

**Done:**
- SL_R.0: SyncExecutor, TaskHandle, typed `spawn_int` / `join_int` / `block_on_int`, low-level `*_i64` over `ptr[u8]`
- Multi-file HIR link; import typeck without diagnostic leak into entry
- SL_R.2: EpollReactor (Linux epoll + timerfd; portable sleep fallback), sleep/arm/poll demos
- Coroutine → `ptr[u8]` cast (ABI truth)

**Not yet:**
- Generic `spawn<T>` / non-int payloads
- Thread pool, Waker/Context trait surface, socket I/O
- SL_R.3 io_uring detect + fallback
- SL_R.1 supervisor process model

---

## 7. Remaining implementation slices

1. **SL_R.0 done** — typed Coroutine + SyncExecutor.
2. **SL_R.2 done** — EpollReactor + timer sleep/poll.
3. **SL_R.3** — io_uring detect + fallback.
4. **SL_R.1** — supervisor process model (can parallelize with 3 as ops design).
