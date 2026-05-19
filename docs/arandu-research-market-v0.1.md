# Arandu Research and Market Notes v0.1

Status: foundation note  
Date: 2026-05-18  
Project direction: safe systems language with approachable syntax

## Positioning

Arandu should not try to win as a generic language by ecosystem size. That market is already dominated by Python, JavaScript/TypeScript, Java, C#, C/C++, and Go. The stronger opening is narrower:

- systems programming with explicit safety;
- predictable error handling;
- FFI as a first-class concern;
- readable syntax for developers who want Rust-like guarantees with a smaller first step;
- tooling-friendly grammar and AST so editor support and AI-assisted coding can work early.

The practical message is:

> Arandu is a safe systems language designed around explicit errors, ownership-aware memory, readable syntax, and strong compiler feedback.

## Market Signals

### Stack Overflow Developer Survey 2025

Source: [Stack Overflow Developer Survey 2025, Technology](https://survey.stackoverflow.co/2025/technology)

Relevant signals:

- Rust remains the most admired programming language in the survey, at roughly 72%.
- Gleam, Elixir, and Zig also score highly in admiration, which suggests developers are open to newer languages when they offer clear ergonomics or safety.
- Rust usage is far smaller than JavaScript, Python, TypeScript, Java, C#, C++, and Go, which reinforces that admiration does not automatically become adoption.
- Cargo is also highly admired, showing that language quality and package/build tooling are perceived together.

Implication for Arandu:

- The compiler UX matters as much as the language design. Error messages, examples, package layout, and future build commands must feel official from day one.
- Do not lead with novelty. Lead with safety, clarity, and fast feedback.

### GitHub Octoverse 2025

Source: [GitHub Octoverse 2025](https://github.blog/news-insights/octoverse/octoverse-a-new-developer-joins-github-every-second-as-ai-leads-typescript-to-1/)

Relevant signals:

- TypeScript became the most used language on GitHub in August 2025, passing both Python and JavaScript.
- GitHub connects this shift partly to typed code being easier to maintain and easier to use with agent-assisted coding.
- Python remains central for AI and data work, so market gravity is not just syntax; it is ecosystem and workflow.

Implication for Arandu:

- A precise AST, stable examples, and type-aware tooling are strategic assets, not compiler bureaucracy.
- Arandu should keep syntax regular and machine-readable. This helps parsers, formatters, language servers, documentation tools, and AI code generation.

### TIOBE Index

Source: [TIOBE Index](https://www.tiobe.com/tiobe-index/)

Relevant signals:

- TIOBE is useful as a broad adoption signal, not a quality measure.
- Rust and Go are visible but still much smaller than long-established languages in broad popularity indices.
- Emerging systems languages can earn attention, but adoption requires tooling, docs, libraries, and interoperability.

Implication for Arandu:

- FFI and examples are not optional. They are the bridge from a small language to useful existing ecosystems.
- The early compiler should prioritize parse/check/run feedback over advanced backend work.

## Research Foundations

### Parsing: Pratt Parser

Source: [Vaughan Pratt, Top Down Operator Precedence](https://tdop.github.io/)

Use in Arandu:

- Keep recursive descent for declarations and statements.
- Use Pratt parsing for expressions.
- Store operator precedence in parser tables, not in a deep stack of one function per precedence level.

Why it fits:

- Arandu has many expression forms: binary operators, unary operators, `await`, casts, calls, field access, safe access, indexing, `?`, `catch`, and ranges.
- Pratt parsing keeps expression parsing compact and extensible.

### Memory Safety: RustBelt

Source: [RustBelt: Securing the Foundations of the Rust Programming Language](https://research.tudelft.nl/en/publications/rustbelt-securing-the-foundations-of-the-rust-programming-languag/)

Use in Arandu:

- Treat `unsafe` as a controlled boundary, not as a casual escape hatch.
- Keep ownership and borrowing as type-system concepts, not runtime conventions.
- Require future unsafe APIs to document why their safe wrappers preserve invariants.

Why it fits:

- Arandu's differentiator is memory safety with lower cognitive load. RustBelt is a reminder that a language can expose low-level control only if the safe surface is disciplined.

### Memory Regions: Cyclone

Source: [Region-Based Memory Management in Cyclone](https://www.cs.cornell.edu/projects/cyclone/papers/cyclone-regions.pdf)

Use in Arandu:

- Study regions as a possible model for `defer`, `errdefer`, ownership scopes, and explicit resource lifetimes.
- Keep manual memory control behind clear syntax: `alloc`, `free`, `own`, `mut`, `unsafe`.
- Avoid designing the memory checker before the parser/type checker are stable.

Why it fits:

- Arandu wants C-like control without C-like footguns. Cyclone is directly relevant because it explores safer systems programming without removing low-level control.

### Interpreters: Reynolds

Source: [John C. Reynolds, Definitional Interpreters for Higher-Order Programming Languages](https://cir.nii.ac.jp/crid/1360855569765909632)

Use in Arandu:

- Build a small interpreter before choosing a native backend.
- Define runtime semantics for literals, variables, calls, `if`, `return`, and `println` with minimal machinery.
- Use the interpreter as a language laboratory, not as the final runtime architecture.

Why it fits:

- An interpreter gives fast feedback while the language is still moving.
- It helps test whether AST decisions actually support execution.

### Type Checking: Milner and Hindley-Milner

Source: [Robin Milner, A Theory of Type Polymorphism in Programming](https://www.pure.ed.ac.uk/ws/files/15143545/1_s2.0_0022000078900144_main.pdf)

Use in Arandu:

- Treat parametric polymorphism as a core concept, but do not overbuild inference in v0.1.
- Start with explicit function parameter types and generic parameters.
- Add local inference for variable declarations from initializer expressions.

Why it fits:

- Arandu syntax already supports generic types and functions.
- A restrained type checker can grow toward richer inference without hiding too much from users early.

### Pattern Matching: Maranget

Source: [Luc Maranget, Warnings for Pattern Matching](https://www.cambridge.org/core/services/aop-cambridge-core/content/view/3165B75113781E2431E3856972940347/S0956796807006223a.pdf/warnings-for-pattern-matching.pdf)

Use in Arandu:

- Future `match` checking should report both non-exhaustive matches and useless arms.
- The AST should preserve pattern shape directly, not lower patterns too early.
- Keep enum and pattern semantics simple before adding open unions or advanced subtyping.

Why it fits:

- Arandu has enums and patterns in the grammar already.
- Good match diagnostics are a major part of making safe code pleasant.

## Product Decisions for v0.1

### Official Examples Come First

Examples are the contract between grammar and compiler. They should be small, curated, and organized by maturity:

- `examples/stable/`: must parse according to EBNF v0.6.
- `examples/invalid/`: must fail later parse/check/memory passes.
- `examples/draft/`: may guide future syntax and product direction.

### AST Before Lexer and Parser

The AST is the internal product spec for the compiler. Without it, the parser has no target and the examples have no semantic structure.

The AST should be documented in Markdown first because Arandu has not chosen a compiler implementation language yet.

### Interpreter Before Backend

The first runnable target should be:

```text
arandu run examples/stable/syntax/hello.aru
```

Minimum interpreter features:

- `int`
- `float`
- `bool`
- `str`
- `func`
- `if`
- `return`
- function calls
- `io.println`

This gives fast language feedback before committing to LLVM, QBE, C, Cranelift, or another backend.

### Type Checker Before Memory Checker

The memory checker depends on type and ownership facts. Building it before parser and type checker would freeze too many decisions too early.

Recommended order:

1. examples
2. AST
3. lexer
4. parser
5. minimal interpreter
6. type checker
7. memory checker
8. backend

## Near-Term Risks

- Enum declarations exist, but enum value construction is not yet explicit in EBNF v0.6.
- `Err` is a primitive type but not a value constructor in the grammar.
- UI and web syntax are promising, but they should remain draft until core language rules are stable.
- Ownership syntax exists, but ownership semantics should wait until type checking can provide reliable facts.
- FFI examples can parse before they are semantically meaningful; type mapping must be specified later.

## Recommended Next Milestone

After this foundation, the next milestone should be:

```text
Arandu v0.1 Lexer Contract
```

It should define:

- token kinds;
- keyword table;
- identifier category rules;
- string interpolation tokenization;
- doc comments;
- numeric literal normalization;
- automatic logical semicolon insertion;
- exact error format for malformed tokens.

This keeps the compiler path grounded: examples and AST first, then lexer, then parser.
