# JIT memory policy (v0.1 / debug Cranelift)

**Status:** intentional process-lifetime allocation for the **host debug JIT**.  
**Not** a production GC or ownership runtime.

## What allocates

| Source | Allocator | Lifetime |
|--------|-----------|----------|
| String literals in data section | module data | until `CompiledModule` drop |
| `ToStr` / string interp / `err.new` | `malloc` | process (leak OK in debug) |
| Struct / enum construct | `malloc` | process |
| Boxed `int?` / scalar `T?` | `malloc` | process |

## Guarantees (v0.2 Dev/Debug)

1. **Correctness over reclaim**: handles and fat pointers remain valid for the duration of the process after `run`.
2. **No double-free** in the happy path: JIT does not free user values yet; poison-on-free in debug is reserved for future ownership passes (OSSA M2).
3. **ABI**: `T?` is a null-or-pointer handle; scalars are boxed so payload `0` ≠ `nil`.

## Planned (not this doc)

- Arena tied to `CompiledModule` with batch free on drop, **or** bump allocator (`bumpalo`) per compile unit.
- OSSA-driven `Destroy` / `free` when ownership analysis is complete.

## C backend

`emit-c` may emit `malloc`/`free` stubs for portability; host C parity tests do not claim a freestanding allocator yet.
