//! Low-level Cranelift JIT compilation internals.
//!
//! [`AranduJit`] drives the Cranelift JIT module lifecycle: it declares all
//! functions, defines each one via [`FunctionTranslator`], finalizes the
//! in-memory compilation, and returns a [`CompiledModule`] ready for execution.

use crate::abi::build_signature;
use crate::translator::FunctionTranslator;
use arandu_base::span::Span;
use arandu_semantics::amir::AmirProgram;
use arandu_semantics::passes::type_checker::types::{ArType, Primitive};
use arandu_semantics::{DiagCode, Diagnostic, SymbolTable};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use rustc_hash::FxHashMap;

fn codegen_ice(message: impl Into<String>) -> Diagnostic {
    Diagnostic::ice(DiagCode::ICEGEN001, message, Span::new(0, 0, 0))
}

/// Stateful Cranelift JIT context.
///
/// Wraps a [`JITModule`] and orchestrates the full compilation of an
/// [`AmirProgram`]: function declaration, translation, and memory finalization.
/// Consumed by [`AranduJit::compile_program`] — create a fresh instance for
/// each compilation.
pub struct AranduJit {
    pub module: JITModule,
}

impl AranduJit {
    /// Creates a new [`AranduJit`] with default Cranelift settings.
    ///
    /// Configures the host ISA via `cranelift_native`, disables PIC and
    /// library colocated calls, and sets optimization level to `none` (for
    /// fastest JIT compilation during development/testing).
    pub fn try_new() -> Result<Self, Diagnostic> {
        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").unwrap();
        flag_builder.set("is_pic", "false").unwrap();
        flag_builder.set("opt_level", "none").unwrap();

        let isa_builder = cranelift_native::builder()
            .map_err(|e| codegen_ice(format!("Failed to create Cranelift isa builder: {e}")))?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| codegen_ice(format!("Failed to build Cranelift isa: {e}")))?;

        let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        // ToStr v0.1 host helpers (malloc-backed fat strings).
        builder.symbol(
            "ar_jit_i64_to_str",
            crate::to_str_runtime::ar_jit_i64_to_str as *const u8,
        );
        builder.symbol(
            "ar_jit_u64_to_str",
            crate::to_str_runtime::ar_jit_u64_to_str as *const u8,
        );
        builder.symbol(
            "ar_jit_f64_to_str",
            crate::to_str_runtime::ar_jit_f64_to_str as *const u8,
        );
        builder.symbol(
            "ar_jit_bool_to_str",
            crate::to_str_runtime::ar_jit_bool_to_str as *const u8,
        );
        builder.symbol(
            "ar_jit_char_to_str",
            crate::to_str_runtime::ar_jit_char_to_str as *const u8,
        );
        // Prelude `io.println` (fat-pointer ABI: ptr + i64 len).
        builder.symbol(
            "io.println",
            crate::to_str_runtime::ar_jit_println as *const u8,
        );
        // Prelude `err.new(str) -> Err` (message handle = non-null ptr; fat-pointer str arg).
        builder.symbol(
            "err.new",
            crate::to_str_runtime::ar_jit_err_new as *const u8,
        );
        let module = JITModule::new(builder);

        Ok(Self { module })
    }

    /// Compiles all functions in `program` to native machine code.
    ///
    /// Three-phase compilation:
    /// 1. Declare all functions (enabling mutual recursion).
    /// 2. Define/translate each function body via [`FunctionTranslator`].
    /// 3. Finalize in-memory code and return a [`CompiledModule`].
    #[tracing::instrument(
        level = "trace",
        target = "arandu_backend_cranelift",
        skip(self, program, symbols, type_info)
    )]
    pub fn compile_program(
        mut self,
        program: &AmirProgram,
        symbols: &SymbolTable,
        type_info: &arandu_semantics::TypeInfo,
    ) -> Result<CompiledModule, Diagnostic> {
        let mut func_ids = FxHashMap::default();
        let default_call_conv = self.module.isa().default_call_conv();
        let ptr_type = self.module.target_config().pointer_type();

        // Declare malloc as import
        let mut malloc_sig = cranelift_codegen::ir::Signature::new(default_call_conv);
        malloc_sig
            .params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        malloc_sig
            .returns
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        let malloc_id = self
            .module
            .declare_function("malloc", Linkage::Import, &malloc_sig)
            .map_err(|err| codegen_ice(format!("failed to declare malloc: {err:?}")))?;
        func_ids.insert("malloc".to_string(), malloc_id);

        // Declare free as import
        let mut free_sig = cranelift_codegen::ir::Signature::new(default_call_conv);
        free_sig
            .params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        let free_id = self
            .module
            .declare_function("free", Linkage::Import, &free_sig)
            .map_err(|err| codegen_ice(format!("failed to declare free: {err:?}")))?;
        func_ids.insert("free".to_string(), free_id);

        // Declare fmod as import
        let mut fmod_sig = cranelift_codegen::ir::Signature::new(default_call_conv);
        fmod_sig.params.push(cranelift_codegen::ir::AbiParam::new(
            cranelift_codegen::ir::types::F64,
        ));
        fmod_sig.params.push(cranelift_codegen::ir::AbiParam::new(
            cranelift_codegen::ir::types::F64,
        ));
        fmod_sig.returns.push(cranelift_codegen::ir::AbiParam::new(
            cranelift_codegen::ir::types::F64,
        ));
        let fmod_id = self
            .module
            .declare_function("fmod", Linkage::Import, &fmod_sig)
            .map_err(|err| codegen_ice(format!("failed to declare fmod: {err:?}")))?;
        func_ids.insert("fmod".to_string(), fmod_id);

        // Declare memcpy as import (used by string interpolation concat)
        let mut memcpy_sig = cranelift_codegen::ir::Signature::new(default_call_conv);
        memcpy_sig
            .params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        memcpy_sig
            .params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        memcpy_sig
            .params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        memcpy_sig
            .returns
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        let memcpy_id = self
            .module
            .declare_function("memcpy", Linkage::Import, &memcpy_sig)
            .map_err(|err| codegen_ice(format!("failed to declare memcpy: {err:?}")))?;
        func_ids.insert("memcpy".to_string(), memcpy_id);

        // Declare memcmp as import (used by `str` equality / inequality).
        let mut memcmp_sig = cranelift_codegen::ir::Signature::new(default_call_conv);
        memcmp_sig
            .params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        memcmp_sig
            .params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        memcmp_sig
            .params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        memcmp_sig
            .returns
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I32,
            ));
        let memcmp_id = self
            .module
            .declare_function("memcmp", Linkage::Import, &memcmp_sig)
            .map_err(|err| codegen_ice(format!("failed to declare memcmp: {err:?}")))?;
        func_ids.insert("memcmp".to_string(), memcmp_id);

        // ToStr v0.1: host helpers `(value, *mut i64 out_len) -> *mut u8`
        let i64_ty = cranelift_codegen::ir::types::I64;
        let f64_ty = cranelift_codegen::ir::types::F64;
        let i8_ty = cranelift_codegen::ir::types::I8;
        let i32_ty = cranelift_codegen::ir::types::I32;
        for (name, val_ty) in [
            ("ar_jit_i64_to_str", i64_ty),
            ("ar_jit_u64_to_str", i64_ty),
            ("ar_jit_f64_to_str", f64_ty),
            ("ar_jit_bool_to_str", i8_ty),
            ("ar_jit_char_to_str", i32_ty),
        ] {
            let mut sig = cranelift_codegen::ir::Signature::new(default_call_conv);
            sig.params
                .push(cranelift_codegen::ir::AbiParam::new(val_ty));
            sig.params
                .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
            sig.returns
                .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
            let id = self
                .module
                .declare_function(name, Linkage::Import, &sig)
                .map_err(|err| codegen_ice(format!("failed to declare {name}: {err:?}")))?;
            func_ids.insert(name.to_string(), id);
        }

        // 1. Declare all functions first to support cross-calls
        for func in &program.funcs {
            let sym = symbols.get(func.symbol);
            let param_types: Vec<_> = func
                .params
                .iter()
                .map(|&p| type_info.type_interner.resolve(func.temps[p.as_usize()].ty))
                .collect();
            let ret_ty = type_info.type_interner.resolve(func.return_type);
            let sig = build_signature(&param_types, &ret_ty, default_call_conv, ptr_type);

            let func_id = self
                .module
                .declare_function(&sym.name, Linkage::Export, &sig)
                .map_err(|err| {
                    codegen_ice(format!(
                        "failed to declare function '{}': {err:?}",
                        sym.name
                    ))
                })?;
            func_ids.insert(sym.name.to_string(), func_id);

            // Also find all NamespaceMember symbols that refer to this function (by matching name ending and span)
            // and map them to the same func_id!
            use arandu_semantics::SymbolKind;
            for s in symbols.iter() {
                if s.kind == SymbolKind::NamespaceMember
                    && s.name.ends_with(&format!(".{}", sym.name))
                    && s.span == sym.span
                {
                    func_ids.insert(s.name.to_string(), func_id);
                }
            }
        }

        // Declare all extern functions as imports
        for (&symbol_id, (param_types, return_type)) in &program.extern_funcs {
            let sym = symbols.get(symbol_id);
            if func_ids.contains_key(sym.name.as_str()) {
                continue;
            }
            let c_name = sym.name.split('.').next_back().unwrap_or(&sym.name);
            let func_id = if let Some(&existing_id) = func_ids.get(c_name) {
                existing_id
            } else {
                let sig = build_signature(param_types, return_type, default_call_conv, ptr_type);
                self.module
                    .declare_function(c_name, Linkage::Import, &sig)
                    .map_err(|err| {
                        codegen_ice(format!(
                            "failed to declare extern function '{}': {err:?}",
                            c_name
                        ))
                    })?
            };
            func_ids.insert(sym.name.to_string(), func_id);
            if c_name != sym.name {
                func_ids.insert(c_name.to_string(), func_id);
            }
        }

        // Builtin prelude host imports (fat-pointer `str` args).
        let str_ty = ArType::Primitive(Primitive::Str);
        let void_ty = ArType::Void;
        let err_ty = ArType::Err;
        if !func_ids.contains_key("io.println") {
            let sig = build_signature(
                std::slice::from_ref(&str_ty),
                &void_ty,
                default_call_conv,
                ptr_type,
            );
            let id = self
                .module
                .declare_function("io.println", Linkage::Import, &sig)
                .map_err(|err| codegen_ice(format!("failed to declare io.println: {err:?}")))?;
            func_ids.insert("io.println".to_string(), id);
        }
        // `err.new(str) -> Err` (Err = message pointer handle).
        if !func_ids.contains_key("err.new") {
            let sig = build_signature(
                std::slice::from_ref(&str_ty),
                &err_ty,
                default_call_conv,
                ptr_type,
            );
            let id = self
                .module
                .declare_function("err.new", Linkage::Import, &sig)
                .map_err(|err| codegen_ice(format!("failed to declare err.new: {err:?}")))?;
            func_ids.insert("err.new".to_string(), id);
        }

        // 2. Define/compile each function
        let mut context = self.module.make_context();

        for func in &program.funcs {
            let mut builder_context = FunctionBuilderContext::new();
            let sym = symbols.get(func.symbol);
            let func_id = func_ids[sym.name.as_str()];

            let param_types: Vec<_> = func
                .params
                .iter()
                .map(|&p| type_info.type_interner.resolve(func.temps[p.as_usize()].ty))
                .collect();
            let ret_ty = type_info.type_interner.resolve(func.return_type);
            let sig = build_signature(&param_types, &ret_ty, default_call_conv, ptr_type);
            context.func.signature = sig;

            {
                let builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
                let mut translator = FunctionTranslator::new(
                    builder,
                    &mut self.module,
                    symbols,
                    &func_ids,
                    ptr_type,
                    &program.literal_pool,
                    func,
                    type_info,
                );
                translator.translate()?;
            }

            self.module
                .define_function(func_id, &mut context)
                .map_err(|err| {
                    codegen_ice(format!("failed to define function '{}': {err:?}", sym.name))
                })?;
            self.module.clear_context(&mut context);
        }

        // 3. Finalize in-memory compilation
        self.module
            .finalize_definitions()
            .map_err(|err| codegen_ice(format!("failed to finalize JIT definitions: {err:?}")))?;

        Ok(CompiledModule {
            module: self.module,
            func_ids,
        })
    }
}

/// The result of a successful JIT compilation.
///
/// Holds the finalized [`JITModule`] and the mapping from function names to
/// their [`FuncId`]s. Use [`CompiledModule::get_fn`] to obtain callable
/// function pointers.
pub struct CompiledModule {
    module: JITModule,
    func_ids: FxHashMap<String, FuncId>,
}

impl CompiledModule {
    /// Returns a callable function pointer for the named function.
    ///
    /// # Safety
    /// The caller must guarantee that the type `F` exactly matches the
    /// signature of the compiled function. Mismatched signatures cause
    /// undefined behaviour.
    pub unsafe fn get_fn<F>(&self, name: &str) -> Option<F> {
        let id = self.func_ids.get(name)?;
        let ptr = self.module.get_finalized_function(*id);
        assert_eq!(
            std::mem::size_of::<F>(),
            std::mem::size_of::<*const u8>(),
            "Type F must be the size of a function pointer"
        );
        Some(unsafe { std::mem::transmute_copy(&ptr) })
    }

    /// Returns a callable function pointer for the named function, but first checks
    /// that the full signature (types and arity) matches the expected signature.
    ///
    /// # Safety
    /// The caller must still guarantee that the type `F` matches the signature's types.
    pub unsafe fn get_fn_checked<F>(
        &self,
        name: &str,
        expected_sig: &cranelift_codegen::ir::Signature,
    ) -> Result<F, arandu_semantics::JitError> {
        let id = self
            .func_ids
            .get(name)
            .ok_or(arandu_semantics::JitError::NotFound)?;
        let decl = self.module.declarations().get_function_decl(*id);
        if decl.signature != *expected_sig {
            return Err(arandu_semantics::JitError::SignatureMismatch {
                expected: format!("{:?}", expected_sig),
                actual: format!("{:?}", decl.signature),
            });
        }

        unsafe { self.get_fn::<F>(name) }.ok_or(arandu_semantics::JitError::NotFound)
    }
}
