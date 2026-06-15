#![allow(clippy::collapsible_if)]
pub mod types;
pub mod abi;
pub mod jit;
pub mod translator;

pub use crate::jit::CompiledModule;

use arandu_semantics::amir::AmirProgram;
use arandu_semantics::{Diagnostic, DiagCode, SymbolTable, CodegenBackend, CompiledCode};
use arandu_base::span::Span;
use crate::jit::AranduJit;

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
        self.jit
            .compile_program(program, symbols)
            .map_err(|err| {
                Diagnostic::ice(DiagCode::ICEL001, err, Span::new(0, 0, 0))
            })
    }
}

impl CompiledCode for CompiledModule {
    unsafe fn get_fn<F>(&self, name: &str) -> Option<F> {
        unsafe { self.get_fn(name) }
    }
}
