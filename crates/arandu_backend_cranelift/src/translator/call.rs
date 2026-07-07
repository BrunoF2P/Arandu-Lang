use arandu_semantics::amir::{AmirOperand, TempId};
use arandu_semantics::passes::type_checker::types::{ArType, Primitive};
use cranelift_codegen::ir::{InstBuilder, Type};
use cranelift_module::Module;

use super::FunctionTranslator;

impl FunctionTranslator<'_, '_> {
    pub(super) fn translate_call(
        &mut self,
        lhs: &Option<TempId>,
        callee: &AmirOperand,
        args: &[AmirOperand],
    ) {
        if let AmirOperand::FunctionRef(sym_id) = callee {
            let sym = self.symbol_table.get(*sym_id);
            if sym.name.starts_with("std.core.mem.ptr_read") {
                let ptr_val = self.translate_operand(&args[0], Some(self.ptr_type));
                let clif_ty = lhs
                    .and_then(|temp| self.get_temp_clif_type(temp))
                    .unwrap_or(self.ptr_type);
                let loaded_val = self.builder.ins().load(
                    clif_ty,
                    cranelift_codegen::ir::MemFlagsData::new(),
                    ptr_val,
                    0,
                );
                if let Some(lhs_temp) = lhs {
                    if let Some(&var) = self.temp_map.get(lhs_temp) {
                        self.builder.def_var(var, loaded_val);
                    }
                }
                return;
            }
            if sym.name.starts_with("std.core.mem.ptr_write") {
                let ptr_val = self.translate_operand(&args[0], Some(self.ptr_type));
                let val_to_store = self.translate_operand(&args[1], None);
                self.builder.ins().store(
                    cranelift_codegen::ir::MemFlagsData::new(),
                    val_to_store,
                    ptr_val,
                    0,
                );
                return;
            }
        }

        let call_inst = match callee {
            AmirOperand::FunctionRef(sym_id) => {
                let sym = self.symbol_table.get(*sym_id);
                let func_id = match self.func_ids.get(sym.name.as_str()) {
                    Some(func_id) => *func_id,
                    None => {
                        self.record_ice(
                            format!("function '{}' was not declared in the JIT module", sym.name),
                            sym.span,
                        );
                        return;
                    }
                };
                let local_ref = self.module.declare_func_in_func(func_id, self.builder.func);

                let sig_id = self.builder.func.dfg.ext_funcs[local_ref].signature;
                let expected_tys: Vec<Type> = self.builder.func.dfg.signatures[sig_id]
                    .params
                    .iter()
                    .map(|param| param.value_type)
                    .collect();

                let mut clif_args = Vec::new();
                let mut clif_param_idx = 0;
                for arg in args {
                    let arg_ty = self.get_operand_ar_type(arg);
                    if matches!(arg_ty, ArType::Primitive(Primitive::Str)) {
                        let (ptr_val, len_val) = self.translate_str_operand(arg);
                        clif_args.push(ptr_val);
                        clif_args.push(len_val);
                        clif_param_idx += 2;
                    } else {
                        let expected = expected_tys.get(clif_param_idx).copied();
                        let val = self.translate_operand(arg, expected);
                        clif_args.push(val);
                        clif_param_idx += 1;
                    }
                }

                self.builder.ins().call(local_ref, &clif_args)
            }
            _ => {
                self.record_ice(
                    "indirect function calls are not implemented (and should have been rejected by the type checker)",
                    self.func_span(),
                );
                return;
            }
        };
        if let Some(lhs_temp) = lhs {
            let lhs_ty = &self.current_func.temps[lhs_temp.as_usize()].ty;
            if matches!(lhs_ty, ArType::Primitive(Primitive::Str)) {
                let results = self.builder.inst_results(call_inst);
                if results.len() >= 2 {
                    let res0 = results[0];
                    let res1 = results[1];
                    if let Some(&(var_ptr, var_len)) = self.str_temp_map.get(lhs_temp) {
                        self.builder.def_var(var_ptr, res0);
                        self.builder.def_var(var_len, res1);
                    }
                }
            } else if let Some(&var) = self.temp_map.get(lhs_temp) {
                let results = self.builder.inst_results(call_inst);
                if !results.is_empty() {
                    let res0 = results[0];
                    self.builder.def_var(var, res0);
                }
            }
        }
    }
}
