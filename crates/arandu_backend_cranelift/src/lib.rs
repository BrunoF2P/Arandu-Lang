#![allow(clippy::collapsible_if)]
pub mod abi;
pub mod jit;
pub mod translator;
pub mod types;

pub use crate::jit::CompiledModule;

use crate::jit::AranduJit;
use arandu_semantics::amir::AmirProgram;
use arandu_semantics::{CodegenBackend, CompiledCode, Diagnostic, SymbolTable};

pub struct CraneliftBackend {
    jit: AranduJit,
}

impl Default for CraneliftBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CraneliftBackend {
    #[must_use]
    pub fn new() -> Self {
        Self {
            jit: AranduJit::new(),
        }
    }

    pub fn compile(
        self,
        program: &AmirProgram,
        symbols: &SymbolTable,
    ) -> Result<CompiledModule, Diagnostic> {
        CodegenBackend::compile(self, program, symbols, &())
    }
}

impl CodegenBackend for CraneliftBackend {
    type TargetConfig = ();
    type CompilationOutput = CompiledModule;

    fn compile(
        self,
        program: &AmirProgram,
        symbols: &SymbolTable,
        _config: &Self::TargetConfig,
    ) -> Result<Self::CompilationOutput, Diagnostic> {
        self.jit.compile_program(program, symbols)
    }
}

impl CompiledCode for CompiledModule {
    unsafe fn get_fn<F>(&self, name: &str) -> Option<F> {
        unsafe { self.get_fn(name) }
    }
}
