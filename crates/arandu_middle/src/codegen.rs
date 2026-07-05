use crate::amir::AmirProgram;
use crate::diagnostics::Diagnostic;
use crate::symbol_table::SymbolTable;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JitError {
    NotFound,
    SignatureMismatch { expected: usize, actual: usize },
}

pub trait CompiledCode {
    /// # Safety
    /// The caller must ensure that the function signature (F) matches
    /// exactly the signature of the compiled function.
    unsafe fn get_fn<F>(&self, name: &str) -> Option<F>;

    /// # Safety
    /// The caller must ensure that the function signature (F) matches
    /// exactly the signature of the compiled function, but this method
    /// performs a runtime check on the expected arity.
    unsafe fn get_fn_checked<F>(&self, name: &str, expected_arity: usize) -> Result<F, JitError>;
}

pub trait CodegenBackend {
    type TargetConfig;
    type CompilationOutput: CompiledCode;

    fn compile(
        self,
        program: &AmirProgram,
        symbols: &SymbolTable,
        config: &Self::TargetConfig,
    ) -> Result<Self::CompilationOutput, Diagnostic>;
}
