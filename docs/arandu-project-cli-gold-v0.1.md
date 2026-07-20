# Arandu Project CLI — Gold Bars (P2)

**Status:** implemented (2026-07-19)  
**Binary:** `arandu_cli` (install may symlink as `arandu`)  
**Tests:** `cli_project_gold`, `test_new_project_template_parses_cleanly`, unit tests in `arandu_query::{manifest,stdlib}`

This document freezes the decisions that are **expensive to reverse** after users adopt the CLI. It is the product counterpart of the Minimal freeze language surface.

---

## 1. Value order (why these five)

| # | Gold bar | Why it is gold |
|---|----------|----------------|
| 1 | Stdlib via `current_exe()`, never cwd | Classic silent install bug (Zig early / Nim-ish paths); symlink + PATH must work |
| 2 | `arandu doctor` | 80% of “why doesn’t it work?” onboarding |
| 3 | Manifest as Salsa input from day 1 | Avoids painful migration when `Arandu.toml` grows deps/workspace |
| 4 | Template in stdlib-style parse CI | First `arandu new` experience must not be where syntax bugs are found |
| 5 | `run` prints `[cached]` / `[rebuilt: N queries]` | Free DX.5 surface; Cargo’s “did it rebuild?” frustration |

Plus: `build` = Cranelift, `build --release` = LLVM (reserved) — dual-backend convention fixed before scripts depend on flags.

---

## 2. Stdlib resolution cascade

**Never** relative to process cwd for the *install* path. Priority:

1. `--stdlib-path=<dir>` (CLI, always wins)  
2. `ARANDU_STDLIB` environment variable  
3. Relative to `std::env::current_exe()`:  
   - install layout: `../share/arandu/stdlib` (binary in `$PREFIX/bin`)  
   - monorepo walk: parents of the executable until a valid `stdlib/` root is found  
4. **Hard error** with a clear “tried:” list — never silently pick a stale tree from another install

Valid root = directory containing `std/` or `core/`.

Import keys remain `stdlib/std/io.aru` etc.; on disk that maps to `$STDLIB_ROOT/std/io.aru`.

**Package discovery** (finding `Arandu.toml`) may walk from cwd/path — that is Cargo’s convention and is *not* the same bug as stdlib-from-cwd.

Code: `arandu_query::stdlib::resolve_stdlib_root`, wired in `DatabaseImpl::set_stdlib_root` / `resolve_module_path`.

### Install layout (P2.5 packaging — versioned, rustup-style)

```text
$PREFIX/
  arandu-0.0.1/
    bin/arandu_cli
    bin/arandu → arandu_cli
    share/arandu/stdlib/{std,core,alloc}/
    BLAKE3SUMS
  arandu-0.0.2/ …
  current → arandu-0.0.2          # atomic ln -sfn
  bin/arandu → ../current/bin/arandu
```

**Hard rules:**

1. `current_exe()` is always **canonicalized** before `../share/arandu/stdlib` (PATH symlinks safe).  
2. Install stages under `$PREFIX/.staging/…`, then `mv` + `ln -sfn` (no half-written final tree).  
3. Tarball ships with `.blake3` sidecar; `install-from-tarball.sh` refuses on mismatch.  
4. Smoke: `./scripts/smoke-install.sh` — clean tmp prefix, `ARANDU_STDLIB` unset, PATH symlink, cwd outside monorepo.

Scripts: `install-local.sh`, `package-release.sh`, `install-from-tarball.sh`, `smoke-install.sh`.  
CI job: `install-smoke`.

---

## 3. `Arandu.toml` + Salsa

MVP fields (strings only):

```toml
name = "hello"
version = "0.0.1"
entry = "src/main.aru"
```

- Malformed TOML / missing required field / non-UTF-8 → **hard error** (BUG-09; never swallowed).  
- Tables/sections rejected in MVP so future `[dependencies]` cannot be silently ignored.  
- `ProjectManifest` is a `#[salsa::input]` with `name`, `version`, `entry`, **BLAKE3 content_hash** of raw bytes, and path.  
- `manifest_fingerprint` tracked query pins the edge in the graph from day 1.

Code: `arandu_query::manifest`.

---

## 4. Commands

```text
arandu_cli new <name>
arandu_cli doctor [--stdlib-path=…] [-v]
arandu_cli check [package-path]     # package mode if Arandu.toml
arandu_cli run   [package-path]     # prints [cached]|[rebuilt: N queries]
arandu_cli build [--release] [path] # Cranelift; --release → LLVM reserved (exit 2)
arandu_cli check|run|… <file.aru>   # legacy single-file mode
```

### Backend convention (roadmap 4.1)

| Invocation | Backend |
|------------|---------|
| `build` / `run` | Cranelift (dev JIT) |
| `build --release` | LLVM when implemented — **flag meaning frozen now** |

### Rebuild status line

`run` (and package `check`/`build`) enable DX.5 `RebuildLog` and print one word-line on stderr before execution:

```text
[rebuilt: 12 queries]
[cached]
```

---

## 5. `arandu doctor`

Flutter-style health report (categories + nested bullets + summary):

```text
Doctor summary (to see all details, run arandu_cli doctor -v):

[✓] Arandu toolchain (v0.0.1)
[✓] Stdlib
[-] Project (Arandu.toml)
    • no package found from …
[✓] Cranelift backend (dev JIT)
[-] LLVM backend (release)
    • not implemented yet

• No issues found!
```

Reuses real init points: `current_exe`, stdlib cascade, manifest parse (BUG-09), Cranelift `try_new()`.  
`doctor -v` expands paths, cascade, host triple, content hash.

---

## 6. Template CI

| File | Role |
|------|------|
| `examples/minimal/TEMPLATE_main.aru` | Canonical template source |
| `arandu_cli::project::TEMPLATE_MAIN_ARU` | Embedded for `new` (keep in sync) |
| `test_new_project_template_parses_cleanly` | Parser contract CI |

---

## 7. Explicitly not in this gold pass

| Item | Track |
|------|--------|
| Package-local multi-module (`import my_app.util`) | `PROMOTE-L2` |
| Disk-persistent Salsa cache across processes | future CACHE/DET |
| LLVM release backend body | roadmap dual-backend |
| Install script / local tarball + BLAKE3 | **done** (P2.5 scripts + `install-smoke` CI) |
| GitHub Release on `v*` tags | **done** (`.github/workflows/release.yml`) |
| Marketing site | after packaging |

---

## 8. Watch mode (shared VFS debounce)

`arandu_cli watch` re-checks a package when the filesystem changes.

| Layer | Implementation |
|-------|----------------|
| Debounce core | `arandu_query::DebouncedMap` (same for LSP + CLI) |
| Text edits (LSP) | `arandu_query::EditVfs` ← re-exported as `arandu_lsp::vfs::Vfs` |
| FS events (CLI) | `WatchBuffer` + `PackageWatchSession` |
| OS watcher | `notify-debouncer-full` (rename correlated as one event) |
| Commit | one Salsa revision: listing + registry + `set_text` / unregister |

**Guarantees (tested without OS notify):**

1. Debounce: no commit inside quiet window  
2. Rename: single commit (no transient orphan Remove)  
3. Delete of imported module → **M001**, not silence  
4. `Arandu.toml` `name` change → full local import invalidation  

```bash
cargo test -p arandu_query --test watch_session_l2
arandu_cli watch   # package cwd
```

## 9. Smoke checklist

```bash
cargo test -p arandu_cli --test cli_project_gold
cargo test -p arandu_parser --test parser_contract test_new_project_template
cargo test -p arandu_query --lib   # manifest + stdlib units
cargo test -p arandu_query --test watch_session_l2

arandu_cli doctor
arandu_cli new demo && cd demo && arandu_cli check && arandu_cli run
arandu_cli watch   # after edits under src/
```
