# Arandu backend contract v0.1

**Status:** S1-C contract; audited 2026-08-20.
**Scope:** the public boundary from validated AMIR to C source or host JIT code.

This document is intentionally conservative. A green frontend test means that a
program reaches AMIR; it does not automatically promote every runtime facility,
target ABI, or external toolchain to supported backend status.

## Target model

LLVM associates a target triple with a data layout, while GCC and Clang also
require ABI, CPU, object format, system headers, libraries, and linker choices.
Arandu v0.1 models only the layout subset below. Consequently, `ptr4` and `ptr8`
are layout profiles, not target triples and not cross-toolchain configurations.

| Profile | Pointer / `int` | `float` | `i64` / `f64` ABI alignment | Contract |
|---|---:|---:|---:|---|
| `host` | host width | IEEE f64 | host profile | Required for C ↔ Cranelift parity. |
| `ptr4` | 4 bytes | IEEE f64 | 8 bytes | Generic ILP32-style layout model; emit-only and experimental. |
| `ptr8` | 8 bytes | IEEE f64 | 8 bytes | Generic LP64-style layout model; emit-only unless it equals the host. |
| `i686` | 4 bytes | IEEE f64 | 4 bytes | i686 SysV data-layout model; emit-only and tested structurally. |

All profiles reject objects at or above the positive `isize` bound of the
modeled address space. Layout arithmetic is checked and a failure returns a
typed diagnostic before a successful artifact is exposed.

## Backend and platform matrix

| Capability | C backend | Cranelift backend |
|---|---|---|
| Product | One GNU C translation unit | Native code held in the current process |
| Execution model | External compiler and linker | JIT only |
| Target selection | Explicit `DataLayout`; no triple | Host ISA, ABI, pointer width, and calling convention |
| Cross-target status | Source emission only; caller supplies matching compiler, flags, sysroot, libc/runtime, and linker | Unsupported |
| C dialect/toolchain | GCC or Clang GNU extensions; MSVC is unsupported | Not applicable |
| Validated CI hosts | Host parity on the S0 Linux gate; install smoke on Linux and macOS | Host tests on the S0 Linux gate; release/install smoke does not prove all JIT runtime paths |
| Invalid AMIR | Shared validator runs before emission | Shared validator runs before JIT mutation |
| Backend failure | `Err(Diagnostic)`; no successful partial source | `Err(Diagnostic)`; no successful partial module |

Windows builds are locally supported by the Rust workspace, but Windows backend
Gold requires a dedicated mandatory CI runner and runtime campaign. Until that
exists, this contract does not claim Windows backend Gold.

## Type and representation matrix

| Family | Representation | C | Cranelift host JIT |
|---|---|---|---|
| `bool`, bytes, chars, fixed integers | fixed-width scalar | Supported | Supported |
| `int`, `uint` | target pointer-width scalar | Supported by selected layout | Supported at host width |
| `float`, `f32`, `f64` | IEEE scalar; language `float` is f64 | Supported | Supported |
| pointers, references, borrows | pointer plus AMIR ownership rules | Supported after borrow lowering | Supported after borrow lowering |
| arrays, slices, `str` | inline array or `{ptr,len}` fat pointer | Supported; target layout applies | Supported on host layout |
| tuples and structs | ordered fields plus ABI padding | Supported | Supported |
| enums, `Option`, `Result` | pointer-width tag plus payload | Supported | Supported |
| functions and `extern "C"` | declared ABI boundary | Partial: caller must provide compatible symbols | Partial: only registered host/runtime symbols are linkable |
| allocation and `free` | libc/runtime calls | Hosted runtime only | Host runtime only |
| string interpolation / primitive `ToStr` | runtime helpers | Supported for primitives and `str` | Supported for primitives and `str` |
| user-defined formatting | no stable ABI yet | Unsupported | Unsupported |
| async/await | current ready/poll runtime model | Partial/experimental | Partial/experimental host runtime |
| OS, sockets, supervisor, generational runtime | runtime-specific symbols | Partial; not a freestanding contract | Experimental host implementation |

“Supported” here means covered at the backend boundary for well-typed AMIR. It
does not override language-phase status or promise a stable external ABI.

## Rejection and artifact rules

1. Both backends run `validate_amir_program` before backend work.
2. Malformed SSA edges, poison types, and invalid statement ranges must produce
   the same `ICE-GEN-002` diagnostic in both backends.
3. A valid AMIR construct that was not lowered to the backend contract produces
   `ICE-GEN-001`; it must never be replaced by a successful placeholder.
4. A source-language feature deliberately unavailable to users produces its
   documented user diagnostic before code generation.
5. C source is returned only after the entire translation succeeds. Cranelift
   returns a module only after definitions are finalized successfully.

## What is required for real cross compilation

Adding a new Gold target requires one named target specification containing at
least architecture, OS/environment, ABI/calling convention, endianness, object
format, data layout, runtime availability, and linker/toolchain configuration.
It also requires compile, link, and execution tests on that target or an
explicitly documented emulator. Adding another pointer-width alias is not
sufficient.

Primary references used for this decision:

- [LLVM Language Reference: target triple and data layout](https://llvm.org/docs/LangRef.html)
- [Clang cross-compilation guide](https://clang.llvm.org/docs/CrossCompilation.html)
- [GCC target-specific options](https://gcc.gnu.org/onlinedocs/gcc/Target-Specific-Options.html)
- [GCC ABI compatibility scope](https://gcc.gnu.org/onlinedocs/gcc/Compatibility.html)
- [Cranelift IR invariants](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md)

## Promotion criteria

The C backend becomes Gold for a named target only when its generated source is
compiled and linked with the declared toolchain and runtime, then executed in a
mandatory CI job. The Cranelift backend becomes Gold on a host only after that
host runs the JIT parity and runtime suites as required checks. Cross-target
object emission, LLVM release codegen, freestanding C, and a stable public ABI
remain separate future milestones.
