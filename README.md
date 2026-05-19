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
- Type checker skeleton with primitive types, assignments, returns, fields, indexing, and basic diagnostics.

Not implemented yet:

- Complete type checker
- Generics instantiation
- Full stdlib/module loading
- Memory checker.
- Backend.

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

Run parse + name resolution:

```bash
cargo run -p arandu_cli -- check examples/stable/syntax/hello.aru
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
  arandu_lexer/   Rust lexer library
  arandu_parser/  Rust parser library
  arandu_semantics/ Name resolution and diagnostics
  arandu_cli/     Debug CLI for compiler experiments

docs/             Language and compiler design notes
examples/         Official stable, invalid, and draft examples
tests/lexer/      Lexer golden fixtures
tests/parser/     Parser golden fixtures
tests/semantics/  Semantics diagnostic fixtures
```

## Next Steps

1. Add filesystem module loading to name resolution.
2. Type checker skeleton.
3. Ownership and memory checker design.
4. Backend planning.
