# Arandu Examples

These examples define the early test surface for the language.

## Validation levels

| Path | Required today |
|------|----------------|
| `examples/stable/**/*.aru` | **parse + `arandu_cli check`** (CLI / Salsa path) |
| `examples/invalid/**` | Fail at the phase described by `expected:` comments |
| `examples/draft/**` | Aspirational only — no check gate |

CI and `crates/arandu_cli` smoke tests gate **stable** with `check`.

## stable/syntax

Files in `examples/stable/syntax/` must parse according to `arandu-grammar-v0.6.ebnf`
and **type-check cleanly** with the current compiler.

They may use builtin prelude modules `io` and `err` (`import io`, `import err`).
Those are compiler-injected stubs (not on-disk `stdlib` files yet).

## stable/semantics

Files in `examples/stable/semantics/` are **semantically valid** under the current
type checker and prelude stubs. They should avoid APIs that are not yet modeled
(e.g. full filesystem `io.create` / file handles — those live under `draft/`).

## stable/interop

Files in `examples/stable/interop/` cover `extern "C"` / FFI shape.
Calls to `extern` functions must sit inside `unsafe { ... }` (O013).

Full ABI and linking are still incomplete; these examples only need to **check**.

## invalid

Files in `examples/invalid/` must contain an `expected:` comment describing the
future parse, type, semantic, or memory error.

Syntax-invalid files should fail during parsing. Semantics-invalid files may
parse successfully and fail in later compiler phases.

## draft

Files in `examples/draft/` are aspirational design sketches.

They are **not** part of the `check` gate. They may use incomplete stdlib
surfaces (e.g. file IO), syntax not yet in EBNF, UI/web DSLs, or async sketches.
