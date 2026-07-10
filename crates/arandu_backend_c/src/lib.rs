//! C backend: AMIR → portable C translation unit.
//!
//! Target layout is explicit via [`arandu_middle::layout::DataLayout`]. Use
//! [`emit_c`] with [`LayoutEngine::host`] for host parity or
//! [`DataLayout::i686_sysv`] / [`DataLayout::ptr_width`] for cross targets.

pub mod emitter;

pub use emitter::CEmitter;

use arandu_middle::amir::AmirProgram;
use arandu_middle::layout::{DataLayout, LayoutEngine, StructLayoutProvider};
use arandu_middle::types::TypeInterner;
use arandu_semantics::SymbolTable;

/// Emit a full C translation unit for `program` under `data_layout`.
///
/// Cranelift does not use this path; call with [`DataLayout::host`] only when
/// comparing host C to host JIT. For 32-bit / i686 portable C, pass the
/// matching [`DataLayout`] (emit-only unless a matching C toolchain is used).
pub fn emit_c(
    program: &AmirProgram,
    symbols: &SymbolTable,
    provider: &dyn StructLayoutProvider,
    interner: &TypeInterner,
    data_layout: DataLayout,
) -> String {
    let engine = LayoutEngine::from_data_layout(data_layout);
    CEmitter::new(program, symbols, &engine, provider, interner).emit()
}
