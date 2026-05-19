# Arandu Examples Validation v0.1

Status: manual validation checklist
Date: 2026-05-18
Depends on: `examples/README.md`, `docs/arandu-lexer-v0.1.md`, `docs/arandu-parser-v0.1.md`

## Goal

Define how examples are validated while the compiler is being built.

This document separates lexer, parser, checker, and draft expectations so examples do not accidentally become stronger contracts than the current implementation supports.

## Current Compiler Stage

Implemented now:

- Rust workspace.
- `arandu_lexer` crate.
- `arandu_cli lex <path>` debug command.
- Lexer golden tests under `tests/lexer/`.
- `arandu_parser` crate.
- `arandu_cli parse <path>` debug command.
- Parser golden tests under `tests/parser/`.
- `arandu_semantics` crate.
- Name resolution v0.2 with hierarchical symbol tables, namespace imports, prelude members, associated functions, doc comment mapping, and diagnostics.
- `arandu_cli check <path>` debug command for parse + name resolution.

Not implemented yet:

- Type checker.
- Memory checker.
- Backend.

## Implementation Matrix

| Feature | Lexer | Parser | Name Resolution | Type Check |
| --- | --- | --- | --- | --- |
| `module` / `import` | yes | yes | single-file namespace imports and named aliases; no file loading | no |
| `func` / `struct` / `enum` / `interface` / `extern` | yes | yes | yes, names, type references, and type-qualified associated funcs | no |
| attributes and doc comments | yes | attributes yes; doc comments attach to documentable nodes | attributes args resolved; docs exposed through `DocCommentMap` | no |
| generics / `where` | yes | yes | type parameter names and constraints | no |
| var declarations / `set` | yes | yes | locals, params, and `set` roots | no |
| `if` / `match` / patterns | yes | yes | names and pattern bindings | no |
| `for` / `while` | yes | yes | names only | no |
| `defer` / `errdefer` | yes | yes | names only | no |
| `unsafe` / `free` / `alloc` | yes | yes | names only | no |
| `catch` / `as` / `?` / `??` / safe access | yes | yes | names only | no |
| lambdas / arrays / block calls | yes | yes | names and lambda params | no |
| `examples/draft/**` | optional | optional | optional | optional |

## Validation Matrix

| Path | Lexer now | Parser now | Name resolver now | Checker later | Required now |
| --- | --- | --- | --- | --- | --- |
| `examples/stable/syntax/**/*.aru` | pass | pass | pass for current single-file/prelude subset | no | lexer + parser smoke |
| `examples/stable/semantics/**/*.aru` | pass | pass | partial | pass | lexer + parser smoke |
| `examples/stable/interop/**/*.aru` | pass | pass | partial until FFI/module loading exists | partial until FFI mapping exists | lexer + parser smoke |
| `examples/invalid/syntax/**/*.aru` | pass unless malformed token | fail | not reached | not reached | lexer smoke only |
| `examples/invalid/semantics/**/*.aru` | pass | pass | partial diagnostics only | fail | lexer + parser smoke |
| `examples/draft/**/*.aru` | optional | optional | optional | optional | no |

## Manual Checklist

1. Every file in `examples/stable/syntax/` should parse with the current parser.
2. Every file in `examples/stable/semantics/` should parse now and make semantic sense once the checker exists.
3. Every file in `examples/invalid/syntax/` should fail in the current parser.
4. Every file in `examples/invalid/semantics/` should parse, then fail in the checker or memory checker.
5. No file in `examples/draft/` should be part of mandatory tests in v0.1.

## Current Automated Checks

Run:

```powershell
cargo test
```

This verifies:

- lexer unit tests;
- lexer golden fixtures in `tests/lexer/`;
- `else` after `}` on the next line does not receive an inserted semicolon;
- nested braces inside string interpolation do not close interpolation early;
- smoke lexing for `examples/stable/syntax/`, `examples/stable/semantics/`, `examples/stable/interop/`, `examples/invalid/syntax/`, and `examples/invalid/semantics/`;
- no mandatory behavior for `examples/draft/`.

Also run:

```powershell
cargo test -p arandu_parser
```

This verifies:

- parser unit tests;
- parser golden fixtures in `tests/parser/`;
- parser contract fixtures in `tests/parser_contract/`;
- combined lexer+parser smoke traversal for `examples/stable/syntax/`, `examples/stable/interop/`, and `examples/invalid/syntax/`.

Also run:

```powershell
cargo test -p arandu_semantics
```

This verifies:

- name resolution unit and integration tests;
- forward references for top-level functions;
- local, param, `for`, and `match` pattern bindings;
- same-scope redeclaration diagnostics;
- undefined value/type diagnostics and suggestions;
- namespace imports and prelude members such as `io.println`;
- named import aliases by identifier casing;
- type-qualified associated function lookup;
- specific assignment-target diagnostics for `set missing = ...`;
- doc comment attachments surfaced through the semantic result.

Run:

```powershell
cargo run -p arandu_cli -- lex examples/stable/syntax/hello.aru
```

This prints the debug token stream for the hello example.

Run:

```powershell
cargo run -p arandu_cli -- check examples/stable/syntax/hello.aru
```

This runs parse + name resolution for the hello example.

## Future Checker Checks

When checker exists, add:

```powershell
arandu check examples/stable/semantics/**/*.aru
```

Expected result:

- all pass.

Add negative checker tests:

```powershell
arandu check examples/invalid/semantics/**/*.aru
```

Expected result:

- all fail with the error category described by each `expected:` comment.
