use arandu_semantics::amir::AmirStmt;
use arandu_semantics::passes::type_checker::types::{ArType, Primitive};
use cranelift_codegen::ir::InstBuilder;

use super::FunctionTranslator;
use crate::types::{ClifType, clif_type};

impl FunctionTranslator<'_, '_> {
    #[tracing::instrument(level = "trace", target = "arandu_backend_cranelift", skip(self))]
    pub(super) fn translate_stmt(&mut self, stmt: &AmirStmt) {
        if self.error.is_some() {
            return;
        }
        match stmt {
            AmirStmt::Assign { lhs, rhs } => {
                let lhs_ty = &self.current_func.temps[lhs.as_usize()].ty;
                if matches!(lhs_ty, ArType::Primitive(Primitive::Str)) {
                    let (ptr_val, len_val) = self.translate_str_rvalue(rhs);
                    if let Some(&(var_ptr, var_len)) = self.str_temp_map.get(lhs) {
                        self.builder.def_var(var_ptr, ptr_val);
                        self.builder.def_var(var_len, len_val);
                    }
                } else {
                    let expected_ty = self.get_temp_clif_type(*lhs);
                    let expected_ar_type = Some(&self.current_func.temps[lhs.as_usize()].ty);
                    let val = self.translate_rvalue(rhs, expected_ty, expected_ar_type);
                    if let Some(&var) = self.temp_map.get(lhs) {
                        self.builder.def_var(var, val);
                    }
                }
            }
            AmirStmt::Store { lhs, rhs } => {
                let lhs_ty = &self.current_func.locals[lhs.local.as_usize()].ty;
                if matches!(lhs_ty, ArType::Primitive(Primitive::Str)) {
                    let (ptr_val, len_val) = self.translate_str_operand(rhs);
                    if lhs.projections.is_empty() {
                        if let Some(&(var_ptr, var_len)) = self.str_local_map.get(&lhs.local) {
                            self.builder.def_var(var_ptr, ptr_val);
                            self.builder.def_var(var_len, len_val);
                        }
                    } else {
                        let (base_ptr, offset) = self.translate_place_address_for_load(lhs);
                        self.builder.ins().store(
                            cranelift_codegen::ir::MemFlagsData::new(),
                            ptr_val,
                            base_ptr,
                            offset,
                        );
                        self.builder.ins().store(
                            cranelift_codegen::ir::MemFlagsData::new(),
                            len_val,
                            base_ptr,
                            offset + self.ptr_type.bytes() as i32,
                        );
                    }
                } else {
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
            }

            AmirStmt::Call { lhs, callee, args } => {
                self.translate_call(lhs, callee, args);
            }
            AmirStmt::Free(op) => {
                let ptr_val = self.translate_operand(op, Some(self.ptr_type));
                self.emit_free_ptr(ptr_val);
            }
            AmirStmt::StorageLive(_) | AmirStmt::StorageDead(_) => {}
            AmirStmt::Destroy(place) => {
                if place.projections.is_empty() {
                    let ty = &self.current_func.locals[place.local.as_usize()].ty;
                    if !ty.is_copy_v01() {
                        if let Some(&var) = self.local_map.get(&place.local) {
                            let ptr_val = self.builder.use_var(var);
                            self.emit_free_ptr(ptr_val);
                        }
                    }
                }
            }
            AmirStmt::Nop => {}
        }
    }
}
