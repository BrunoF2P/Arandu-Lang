# Arandu Mid-level Intermediate Representation (AMIR) v0.1

AMIR v0.1 is a Control Flow Graph (CFG) representation of the program. It flattens the high-level syntax structures (like nesting and structured control flow) into linear lists of instructions grouped in Basic Blocks and linked by jumps.

## Core Concepts

1. **Locals and Temporary Registers**:
   - All function parameters, local variables, and intermediate expression values are represented as numbered registers: `_0`, `_1`, `_2`, etc.
   - By convention:
     - `_0` is the register designated for the function's return value.
     - `_1` to `_N` are registers designated for the function's incoming parameters in order.
     - `_N+1` onwards represent local variables and intermediate evaluation temporaries.
   - Every register has a concrete `ArType`.

2. **Basic Blocks**:
   - A basic block (`bb0`, `bb1`, etc.) is a straight-line sequence of statements that executes from start to end without branching.
   - Every basic block ends with a single **Terminator** instruction.

3. **Terminators**:
   - `Return`: Exits the function, passing the value in the return register `_0`.
   - `Goto(bbX)`: Unconditionally transfers control to basic block `bbX`.
   - `Branch { condition, if_true: bbX, if_false: bbY }`: Boolean conditional branch; used for `if`, `while`, and C-style `for` conditions.
   - `SwitchInt { discriminant, targets: [(value, bbX), ...], otherwise: bbY }`: Integer discriminant switch (reserved for `match` on integers / enum tags in a future version).
   - `Unreachable`: Denotes code that cannot be executed.

4. **Statements and Rvalues**:
   - `Assign(lhs, rvalue)`: Evaluates an rvalue and assigns it to a register.
   - `Call(lhs, callee, args)`: Invokes a function or callable, writing the result to `lhs` (if any).

## Textual Pretty-Printing Contract

AMIR outputs conform to a deterministic, indented format:
1. Two spaces (`  `) are used for code blocks.
2. The `locals` section lists all registers defined in the function with their types and names (if they correspond to a source variable).
3. Basic blocks are printed as `bbX:` followed by statements and a terminator.

### Example: Basic Addition
Source:
```swift
func add(a int, b int) int {
    return a + b
}
```

AMIR Output:
```text
Func add(a: int, b: int) -> int
  locals:
    _0: int // return
    _1: int // a
    _2: int // b

  bb0:
    _0 = add _1, _2
    return
```

### Example: Branching (If/Else)
Source:
```swift
func test(x int) int {
    if x > 5 {
        return 10
    } else {
        return 20
    }
}
```

AMIR Output:
```text
Func test(x: int) -> int
  locals:
    _0: int // return
    _1: int // x
    _2: bool // temp for comparison

  bb0:
    _2 = gt _1, 5
    switchInt _2 { 1 => bb1, otherwise => bb2 }

  bb1:
    _0 = 10
    return

  bb2:
    _0 = 20
    return
```

### Example: While Loop
Source:
```swift
func test() {
    idx = 0
    while idx < 5 {
        set idx = idx + 1
    }
}
```

AMIR Output:
```text
Func test() -> void
  locals:
    _0: void // return
    _1: int // idx
    _2: bool // temp for comparison
    _3: int // temp for add

  bb0:
    _1 = 0
    goto bb1

  bb1:
    _2 = lt _1, 5
    switchInt _2 { 1 => bb2, otherwise => bb3 }

  bb2:
    _3 = add _1, 1
    _1 = _3
    goto bb1

  bb3:
    return
```

## Lowering from AHIR to AMIR

The AMIR lowering compiler pass performs the following steps:
1. Pre-allocates parameters (`_1.._N`) and maps their symbol IDs in a translation context.
2. Creates an initial basic block `bb0`.
3. Processes declarations and statements sequentially.
4. For expressions:
   - Constants and direct references to variables are treated as simple operands (constants or copies of registers).
   - Nested operators (e.g. `a + b * c`) are flattened: inner operators are evaluated into temporary registers, and these temporaries are used as operands in subsequent statements.
5. For structured statements:
   - `If`: Allocates the conditional evaluation block, branches into a `then` block and an `else` block (each ending with a jump to a joint `bb_exit` block).
   - `While`: Jumps to a condition evaluation block, which checks the condition and jumps either to the loop body block or the loop exit block. The loop body block ends with an unconditional jump back to the condition block.
