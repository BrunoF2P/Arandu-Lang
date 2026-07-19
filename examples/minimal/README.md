# Arandu Minimal 0.1 — gold examples

These programs define the **installable** language surface.  
See `docs/arandu-minimal-0.1-freeze.md`.

## Contract

Every file here must pass:

```bash
arandu_cli check <file>
arandu_cli run   <file>   # exit code documented in CLI tests
```

**Do not** add examples that require experimental APIs (reactor, TCP, supervisor, dyn, …).

## Index

| File | Exit | Covers |
|------|------|--------|
| `m01_hello.aru` | 0 | prelude io, main |
| `m02_structs_enums.aru` | 3 | struct, enum, match |
| `m03_result_option.aru` | 7 | Result/Option sugar |
| `m04_generics_bounds.aru` | 10 | free-function generics (mono) |
| `m05_borrow_shared.aru` | 5 | shared self method |
| `m06_async_await.aru` | 42 | async func + await |
| `m07_async_spawn_join.aru` | 42 | std.runtime spawn/join |
| `m08_modules/main.aru` | 9 | multi-file via std (path+runtime) |
| `m09_interp_tostr.aru` | 0 | string interpolation |
| `m10_path_empty.aru` | 0 | std.path thin IN |

## Default template (installer)

See `TEMPLATE_main.aru` — only IN surface.
