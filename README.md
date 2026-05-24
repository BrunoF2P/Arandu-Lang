# Arandu

Arandu is an experimental Brazilian systems programming language focused on memory safety, clean syntax, explicit errors, and native tooling.

## Current Status

Implemented:

- Rust workspace.
- Lexer crate.
- Token stream CLI.
- Golden lexer tests.
- Smoke lexing for official stable and invalid examples.
- Parser crate with AST debug output for the current parser slice.
- Parser golden tests for declarations, generics, extern, match, interpolation, places, and expressions.
- Semantics crate with v0.2 name resolution, hierarchical symbol tables, namespace imports, prelude members, doc comment mapping, diagnostics, and CLI `check`.
- Type checker (experimental) with primitive types, assignments, returns, fields, indexing, and basic diagnostics.
- AHIR lowering and pretty-printing with golden tests (`tests/hir/`).
- AMIR lowering v0.1 (experimental) with CFG, locals, match, defer/errdefer, `?`/safe ops, for-in, alloc/free, and golden tests (`tests/amir/`).

Not implemented yet:

- `Result<T,E>` / `Option<T>` as first-class types (today: tuple-error heuristics)
- `self` receiver on methods
- Complete type checker (generics instantiation, interface satisfaction)
- Definite init, OSSA, move checker
- Memory checker / generational fallback
- Backend

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
cargo run -p arandu_cli -- amir tests/amir/add.aru
cargo run -p arandu_cli -- amir tests/amir/add.aru --debug
```

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
  arandu_lexer/     Rust lexer library
  arandu_parser/    Rust parser library
  arandu_semantics/ Name resolution, type checking, HIR, and AMIR
  arandu_cli/       Debug CLI for compiler experiments

docs/             Language and compiler design notes
examples/         Official stable, invalid, and draft examples
tests/lexer/      Lexer golden fixtures
tests/parser/     Parser golden fixtures
tests/semantics/  Semantics diagnostic fixtures
tests/hir/        AHIR golden fixtures (.aru → .hir)
tests/amir/       AMIR golden fixtures (.aru → .amir)
```

## Next Steps

See [docs/arandu-compiler-roadmap-v0.1.md](docs/arandu-compiler-roadmap-v0.1.md) — start with **v0.1-B** (`Result<T,E>` in the type checker), then **C** (`self`), **D** (AMIR on `Result`), then ownership passes.
