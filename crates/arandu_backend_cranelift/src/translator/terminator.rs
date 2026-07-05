use arandu_semantics::amir::AmirTerminator;
use arandu_semantics::passes::type_checker::types::{ArType, Primitive};
use cranelift_codegen::ir::{BlockArg, InstBuilder, TrapCode};
use cranelift_frontend::Switch;

use super::FunctionTranslator;
use crate::types::{ClifType, clif_type};

impl FunctionTranslator<'_, '_> {
    pub(super) fn translate_terminator(&mut self, terminator: &AmirTerminator) {
        match terminator {
            AmirTerminator::Return => {
                if matches!(
                    self.current_func.return_type,
                    ArType::Primitive(Primitive::Str)
                ) {
                    let ret_temp = arandu_semantics::amir::TempId::from_usize(0);
                    if let Some(&(var_ptr, var_len)) = self.str_temp_map.get(&ret_temp) {
                        let ptr_val = self.builder.use_var(var_ptr);
                        let len_val = self.builder.use_var(var_len);
                        self.builder.ins().return_(&[ptr_val, len_val]);
                    } else {
                        let p = self.poison_i32();
                        self.builder.ins().return_(&[p, p]);
                    }
                } else {
                    let clif_ret = clif_type(&self.current_func.return_type, self.ptr_type);
                    match clif_ret {
                        ClifType::Concrete(_) => {
                            let ret_temp = arandu_semantics::amir::TempId::from_usize(0);
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
            }

            AmirTerminator::Goto { target, args } => {
                let target_block = &self.current_func.blocks[target.as_usize()];
                let mut clif_args = Vec::new();
                for (j, arg) in args.iter().enumerate() {
                    let param_ty = &target_block.params[j].ty;
                    if matches!(param_ty, ArType::Primitive(Primitive::Str)) {
                        let (ptr_val, len_val) = self.translate_str_operand(arg);
                        clif_args.push(BlockArg::Value(ptr_val));
                        clif_args.push(BlockArg::Value(len_val));
                    } else if let ClifType::Concrete(ty) = clif_type(param_ty, self.ptr_type) {
                        let val = self.translate_operand(arg, Some(ty));
                        clif_args.push(BlockArg::Value(val));
                    }
                }
                let clif_target = self.block_map[target];
                self.builder.ins().jump(clif_target, &clif_args);
            }
            AmirTerminator::Branch {
                condition,
                if_true,
                true_args,
                if_false,
                false_args,
            } => {
                let cond_val = self.translate_operand(condition, None);

                let true_block_def = &self.current_func.blocks[if_true.as_usize()];
                let mut true_clif_args = Vec::new();
                for (j, arg) in true_args.iter().enumerate() {
                    let param_ty = &true_block_def.params[j].ty;
                    if matches!(param_ty, ArType::Primitive(Primitive::Str)) {
                        let (ptr_val, len_val) = self.translate_str_operand(arg);
                        true_clif_args.push(BlockArg::Value(ptr_val));
                        true_clif_args.push(BlockArg::Value(len_val));
                    } else if let ClifType::Concrete(ty) = clif_type(param_ty, self.ptr_type) {
                        let val = self.translate_operand(arg, Some(ty));
                        true_clif_args.push(BlockArg::Value(val));
                    }
                }

                let false_block_def = &self.current_func.blocks[if_false.as_usize()];
                let mut false_clif_args = Vec::new();
                for (j, arg) in false_args.iter().enumerate() {
                    let param_ty = &false_block_def.params[j].ty;
                    if matches!(param_ty, ArType::Primitive(Primitive::Str)) {
                        let (ptr_val, len_val) = self.translate_str_operand(arg);
                        false_clif_args.push(BlockArg::Value(ptr_val));
                        false_clif_args.push(BlockArg::Value(len_val));
                    } else if let ClifType::Concrete(ty) = clif_type(param_ty, self.ptr_type) {
                        let val = self.translate_operand(arg, Some(ty));
                        false_clif_args.push(BlockArg::Value(val));
                    }
                }

                let true_block = self.block_map[if_true];
                let false_block = self.block_map[if_false];
                self.builder.ins().brif(
                    cond_val,
                    true_block,
                    &true_clif_args,
                    false_block,
                    &false_clif_args,
                );
            }

            AmirTerminator::SwitchInt {
                discriminant,
                targets,
                otherwise,
            } => {
                let mut disc_val = self.translate_operand(discriminant, None);
                let disc_ty = self.builder.func.dfg.value_type(disc_val);
                if disc_ty == cranelift_codegen::ir::types::I64 {
                    disc_val = self
                        .builder
                        .ins()
                        .ireduce(cranelift_codegen::ir::types::I32, disc_val);
                }
                let otherwise_block = self.block_map[&otherwise.0];
                assert!(
                    otherwise.1.is_empty(),
                    "SwitchInt otherwise block cannot have arguments"
                );

                let mut switch = Switch::new();
                for &(val, ref target, ref args) in targets {
                    assert!(
                        args.is_empty(),
                        "SwitchInt target block cannot have arguments"
                    );
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
