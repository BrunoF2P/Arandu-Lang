# Arandu Examples Validation v0.1

Status: manual validation checklist
Date: 2026-05-19
Depends on: `examples/README.md`, `docs/arandu-lexer-v0.1.md`, `docs/arandu-parser-v0.1.md`

## Goal

Define how examples are validated while the compiler is being built.

This document separates lexer, parser, checker, and draft expectations so examples do not accidentally become stronger contracts than the current implementation supports.

## Current Compiler Stage

| Component | Status |
| --- | --- |
| Lexer | implemented |
| Parser | implemented |
| Name resolver | implemented |
| Type checker | experimental |
| AHIR | implemented |
| AMIR | experimental |
| Memory checker | not implemented |
| Backend | not implemented |

Implemented now:

- `arandu_lexer` crate.
- `arandu_cli lex <path>` debug command.
- Lexer golden tests under `tests/lexer/`.
- `arandu_parser` crate.
- `arandu_cli parse <path>` debug command.
- Parser golden tests under `tests/parser/`.
- `arandu_semantics` crate.
- Name resolution v0.2 with hierarchical symbol tables, namespace imports, prelude members, associated functions, doc comment mapping, and diagnostics.
- Type checker (experimental) with primitive types, assignments, returns, fields, indexing, and basic diagnostics.
- AHIR lowering and pretty-printing with golden tests under `tests/hir/`.
- AMIR lowering v0.1 (experimental) with CFG and locals, golden tests under `tests/amir/`.
- `arandu_cli check <path>` debug command for parse + name resolution + type check.
- `arandu_cli hir <path>` debug command for AHIR pretty-printing.
- `arandu_cli amir <path>` debug command for AMIR pretty-printing.

Not implemented yet:

- Complete type checker (generics instantiation, full constraint solving).
- AMIR match/defer/try/catch/safe-access lowering.
- Memory checker.
- Backend.

## Implementation Matrix

| Feature | Lexer | Parser | Name Resolution | Type Check | AHIR | AMIR |
| --- | --- | --- | --- | --- | --- | --- |
| `module` / `import` | yes | yes | single-file namespace imports and named aliases; no file loading | no | yes | n/a |
| `func` / `struct` / `enum` / `interface` / `extern` | yes | yes | yes, names, type references, and type-qualified associated funcs | experimental | yes | experimental (funcs only) |
| attributes and doc comments | yes | attributes yes; doc comments attach to documentable nodes | attributes args resolved; docs exposed through `DocCommentMap` | no | yes | n/a |
| generics / `where` | yes | yes | type parameter names and constraints | no | yes | n/a |
| var declarations / `set` | yes | yes | locals, params, and `set` roots | experimental | yes | yes |
| `if` / `while` | yes | yes | names and pattern bindings | experimental | yes | yes |
| `match` / patterns | yes | yes | names and pattern bindings | experimental | yes | **not yet** |
| `for` | yes | yes | names only | no | yes | experimental |
| `defer` / `errdefer` | yes | yes | names only | no | yes | **not yet** |
| `unsafe` / `free` / `alloc` | yes | yes | names only | no | yes | partial |
| `catch` / `as` / `?` / `??` / safe access | yes | yes | names only | no | yes | **not yet** |
| lambdas / arrays / block calls | yes | yes | names and lambda params | no | yes (arrays), partial (lambdas) | arrays yes, lambdas **not yet** |
| `examples/draft/**` | optional | optional | optional | optional | optional | optional |

## Validation Matrix

| Path | Lexer now | Parser now | Name resolver now | Type checker now | AHIR now | AMIR now | Required now |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `examples/stable/syntax/**/*.aru` | pass | pass | pass for current single-file/prelude subset | experimental | pass | partial | lexer + parser smoke |
| `examples/stable/semantics/**/*.aru` | pass | pass | partial | experimental | partial | partial | lexer + parser smoke |
| `examples/stable/interop/**/*.aru` | pass | pass | partial until FFI/module loading exists | partial | partial | n/a | lexer + parser smoke |
| `examples/invalid/syntax/**/*.aru` | pass unless malformed token | fail | not reached | not reached | not reached | not reached | lexer smoke only |
| `examples/invalid/semantics/**/*.aru` | pass | pass | partial diagnostics only | experimental | partial | n/a | lexer + parser smoke |
| `examples/draft/**/*.aru` | optional | optional | optional | optional | optional | optional | no |

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
- type checker golden diagnostics;
- AHIR golden tests in `tests/hir/`;
- AMIR golden tests in `tests/amir/`;
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
cargo run -p arandu_cli -- hir examples/stable/syntax/hello.aru
```

This prints the AHIR pretty-printed representation.

Run:

```powershell
cargo run -p arandu_cli -- amir tests/amir/add.aru
```

This prints the AMIR pretty-printed representation.

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
