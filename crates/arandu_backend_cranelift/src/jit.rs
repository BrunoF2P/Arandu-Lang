use crate::abi::build_signature;
use crate::translator::FunctionTranslator;
use arandu_semantics::SymbolTable;
use arandu_semantics::amir::AmirProgram;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use rustc_hash::FxHashMap;

pub struct AranduJit {
    pub module: JITModule,
}

impl Default for AranduJit {
    fn default() -> Self {
        Self::new()
    }
}

impl AranduJit {
    #[must_use]
    pub fn new() -> Self {
        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").unwrap();
        flag_builder.set("is_pic", "false").unwrap();
        flag_builder.set("opt_level", "none").unwrap();

        let isa = cranelift_native::builder()
            .expect("Failed to create Cranelift isa builder")
            .finish(settings::Flags::new(flag_builder))
            .expect("Failed to build Cranelift isa");

        let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        let module = JITModule::new(builder);

        Self { module }
    }

    pub fn compile_program(
        mut self,
        program: &AmirProgram,
        symbols: &SymbolTable,
    ) -> Result<CompiledModule, String> {
        let mut func_ids = FxHashMap::default();
        let default_call_conv = self.module.isa().default_call_conv();
        let ptr_type = self.module.target_config().pointer_type();

        // 1. Declare all functions first to support cross-calls
        // 1. Declare all functions first to support cross-calls
        for func in &program.funcs {
            let sym = symbols.get(func.symbol);
            let param_types: Vec<_> = func
                .params
                .iter()
                .map(|&p| func.temps[p.as_usize()].ty.clone())
                .collect();
            let sig = build_signature(&param_types, &func.return_type, default_call_conv, ptr_type);

            let func_id = self
                .module
                .declare_function(&sym.name, Linkage::Export, &sig)
                .map_err(|e| format!("Failed to declare function {}: {:?}", sym.name, e))?;
            func_ids.insert(sym.name.clone(), func_id);

            // Also find all NamespaceMember symbols that refer to this function (by matching name ending and span)
            // and map them to the same func_id!
            use arandu_semantics::SymbolKind;
            for s in symbols.iter() {
                if s.kind == SymbolKind::NamespaceMember
                    && s.name.ends_with(&format!(".{}", sym.name))
                    && s.span == sym.span
                {
                    func_ids.insert(s.name.clone(), func_id);
                }
            }
        }

        // Declare all extern functions as imports
        for (&symbol_id, (param_types, return_type)) in &program.extern_funcs {
            let sym = symbols.get(symbol_id);
            if func_ids.contains_key(&sym.name) {
                continue;
            }
            let c_name = sym.name.split('.').next_back().unwrap_or(&sym.name);
            let func_id = if let Some(&existing_id) = func_ids.get(c_name) {
                existing_id
            } else {
                let sig = build_signature(param_types, return_type, default_call_conv, ptr_type);
                self.module
                    .declare_function(c_name, Linkage::Import, &sig)
                    .map_err(|e| format!("Failed to declare extern function {}: {:?}", c_name, e))?
            };
            func_ids.insert(sym.name.clone(), func_id);
            if c_name != sym.name {
                func_ids.insert(c_name.to_string(), func_id);
            }
        }

        // 2. Define/compile each function
        let mut context = self.module.make_context();

        for func in &program.funcs {
            let mut builder_context = FunctionBuilderContext::new();
            let sym = symbols.get(func.symbol);
            let func_id = func_ids[&sym.name];

            let param_types: Vec<_> = func
                .params
                .iter()
                .map(|&p| func.temps[p.as_usize()].ty.clone())
                .collect();
            let sig = build_signature(&param_types, &func.return_type, default_call_conv, ptr_type);
            context.func.signature = sig;

            {
                let builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
                let mut translator = FunctionTranslator {
                    builder,
                    module: &mut self.module,
                    symbol_table: symbols,
                    func_ids: &func_ids,
                    block_map: FxHashMap::default(),
                    temp_map: FxHashMap::default(),
                    local_map: FxHashMap::default(),
                    ptr_type,
                    literal_pool: &program.literal_pool,
                    current_func: func,
                };
                translator.translate();
            }

            self.module
                .define_function(func_id, &mut context)
                .map_err(|e| format!("Failed to define function {}: {:?}", sym.name, e))?;
            self.module.clear_context(&mut context);
        }

        // 3. Finalize in-memory compilation
        self.module
            .finalize_definitions()
            .map_err(|e| format!("Failed to finalize JIT definitions: {:?}", e))?;

        Ok(CompiledModule {
            module: self.module,
            func_ids,
        })
    }
}

pub struct CompiledModule {
    module: JITModule,
    func_ids: FxHashMap<String, FuncId>,
}

impl CompiledModule {
    /// # Safety
    /// O chamador garante que a assinatura `F` corresponde
    /// exatamente à função compilada.
    pub unsafe fn get_fn<F>(&self, name: &str) -> Option<F> {
        let id = self.func_ids.get(name)?;
        let ptr = self.module.get_finalized_function(*id);
        Some(unsafe { std::mem::transmute_copy(&ptr) })
    }
}
