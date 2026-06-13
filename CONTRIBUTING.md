# Contributing to Arandu

Thank you for your interest in contributing to Arandu! We welcome contributions of all forms, including bug reports, feature requests, documentation improvements, and code changes.

Arandu is an experimental Brazilian systems programming language focused on memory safety, clean syntax, explicit errors, and native tooling.

---

## Getting Started

### Prerequisites

To build and test Arandu, you will need the standard Rust toolchain installed:

- **Rust (Stable)**: Ensure you have the latest stable compiler. [rustup.rs](https://rustup.rs) is the recommended way to install Rust.

### Setup and Compilation

1. Clone the repository:
   ```bash
   git clone https://github.com/BrunoF2P/Arandu-Lang.git
   cd Arandu-Lang
   ```

2. Build the workspace:
   ```bash
   cargo build --workspace
   ```

3. Run the test suite:
   ```bash
   cargo test --workspace
   ```

---

## Coding Standards & Guidelines

- **Formatting**: Always format your code with `cargo fmt` before submitting a pull request.
- **Clippy**: Run Clippy to catch common mistakes and keep the code clean:
   ```bash
   cargo clippy --workspace --all-targets
   ```
- **Casing Rules**: Arandu uses strict casing rules to distinguish values and types:
  - **Values & Functions**: `camelCase` (e.g. `userName`, `totalPrice`, `parseJson`).
  - **Types & Structs**: `PascalCase` (e.g. `User`, `HttpClient`).
  - **Enum Variants**: `PascalCase` (e.g. `Ok`, `Err`, `Loading`).

---

## Adding or Modifying Diagnostics

Arandu enforces a strict **1-to-1 bijection** between declared compiler diagnostic codes and their detailed markdown documentation. If you add a new diagnostic code, you **must** also add its corresponding documentation file, or the workspace build will fail.

### Step-by-Step Guide to Add a Diagnostic

1. **Declare the Code in `arandu_diagnostics`**:
   Add your new diagnostic variant to the `DiagCode` enum in [`crates/arandu_diagnostics/src/lib.rs`](crates/arandu_diagnostics/src/lib.rs):
   ```rust
   pub enum DiagCode {
       // ...
       T026CannotAssignImmutable, // New code
   }
   ```
   And map it to its string representation (e.g. `"T026"`) in the `as_str` method.

2. **Catalog in `SPEC.md`**:
   Document the code, its default message template, and its compiler version inside [`docs/diagnostics/SPEC.md`](docs/diagnostics/SPEC.md).

3. **Create the Documentation File**:
   Create a detailed documentation file under `docs/errors/<code>.md` (e.g. `docs/errors/T026.md`) written in **English**. 
   Ensure it includes:
   - An explanation of the diagnostic.
   - An **Erroneous Code Example** using ` ```arandu ` blocks.
   - A **Semantic Explanation** of the error.
   - A **How to Fix** section with a corrected code example.

4. **Verify the Bijection**:
   Run the test suite with the `ARANDU_VALIDATE_DOCS` environment variable set:
   ```bash
   ARANDU_VALIDATE_DOCS=1 cargo test --workspace
   ```
   This triggers the build script validation to verify that every diagnostic code matches a documentation file, and vice-versa.

---

## Testing Contributions

Arandu relies on various test types:
- **Unit & Integration Tests**: Standard cargo test files found inside crate `tests/` directories.
- **Golden Tests**: Tests that compare the output of parser/AST/AMIR lowering stages against stable snapshots (`.hir`, `.amir`, `.diag` files).
  To update golden tests after making valid parser/compiler modifications, run:
  ```bash
  UPDATE_EXPECT=1 cargo test --workspace
  ```

Thank you again for contributing to Arandu!
