# Arandu — Public Comptime Contract

**Status:** scalar surface and additional value/staging cuts are implemented in
the 0.1.9 campaign. Complete campaign validation, RFC acceptance and release
readiness are tracked separately; this document does not certify those gates.

## Syntax and result

In a function body, `comptime` evaluates an admitted expression or
block during compilation and inserts its typed value into the residual program.
Dependent generic bodies are staged with a concrete instance environment.

```arandu
func add(a: int, b: int): int {
    return a + b
}

func main(): int {
    let answer: int = comptime add(20, 22)
    let same: int = comptime {
        let half = 21
        return half * 2
    }
    return answer
}
```

The expression form binds as a prefix operator: `comptime 20 + 22` stages only
`20`; use `comptime (20 + 22)` to stage the whole binary expression. Calls and
postfix operations belong to the prefix operand. The block form owns its scope
and return target. `return value` exits that CTFE root, not the runtime function.
All returns unify with a productive final expression, including syntactically
present unreachable returns/tails. `return;` and an empty block produce `void`.
A semicolon discards a final expression. No new `()` type spelling is introduced.
Every non-unit exit must have a result.

The admitted result types are `bool`, `int`, `uint`, `isize`, `usize`, `i8`,
`i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `byte`, `float`, `f32`, `f64`,
`str`, immutable literal-backed byte views, and `void`. Tuples, fixed arrays
and closed Copy structs may contain admitted values when their types have no
runtime destructor or unsupported resource. Enums, arbitrary references and
mutable/owned heap resources remain outside the value model. Contextual
typing chooses the numeric type before evaluation. Without a contextual type,
an integer result defaults to `int` and cannot silently narrow at a later use.
Pointer-sized types use the selected target layout; ordinary `int`/`uint` remain
32-bit. Materialization
preserves the full signed/unsigned range without lossy casts or VM handles.

## Captures, operations and limits

Locals declared inside the root belong to its CTFE frame. Ordinary global
constants are available; runtime locals and parameters outside the root cannot
be captured, even if initialized from literals. Pure helpers, including
registered imported modules and concrete generic instances, use independent
AMIR units. Retained operations/callees must be admitted; an untaken runtime
branch does not authorize an external call, allocation or unsupported type.

Integer arithmetic is checked. Overflow/invalid shifts use T046, and division
or remainder by zero uses T040. Resource exhaustion uses T045. Each root has
an allowance drawn from 1,000,000 fuel units and 1,000,000 value slots per
obligation family in a staged item/instance, with 128 frames per evaluation.
Explicit roots, static conditions, computed arguments and loop bounds are
separately bounded families; roots and generated occurrences within each family
share its allowance rather than each restarting the full budget.
At most 4,096 roots are allowed per item. Aggregate shapes/values also enforce
128 levels and 4,096 nodes, while allocation accounting bounds payloads and
backings independently of fuel. These limits do not measure compiler-wide RSS.
Cancellation unwinds analysis rather than caching
a language error. There is no wall-clock, filesystem, network or host-ABI
dependency in CTFE queries and no automatic fallback to runtime execution.

Unsupported result/intermediate types use T042; forbidden captures use T043
with declaration context. Other admission/evaluation failures use T044 with
bounded source traces. The formatter, semantic keyword classification,
completion and TextMate fallback recognize the public keyword. Themes still
choose colors.

## Public target layout expressions

`@sizeOf(Type)` and `@alignOf(Type)` inspect the selected target ABI and return
`usize`. They are type-operand expressions, not declaration annotations or
ordinary functions recognized by spelling. They use the same `LayoutEngine`
as the legacy intrinsic `mem.sizeOf<T>()` / `mem.alignOf<T>()` APIs.

```arandu
func size<T>(): usize {
    return @sizeOf(T)
}

func main(): usize {
    let bytes = comptime (@sizeOf([3]u16))
    return bytes + @alignOf(i64) + size<int>()
}
```

Size and alignment are different: i686 System V gives `i64` size 8 and alignment
4. Generic operand types are substituted before AMIR lowering; pre-body scalar
conditions and computed generic arguments can inspect concrete operand types
without requesting runtime AMIR. Ordinary functions named `sizeOf` or `alignOf`
retain their bodies. Unsupported/open nominal layouts and target object-size
overflow use T047; recursive by-value source definitions retain T029. Physical
layout and niche walks share the existing structural ceilings of 128 levels
and 4,096 visits, rather than resetting limits in recursive calls.

Expression-sized array-type grammar remains outside this cut. Concrete generic
instances can use layout queries in dependent pre-body selection without
requesting final runtime AMIR.

## Architecture

CST/AST preserves explicit staging roots. Initial item typing records local
obligations but does not invoke the interpreter. `item_staged_typing` in
`arandu_query` evaluates them through `ctfe_eval_root` with source-local ordinal
selectors and expected pool-independent types. Concrete environments carry
structural instance arguments, and generated occurrences retain their path.
This path does not request runtime units,
borrow contracts or final AMIR. CTFE helpers use initial typing, preventing a
recursive staging request from cycling through final runtime typing.

The evaluated values are pool-independent metadata keyed by full source spans
(including file identity), not arena IDs retained by another item revision.
HIR lowering uses the pure materializer; backends see ordinary literals,
aggregate constructors and the common AMIR only. Typed IEEE encodings and frozen
byte backing are shared value contracts, not per-backend evaluators.
Values and target layout participate in body fingerprints, not exported
declaration hashes. Items without explicit staging share the initial memo
without scanning the source arena. Regression tests cover sibling cutoff,
callee invalidation, target changes, arithmetic/capture failures, LSP UTF-16
and residual execution in C/Cranelift/Wasm at O0/O1/O2.

## Static branch selection

In function statement blocks, `comptime if` evaluates
its `bool` condition before resolving the body. Both branches must have valid
syntax, but only the selected branch is resolved, typed and lowered:

```arandu
func main(): int {
    comptime if true {
        return 42
    } else {
        return unavailableOnThisTarget()
    }
}
```

The condition uses the same VM, target layout, checked arithmetic and
budgets as other public roots. Global constants and pure helpers,
including imported helpers with static branches, are available. Runtime locals
and parameters are forbidden captures, including names that shadow global
constants or come from an enclosing loop/pattern. A failed or cyclic condition
selects neither branch and reports its own error, not errors from either body.

Selection is parent-first: nested conditions in discarded branches are never
evaluated, and discarded `comptime` expressions create no obligations. A
selected branch retains a normal lexical scope, with ownership, drops, `defer`,
`return`, `break` and `continue` handled by the existing HIR/AMIR pipeline.
Its body may perform ordinary runtime operations. `return` there returns from
the containing function, unlike a return inside a `comptime { ... }` root.
`else comptime if` is static; an ordinary `else if` remains runtime.

`item_static_branches` owns selection in `arandu_query`, using pre-body
`resolved_headers`/`header_signatures` and isolated condition/helper queries.
Its explicit revision-local decision map travels through resolution and typing.
HIR retains only a neutral lexical `Scope`, not a conditional or an `unsafe`
block. Backends receive the common AMIR without a static-selection opcode.
Body-only edits with unchanged decisions cut off selection consumers; exported
declaration hashes remain independent of those decisions.

Concrete generic instances select dependent conditions before resolving/typing
their residual body. Match arms retain their lexical scopes: runtime pattern
bindings cannot become CTFE inputs, although the selected runtime body can use
them. Lambda staging preserves lexical selection and capture checks, but general
closure typing and execution remain unsupported (U001), scheduled for 0.3;
this preparation does not enable executable lambdas in 0.1.9. Nested explicit
`comptime` roots remain unsupported and fail closed with T042.

## Explicit boundaries and future

### Value parameter spelling

`<comptime N: uint>` is a compatible spelling of `<const N: uint>`. Both lower
to the same value-parameter AST, symbol kind, `Const(u64)` instance argument
and existing monomorphizer. For example:

```arandu
func count<comptime N: uint>(): uint {
    return N
}

func main(): int {
    return (count<20>() + count<22>()) as int
}
```

The same parameters remain available in fixed array sizes (`[N]T`). Parameter
types and numeric bounds retain their existing rules: `uint` is 32-bit, so a
64-bit instance identity does not authorize a value outside that declared
range. Neither spelling introduces an ordinary runtime parameter, another
monomorphizer or negative constant keys.

### Computed generic arguments

In ordinary statement blocks of functions, an explicit
`comptime (expression)` can supply a constant generic argument. Parentheses
are required to separate expression operators and commas from the generic list.

```arandu
func add(a: int, b: int): int { return a + b }
func count<comptime N: uint>(): uint { return N }
func total<comptime N: uint>(values: [N]int): int {
    return (N as int) + values[0] + values[1]
}
func main(): int {
    let answer = count<comptime (add(20, 22))>()
    return total<comptime (1 + 2)>([19, 20, 0])
}
```

`item_const_arguments` freezes each value **before** ordinary body typing,
using isolated roots, body-free headers and pure helpers. The
pure checker reads the frozen argument and the existing monomorphizer receives
`Const(u64)`: `count<comptime (20 + 22)>()` and `count<42>()` share an instance.
No helper call for the argument computation survives in the runtime caller.
Values must be integers, non-negative and representable as `u64`; otherwise
T003 reports the invalid argument. Their expression type is inferred normally
(default `int`), not widened to the generic parameter type. Use an explicitly
typed helper for wider values. T011 also checks the declared parameter range
for computed **and literal** keys, including target-sized `usize`/`isize`.

The same target configuration, resource limits, checked arithmetic and capture
rules apply. Discarded static branches create no argument obligations. At most
4,096 computed arguments are staged per function. The revision-local frozen
map records failed obligations explicitly; missing values cannot fall back to
runtime. Its deterministic hash includes failures and diagnostics. Equal values
cut off consumers; header continuation can still revalidate root lowering after
a sibling edit, so this cut does not promise zero work for every source edit.

Declaration headers/defaults/aliases, condition and loop headers, nested
arguments and nested explicit `comptime` roots remain unsupported. Match arms
use the same lexical continuation and capture checks. Lambda scopes are prepared
likewise but remain subject to the U001 closure boundary described above. The
source-symbol helper API remains distinct from the concrete-instance API;
generic evaluation requires structural arguments rather than an unresolved
template. Expression-valued fixed array sizes remain future work.

Computed arguments inside generic function bodies are supported when independent
of that template's parameters: they freeze once per source item and target,
not once per runtime instance. For example, a `wrapper<T>` may call
`count<comptime (20 + 22)>()`. Dependent expressions such as `N + 1` are deferred
until a concrete instance supplies `N`; separate instances retain separate
decisions and frozen arguments without changing `FunctionInstance` identity.

Forwarding an existing constant parameter (`leaf<M>()`) must preserve its entire
non-negative domain. `M: u8` can flow into `N: u16`, but `M: u16` cannot flow
into `N: u8`, even if a particular caller supplies `1`. This rule prevents
truncation after specialization and uses target-sized bounds for `usize`/`isize`.
T011 labels both declarations and suggests compatible parameter domains.

### IDE values and generated regressions

Hover over a computed argument or scalar root shows `= 42` with an
`evaluated at compile time` caption. Hover over its generic callee keeps the
signature and appends those arguments' frozen values in source order. The pure
presentation lives in `arandu_ide`; the LSP only adapts Markdown and UTF-16
ranges. The AST links each value to its actual callee: no byte windows, nearby
call leakage, guessed result for failed/discarded obligations, or direct VM
execution in the presenter. Subsequent edits use current revision metadata.

T043 explains that runtime locals and parameters do not exist during compilation.
T045 fuel/frame exhaustion suggests checking recursion and loop termination;
value/allocation limits suggest reducing intermediate data instead.

Normal AranduSmith programs include bounded nested static trees, with independent
expected values and unavailable names in discarded branches. One in sixteen
seed keys also checks incremental analysis against clean analysis and requires
cutoff of VM evaluation after same-span discarded-code edits. Edits outside
the owning item preserve selection; displaced source spans require legitimate
revalidation, not stale-map reuse. Cache failures record seed/source artifacts
without pretending single-source shrinking can preserve an edit sequence.

`comptime` in constant declarations, nested explicit roots, arbitrary references
and helpers containing unmaterialized staging remain unsupported. Supported
aggregate/string/float values and concrete staging do not authorize general
metaprogramming, heap allocation, reflection or generated declarations.
Finite static expansion uses integer half-open ranges; inclusive ranges,
mutable iteration bindings and aggregate iteration domains remain unsupported.
Occurrence-dependent staging is implemented within the finite expansion
cut and does not imply arbitrary dependent domain evaluation. Ordinary
`if`/`while` inside a CTFE block are evaluated by the VM with both branches
resolved and typed; use the explicit static statement above for branch exclusion.

Configuration of public budgets, broader constant contexts and nested staging
remain explicit future work in the
[roadmap](arandu-compiler-roadmap-v0.1.md). Native Windows/macOS validation and
release readiness are separate gates from development-host tests.

### Integrated development-host evidence

On Linux x86-64, 2026-10-03, the six prescribed workspace gates passed in
order: formatting, locked check, all-target/all-feature Clippy with warnings
denied, workspace tests, diagnostic documentation and warning-free rustdoc.
The workspace run recorded 2,759 passed tests, zero failures and eight ignored
tests across 132 suites, including doc tests. Ignored tests are not claimed as
executed. Architecture, canonical LF and diagnostic determinism (one versus
eight threads) checks also passed.

The VS Code Extension Host passed both project and manifestless-file runs;
the extension's 26 unit tests, compilation and lint checks passed separately.
Differential tests exercise residual values and finite loops in C, Cranelift
and Wasm at O0/O1/O2. Query regressions cover concrete instances, cycles and
cutoff, including imported layout intrinsics after a sibling body changes
size. This evidence closes the five implementation cuts on the development
host, not native Windows/macOS CI, playground delivery or release promotion.

The final diff review also hardened Cranelift enum payload projections: malformed
field indices and offsets outside the emitter's range produce a reportable ICE
instead of falling back to field zero or truncating an offset. A deliberately
malformed AMIR regression covers both cases. Parser recovery tests keep the
statement-only `comptime if`/`comptime for` diagnostic aligned with the supported
value-producing block syntax.

## Frozen IEEE values and immutable text

The additional value cut admits `float`, `f32`, `f64`, UTF-8 `str` literals and
immutable byte views backed by those literals. `float` follows
the selected `DataLayout.float.size` (currently binary64 in every supported layout), while
`f32` and `f64` retain their IEEE formats. Decimal parsing converts directly to
the destination format. Addition, subtraction, multiplication, division and
numeric conversions use software arithmetic with nearest-ties-to-even rounding.
Float-to-integer casts truncate toward zero and reject NaN, infinity and values
outside the destination range. Float division by zero follows IEEE semantics;
integer division keeps its existing diagnostic.

Subnormal values, infinities and signed zero survive evaluation and
materialization. Arithmetic NaNs use the positive quiet canonical encoding;
numeric comparisons follow IEEE unordered behavior. Value/cache equality uses
the declared format and exact bits, so it distinguishes signed zeros and can
compare frozen NaNs. Remainder, FMA, transcendentals and fast-math remain outside
this cut. One decimal literal is bounded to 4,096 source bytes before invoking
the non-interruptible decimal conversion; VM fuel also charges literal input.

HIR and AMIR retain typed `FloatBits`, and each backend emits those encodings
directly: Cranelift IEEE immediates, Wasm float constants and C `memcpy` helpers.
Decimal display is presentation only. The AMIR and CGU fingerprints include
type/format/bits and use schema v4 to prevent reuse of older cached objects.

`ConstString`/`ConstBytes` share immutable owned UTF-8 backing plus a checked
logical offset/length. String views require codepoint boundaries; byte views
may start inside a codepoint, and conversion to a string validates UTF-8.
Equality and canonical identity use visible content, independently of the
original backing or offset. Unicode, empty strings and embedded NUL survive
freezing. Residual byte views intern their static backing in the destination
literal pool and lower through ordinary string/slice AMIR. Mutable backing,
runtime captures, owned `String`, interpolation and arbitrary pointers remain
outside the admitted value model.

### Software float dependency and cost evidence

`arandu_middle` uses the exact `rustc_apfloat = "=0.2.3"` dependency; the lockfile
selects `0.2.3+llvm-462a31f5a5ab`. `arandu_base` stays independent. The selected
crate is a Rust port of LLVM APFloat, with `no_std` plus `alloc`,
`forbid(unsafe_code)`, explicit rounding and no FFI/native LLVM dependency.
Its manifest declares `Apache-2.0 WITH LLVM-exception`; the included
`LICENSE.txt` and `LICENSE-DETAILS.md` document the LLVM relicensing review and
the additional port authors' agreements. Dependency upgrades require reviewing
those notices and the unstable upstream API again. These package properties
support portability; they do not replace native Windows/macOS validation or a
Wasm compiler build.

The reproducible cost probe is
`cargo run --locked -p arandu_middle --example ctfe_float_probe --release`.
It parses five ordinary/extreme decimal literals and executes dependent additions,
100,000 operations each, with `black_box`; host arithmetic is only the measured
baseline and never the evaluator. Five Linux x86-64 release runs on 2026-10-02,
excluding Rust compilation, gave these medians:

| Operation | Host baseline | Software |
| --- | ---: | ---: |
| 100,000 decimal conversions | 3.438 ms | 111.634 ms |
| 100,000 dependent additions | 0.282 ms | 5.287 ms |

The dependency adds measurable cost; its benefit is explicit target-independent
rounding and bit preservation rather than throughput. This microprobe is not a
whole-compiler CPU/RSS benchmark or a CI performance threshold. Focused
regressions cover direct f32 parsing versus double rounding, ties, signed zero,
subnormals, canonical NaNs, target layouts and residual backend encodings.
