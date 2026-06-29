use arandu_semantics::amir::{
    AmirOperand, AmirPlace, AmirProjection, AmirStmt, AmirTerminator, TempId,
};
use cranelift_codegen::ir::{InstBuilder, TrapCode, Type, Value};
use cranelift_frontend::Switch;
use cranelift_module::{FuncId, Module};

use super::FunctionTranslator;
use crate::types::{ClifType, clif_type};

impl FunctionTranslator<'_, '_> {
    pub(super) fn translate_stmt(&mut self, stmt: &AmirStmt) {
        if self.error.is_some() {
            return;
        }
        match stmt {
            AmirStmt::Assign { lhs, rhs } => {
                let expected_ty = self.get_temp_clif_type(*lhs);
                let val = self.translate_rvalue(rhs, expected_ty);
                if let Some(&var) = self.temp_map.get(lhs) {
                    self.builder.def_var(var, val);
                }
            }
            AmirStmt::Store { lhs, rhs } => {
                let expected_ty = self
                    .current_func
                    .locals
                    .iter()
                    .find(|l| l.id == lhs.local)
                    .and_then(|l| match clif_type(&l.ty, self.ptr_type) {
                        ClifType::Concrete(ty) => Some(ty),
                        ClifType::Void => None,
                    });
                let val = self.translate_operand(rhs, expected_ty);
                self.translate_store_place(lhs, val);
            }
            AmirStmt::Call { lhs, callee, args } => {
                if let AmirOperand::FunctionRef(sym_id) = callee {
                    let sym = self.symbol_table.get(*sym_id);
                    if sym.name.starts_with("std.core.mem.ptr_read") {
                        let ptr_val = self.translate_operand(&args[0], Some(self.ptr_type));
                        let clif_ty = lhs
                            .and_then(|temp| self.get_temp_clif_type(temp))
                            .unwrap_or(self.ptr_type);
                        let loaded_val = self.builder.ins().load(
                            clif_ty,
                            cranelift_codegen::ir::MemFlags::new(),
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
                            cranelift_codegen::ir::MemFlags::new(),
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
                        let func_id = match self.func_ids.get(&sym.name) {
                            Some(func_id) => *func_id,
                            None => {
                                self.record_ice(
                                    format!(
                                        "function '{}' was not declared in the JIT module",
                                        sym.name
                                    ),
                                    sym.span,
                                );
                                return;
                            }
                        };
                        let local_ref = self
                            .module
                            .declare_func_in_func(func_id, self.builder.func);

                        let sig_id = self.builder.func.dfg.ext_funcs[local_ref].signature;
                        let expected_tys: Vec<Type> = self.builder.func.dfg.signatures[sig_id]
                            .params
                            .iter()
                            .map(|param| param.value_type)
                            .collect();

                        let clif_args: Vec<Value> = args
                            .iter()
                            .enumerate()
                            .map(|(i, arg)| {
                                let expected = expected_tys.get(i).copied();
                                self.translate_operand(arg, expected)
                            })
                            .collect();

                        self.builder.ins().call(local_ref, &clif_args)
                    }
                    _ => unimplemented!("Indirect function calls not implemented yet"),
                };
                if let Some(lhs_temp) = lhs {
                    if let Some(&var) = self.temp_map.get(lhs_temp) {
                        let results = self.builder.inst_results(call_inst);
                        if !results.is_empty() {
                            self.builder.def_var(var, results[0]);
                        }
                    }
                }
            }
            AmirStmt::Free(_) => {}
            AmirStmt::StorageLive(_) | AmirStmt::StorageDead(_) => {}
            AmirStmt::Destroy(_) => {}
            AmirStmt::Nop => {}
        }
    }

    pub(super) fn translate_store_place(&mut self, lhs: &AmirPlace, val: Value) {
        if self.error.is_some() {
            return;
        }
        if lhs.projections.is_empty() {
            if let Some(&var) = self.local_map.get(&lhs.local) {
                self.builder.def_var(var, val);
            } else {
                self.record_ice(
                    "use of undeclared AMIR local in codegen",
                    self.local_span(lhs.local),
                );
            }
        } else {
            let mut ptr_val = if let Some(&var) = self.local_map.get(&lhs.local) {
                self.builder.use_var(var)
            } else {
                self.record_ice(
                    "use of undeclared AMIR local in codegen",
                    self.local_span(lhs.local),
                );
                return;
            };

            for i in 0..lhs.projections.len() - 1 {
                let proj = &lhs.projections[i];
                match proj {
                    AmirProjection::Field(symbol_id) => {
                        let name = &self.symbol_table.get(*symbol_id).name;
                        let offset = match name.as_str() {
                            "buf" => 0,
                            "len" => 8,
                            "cap" => 16,
                            _ => {
                                self.record_ice(
                                    format!("unsupported struct field '{}' in codegen", name),
                                    self.symbol_table.get(*symbol_id).span,
                                );
                                return;
                            }
                        };
                        ptr_val = self.builder.ins().load(
                            self.ptr_type,
                            cranelift_codegen::ir::MemFlags::new(),
                            ptr_val,
                            offset,
                        );
                    }
                    AmirProjection::Index(op) => {
                        let idx_val = self.translate_operand(op, Some(self.ptr_type));
                        let elem_size = self.builder.ins().iconst(self.ptr_type, 8);
                        let offset_val = self.builder.ins().imul(idx_val, elem_size);
                        let elem_ptr = self.builder.ins().iadd(ptr_val, offset_val);
                        ptr_val = self.builder.ins().load(
                            self.ptr_type,
                            cranelift_codegen::ir::MemFlags::new(),
                            elem_ptr,
                            0,
                        );
                    }
                }
            }

            let Some(last_proj) = lhs.projections.last() else {
                return;
            };
            match last_proj {
                AmirProjection::Field(symbol_id) => {
                    let name = &self.symbol_table.get(*symbol_id).name;
                    let offset = match name.as_str() {
                        "buf" => 0,
                        "len" => 8,
                        "cap" => 16,
                        _ => {
                            self.record_ice(
                                format!("unsupported struct field '{}' in codegen", name),
                                self.symbol_table.get(*symbol_id).span,
                            );
                            return;
                        }
                    };
                    self.builder.ins().store(
                        cranelift_codegen::ir::MemFlags::new(),
                        val,
                        ptr_val,
                        offset,
                    );
                }
                AmirProjection::Index(op) => {
                    let idx_val = self.translate_operand(op, Some(self.ptr_type));
                    let elem_size = self.builder.ins().iconst(self.ptr_type, 8);
                    let offset_val = self.builder.ins().imul(idx_val, elem_size);
                    let target_ptr = self.builder.ins().iadd(ptr_val, offset_val);
                    self.builder.ins().store(
                        cranelift_codegen::ir::MemFlags::new(),
                        val,
                        target_ptr,
                        0,
                    );
                }
            }
        }
    }

    pub(super) fn malloc_func_id(&mut self) -> Option<FuncId> {
        match self.func_ids.get("malloc") {
            Some(func_id) => Some(*func_id),
            None => {
                self.record_ice(
                    "malloc was not declared in the JIT module",
                    self.func_span(),
                );
                None
            }
        }
    }

    pub(super) fn translate_terminator(&mut self, terminator: &AmirTerminator) {
        match terminator {
            AmirTerminator::Return => {
                let clif_ret = clif_type(&self.current_func.return_type, self.ptr_type);
                match clif_ret {
                    ClifType::Concrete(_) => {
                        let ret_temp = TempId::from_usize(0);
                        if let Some(&var) = self.temp_map.get(&ret_temp) {
                            let ret_val = self.builder.use_var(var);
                            self.builder.ins().return_(&[ret_val]);
                        } else {
                            self.builder.ins().return_(&[]);
                        }
                    }
                    ClifType::Void => {
                        self.builder.ins().return_(&[]);
                    }
                }
            }
            AmirTerminator::Goto(target) => {
                let clif_target = self.block_map[target];
                self.builder.ins().jump(clif_target, &[]);
            }
            AmirTerminator::Branch {
                condition,
                if_true,
                if_false,
            } => {
                let cond_val = self.translate_operand(condition, None);
                let true_block = self.block_map[if_true];
                let false_block = self.block_map[if_false];
                self.builder
                    .ins()
                    .brif(cond_val, true_block, &[], false_block, &[]);
            }
            AmirTerminator::SwitchInt {
                discriminant,
                targets,
                otherwise,
            } => {
                let disc_val = self.translate_operand(discriminant, None);
                let otherwise_block = self.block_map[otherwise];

                let mut switch = Switch::new();
                for &(val, ref target) in targets {
                    let target_block = self.block_map[target];
                    switch.set_entry(val as u128, target_block);
                }
                switch.emit(&mut self.builder, disc_val, otherwise_block);
            }
            AmirTerminator::Unreachable => {
                self.builder.ins().trap(TrapCode::unwrap_user(1));
            }
        }
    }
}
