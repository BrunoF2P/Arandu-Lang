//! Cranelift JIT backend for Arandu.
//!
//! Exposes [`CraneliftBackend`] which implements the [`CodegenBackend`] trait.
//! The backend compiles an [`AmirProgram`] to native machine code in memory
//! via Cranelift and returns a [`CompiledModule`] whose functions can be
//! called directly through raw function pointers.

#![allow(clippy::collapsible_if)]
pub mod abi;
pub mod jit;
pub mod to_str_runtime;
pub mod translator;
pub mod types;

pub use crate::jit::CompiledModule;

use crate::jit::AranduJit;
use arandu_semantics::amir::AmirProgram;
use arandu_semantics::{CodegenBackend, CompiledCode, Diagnostic, SymbolTable};

/// Entry point for the Cranelift JIT backend.
///
/// Implements [`CodegenBackend`]; use [`CraneliftBackend::new`] and then
/// [`CraneliftBackend::compile`] to JIT-compile an [`AmirProgram`].
pub struct CraneliftBackend {
    jit: AranduJit,
}

impl CraneliftBackend {
    /// Creates a new `CraneliftBackend` with a freshly initialized JIT context.
    pub fn try_new() -> Result<Self, Diagnostic> {
        Ok(Self {
            jit: AranduJit::try_new()?,
        })
    }

    /// Compiles `program` to native code and returns the [`CompiledModule`].
    ///
    /// This is a convenience wrapper around [`CodegenBackend::compile`].
    pub fn compile(
        self,
        program: &AmirProgram,
        symbols: &SymbolTable,
        type_info: &arandu_semantics::TypeInfo,
    ) -> Result<CompiledModule, Diagnostic> {
        CodegenBackend::compile(self, program, symbols, type_info)
    }
}

impl CodegenBackend for CraneliftBackend {
    type TargetConfig = arandu_semantics::TypeInfo;
    type CompilationOutput = CompiledModule;

    fn compile(
        self,
        program: &AmirProgram,
        symbols: &SymbolTable,
        config: &Self::TargetConfig,
    ) -> Result<Self::CompilationOutput, Diagnostic> {
        self.jit.compile_program(program, symbols, config)
    }
}

impl CompiledCode for CompiledModule {
    unsafe fn get_fn<F>(&self, name: &str) -> Option<F> {
        unsafe { self.get_fn(name) }
    }
}
