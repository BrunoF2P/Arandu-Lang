# Arandu Examples

These examples define the early test surface for the language.

## stable/syntax

Files in `examples/stable/syntax/` must parse according to `arandu-grammar-v0.6.ebnf`.

They are syntax fixtures first. They may mention standard-library modules such as `io` or `err`, but parser tests must not require those modules to exist.

## stable/semantics

Files in `examples/stable/semantics/` are intended to be semantically valid once the name resolver, type checker, standard-library contracts, and memory checker exist.

They should avoid deliberately invalid ownership, mutation, or error-handling behavior.

## stable/interop

Files in `examples/stable/interop/` are stable parser fixtures for extern/FFI syntax and basic ABI shape.

Full FFI type mapping is not specified yet, so these files should avoid relying on implicit conversions such as `str` to `ptr[u8]`.

## invalid

Files in `examples/invalid/` must contain an `expected:` comment describing the future parse, type, semantic, or memory error.

Syntax-invalid files should fail during parsing. Semantics-invalid files may parse successfully and fail in later compiler phases.

## draft

Files in `examples/draft/` are aspirational design sketches.

They are not parser fixtures. They may use syntax that does not exist in the EBNF yet, such as object literals, alternate lambda forms, or richer UI/web DSLs.
