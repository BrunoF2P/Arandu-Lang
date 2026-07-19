# Arandu

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE-MIT)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE-APACHE)

Arandu is an experimental Brazilian systems programming language focused on memory safety, clean syntax, explicit errors, and native tooling.

## Current Status

**Solidification gate (S5) closed** — foundation (DoD AMIR `TypeId`, spans, `DataLayout`, host C↔Cranelift parity, unified imports) is stable enough to resume language-level Fase 3 work. Details: [docs/arandu-solidification-matrix-v0.1.md](docs/arandu-solidification-matrix-v0.1.md).

**Product freeze (in progress):** [Arandu Minimal 0.1](docs/arandu-minimal-0.1-freeze.md) — stable installable surface before installer / project CLI / site.

Implemented:

- Rust workspace.
- Lexer crate.
- Token stream CLI.
- Golden lexer tests.
- Smoke lexing for official stable and invalid examples.
- Parser crate with AST debug output for the current parser slice.
- Parser golden tests for declarations, generics, extern, match, interpolation, places, and expressions.
- Semantics crate with v0.2 name resolution, hierarchical symbol tables, namespace imports, builtin prelude (`io` / `err` on the CLI path), doc comment mapping, diagnostics, and CLI `check`.
- Official `examples/stable/**` type-check via `arandu_cli check` (prelude + current semantics).
- Type checker v0.1 core with primitive types, assignments, returns, fields, indexing, generics constraints, interface satisfaction, `Result<T,E>`, `Option<T>`, nullable/safe operations, and diagnostics.
- AHIR lowering and pretty-printing with golden tests (`tests/hir/`).
- AMIR lowering v0.1 (experimental) with CFG, locals, match, defer/errdefer, `?`/safe ops, for-in, alloc/free, and golden tests (`tests/codegen/`).
- Dense AMIR types (`TypeId` on locals/temps), use-site spans on ownership diags, shared rvalue visitor.
- Method receivers with `shared self`, `mut self`, and `own self`.
- Definite initialization analysis with O008 diagnostics.
- OSSA foundation in AMIR: move/copy operands, storage lifetime markers, and destroy statements.
- Intraprocedural move checker with O001/O005/O007 diagnostics.
- Opt-in AMIR optimizer (`amir --opt`) with constant folding and DCE.
- Type interning, `DataLayout` (host / 32-bit / i686), and monomorphization graph infrastructure.
- Cranelift JIT backend (experimental, **host** dev/debug) with `run` CLI support.
- C emit path (`emit-c --layout=host|ptr4|i686`) — portable dump; not a polished embedded runtime yet.
- **ToStr v0.1** — auto-format `bool`, integers (incl. fixed-width), floats, `char`, and `str` in:
  - string interpolation (`"n=${n}"`)
  - call args whose formal type is `str` (e.g. `io.println(42)`)
  - method form `value.to_str()`
  - Prelude stays `(str) -> void` for `io.println`; host/C provide a debug `println` stub.
  - Formatted buffers use `malloc` (process-lifetime leak OK for debug; free/ownership later).
  - User `Display` / custom formatting for structs is later.
- **Salsa query DB** (`arandu_query`) — incremental `parse` → `resolve` → `type_check` → `lower_amir`; DX.5 `-Zexplain-rebuild`.
- **LSP gold** (`arandu-lsp`) — diagnostics, goto/hover/complete/signatureHelp/refs/rename/symbols, **type-aware semantic tokens**, **format**, **code actions** (quickfix `;`).  
- **CST-first** (rowan): `syntax_tree` → lower AST; reparse de subtree por ITEM; crate `arandu_fmt` + CLI `fmt`.

Not implemented yet:

- Memory checker / generational fallback
- Full user `Display` trait / custom `to_str` for structs/enums
- Full ownership surface syntax
- Production C polish / freestanding RT; LLVM release backend

**Compiler roadmap (single source of truth):** [docs/arandu-compiler-roadmap-v0.1.md](docs/arandu-compiler-roadmap-v0.1.md)

## Style Guide

Arandu has strong idiomatic casing rules, largely driven by the parser which can differentiate between value identifiers and type identifiers based on casing:

- **Values & Functions**: `camelCase` (e.g. `userName`, `totalPrice`, `buscarUsuario`, `parseJson`). This includes variables, parameters, functions, and struct fields.
- **Types & Structs**: `PascalCase` (e.g. `User`, `HttpClient`, `LoadState`). This includes structs, enums, interfaces, and type aliases.
- **Enum Variants**: `PascalCase` (e.g. `Ok`, `Err`, `Loading`, `NotFound`).
- **Generics**: Short `PascalCase` (e.g. `T`, `K`, `V`, `Item`).
- **Modules**: Lowercase dot-separated (e.g. `net.http`, `app.userService`).
- **Files**: `snake_case.aru` (e.g. `user_service.aru`).
- **Constants**: `SCREAMING_SNAKE_CASE` or `camelCase` (e.g. `MAX_RETRIES`, `maxRetries`).

*Note: `snake_case` is allowed for values but `camelCase` is the officially recommended and preferred style for all Arandu code.*

## Requirements

- Rust stable with edition 2024 support.

If your Rust toolchain is old, update it:

```bash
rustup update stable
```

## Language server

```bash
cargo run -p arandu_lsp --release
# point the editor at the `arandu-lsp` binary (stdio)
```

Architecture: [docs/arandu-salsa-lsp-architecture-v0.1.md](docs/arandu-salsa-lsp-architecture-v0.1.md).

## Run

Run all tests:

```bash
cargo test
```

Run the required lint gate:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Print tokens for the hello example:

```bash
cargo run -p arandu_cli -- lex examples/stable/syntax/hello.aru
```

Print the parser AST debug output:

```bash
cargo run -p arandu_cli -- parse examples/stable/syntax/hello.aru
```

Run parse + name resolution + type check:

```bash
cargo run -p arandu_cli -- check examples/stable/syntax/hello.aru
```

Print the AHIR (typed high-level IR):

```bash
cargo run -p arandu_cli -- hir examples/stable/syntax/hello.aru
cargo run -p arandu_cli -- hir examples/stable/syntax/hello.aru --debug
```

Print the AMIR (mid-level IR / CFG):

```bash
cargo run -p arandu_cli -- amir tests/codegen/add.aru
cargo run -p arandu_cli -- amir tests/codegen/add.aru --debug
cargo run -p arandu_cli -- amir tests/codegen/add.aru --opt
```

Run a program via the Cranelift JIT backend (exit code = `main` return value):

```bash
cargo run -p arandu_cli -- run tests/codegen/add.aru
```

Emit portable C (layout follows [`DataLayout`](docs/arandu-abi-layout-v0.1.md)):

```bash
cargo run -p arandu_cli -- emit-c examples/stable/syntax/fib_main.aru --layout=host
cargo run -p arandu_cli -- emit-c examples/stable/syntax/fib_main.aru --layout=i686
```

### Compiler instrumentation (`-Z` flags)

Unstable developer flags for profiling and debugging the compiler itself. Pass them before the subcommand:

```bash
cargo run -p arandu_cli -- -Ztime-passes check examples/stable/syntax/variables.aru
cargo run -p arandu_cli -- -Ztime-passes -Zprint-alloc-stats run tests/codegen/add.aru
```

| Flag | Effect |
|------|--------|
| `-Ztime-passes` | Print elapsed time per compiler pass (`parse+check`, `lower-hir`, `codegen`, …) |
| `-Zprofile-queries` | Print `TyCtx` binding cache hit/miss summary at the end |
| `-Zprint-alloc-stats` | Print `BumpArena` allocation totals at the end |
| `-Zdump-mir` | Dump MIR after passes (when wired in the pass pipeline) |

Output goes to **stderr** with `[arandu][perf]`, `[stat]`, `[mem]`, and `[info]` tags. See [docs/arandu-compiler-instrumentation-v0.1.md](docs/arandu-compiler-instrumentation-v0.1.md) for details.

Update golden test files (after intentional IR changes):

```bash
$env:UPDATE_GOLDEN=1; cargo test -p arandu_semantics
```

Parser fixtures:

```bash
cargo test -p arandu_parser
cargo run -p arandu_cli -- parse examples/stable/syntax/structs.aru
cargo run -p arandu_cli -- parse examples/stable/syntax/generics.aru
cargo run -p arandu_cli -- parse examples/stable/syntax/match.aru
```

## Project Structure

```text
crates/
  arandu_lexer/              Rust lexer library
  arandu_parser/             Rust parser library
  arandu_semantics/          Name resolution, type checking, HIR, and AMIR
  arandu_backend_cranelift/  Experimental Cranelift JIT backend
  arandu_cli/                Debug CLI for compiler experiments

docs/             Language and compiler design notes
examples/         Official stable, invalid, and draft examples
tests/lexer/      Lexer golden fixtures
tests/parser/     Parser golden fixtures
tests/semantics/  Semantics diagnostic fixtures
tests/hir/        AHIR golden fixtures (.aru → .hir)
tests/codegen/    AMIR golden fixtures (.aru → .amir)
tests/ui/         UI diagnostic fixtures (.aru → .diag)
```

## Next Steps

See [docs/arandu-compiler-roadmap-v0.1.md](docs/arandu-compiler-roadmap-v0.1.md). The next recommended technical milestones are the memory checker / generational fallback and production backends (C, LLVM).

## License

This project is dual-licensed under both the [MIT License](LICENSE-MIT) and the [Apache License, Version 2.0](LICENSE-APACHE).
