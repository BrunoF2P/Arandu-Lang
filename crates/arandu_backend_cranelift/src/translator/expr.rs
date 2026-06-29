use arandu_semantics::amir::{AmirConstant, AmirOperand, AmirProjection, AmirRvalue};
use arandu_semantics::ops::UnaryOp;
use cranelift_codegen::ir::{InstBuilder, Type, Value};
use cranelift_module::Module;

use super::FunctionTranslator;

impl FunctionTranslator<'_, '_> {
    pub(super) fn translate_operand(
        &mut self,
        operand: &AmirOperand,
        expected_ty: Option<Type>,
    ) -> Value {
        if self.error.is_some() {
            return self.poison_i32();
        }

        let mut val = match operand {
            AmirOperand::Copy(temp_id) | AmirOperand::Move(temp_id) => {
                match self.temp_map.get(temp_id) {
                    Some(var) => self.builder.use_var(*var),
                    None => {
                        self.record_ice(
                            "use of undeclared AMIR temp in codegen",
                            self.temp_span(*temp_id),
                        );
                        return self.poison_i32();
                    }
                }
            }
            AmirOperand::Constant(c) => match c {
                AmirConstant::Bool(b) => {
                    let imm = if *b { 1 } else { 0 };
                    self.builder
                        .ins()
                        .iconst(cranelift_codegen::ir::types::I8, imm)
                }
                AmirConstant::Nil => self
                    .builder
                    .ins()
                    .iconst(cranelift_codegen::ir::types::I32, 0),
                AmirConstant::Pool(lit_id) => {
                    let entry = self.literal_pool.get(*lit_id);
                    match entry {
                        arandu_semantics::literal_pool::AmirLiteralEntry::Int(s) => {
                            let parsed = s.parse::<i64>();
                            let val = match parsed {
                                Ok(val) => val,
                                Err(_) => {
                                    self.record_ice(
                                        format!(
                                            "invalid integer literal in AMIR literal pool: '{s}'"
                                        ),
                                        self.func_span(),
                                    );
                                    return self.poison_i32();
                                }
                            };
                            let ty = expected_ty.unwrap_or(cranelift_codegen::ir::types::I32);
                            self.builder.ins().iconst(ty, val)
                        }
                        arandu_semantics::literal_pool::AmirLiteralEntry::Float(s) => {
                            let parsed = s.parse::<f64>();
                            let val = match parsed {
                                Ok(val) => val,
                                Err(_) => {
                                    self.record_ice(
                                        format!(
                                            "invalid float literal in AMIR literal pool: '{s}'"
                                        ),
                                        self.func_span(),
                                    );
                                    return self.poison_i32();
                                }
                            };
                            self.builder.ins().f64const(val)
                        }
                        arandu_semantics::literal_pool::AmirLiteralEntry::Str(s) => {
                            let str_bytes = s.as_bytes();
                            let data_id = match self.module.declare_data(
                                &format!("str_lit_{}", lit_id.0),
                                cranelift_module::Linkage::Local,
                                false,
                                false,
                            ) {
                                Ok(data_id) => data_id,
                                Err(err) => {
                                    self.record_ice(
                                        format!(
                                            "failed to declare string literal in JIT module: {err:?}"
                                        ),
                                        self.func_span(),
                                    );
                                    return self.poison_i32();
                                }
                            };
                            let mut data_ctx = cranelift_module::DataDescription::new();
                            data_ctx.define(str_bytes.to_vec().into_boxed_slice());
                            let _ = self.module.define_data(data_id, &data_ctx);
                            let local_data_ref =
                                self.module.declare_data_in_func(data_id, self.builder.func);
                            self.builder.ins().symbol_value(self.ptr_type, local_data_ref)
                        }
                        arandu_semantics::literal_pool::AmirLiteralEntry::Char(s) => {
                            let val = s.chars().next().unwrap_or('\0') as i64;
                            self.builder
                                .ins()
                                .iconst(cranelift_codegen::ir::types::I32, val)
                        }
                    }
                }
            },
            AmirOperand::FunctionRef(_) | AmirOperand::GlobalRef(_) => {
                unimplemented!("Refs as operands not implemented in Cranelift JIT yet");
            }
        };

        if let Some(target_ty) = expected_ty {
            let val_ty = self.builder.func.dfg.value_type(val);
            if val_ty != target_ty && val_ty.is_int() && target_ty.is_int() {
                if val_ty.bits() < target_ty.bits() {
                    val = self.builder.ins().sextend(target_ty, val);
                } else if val_ty.bits() > target_ty.bits() {
                    val = self.builder.ins().ireduce(target_ty, val);
                }
            }
        }

        val
    }

    pub(super) fn translate_rvalue(
        &mut self,
        rvalue: &AmirRvalue,
        expected_ty: Option<Type>,
    ) -> Value {
        if self.error.is_some() {
            return self.poison_i32();
        }

        match rvalue {
            AmirRvalue::Use(op) => self.translate_operand(op, expected_ty),
            AmirRvalue::Binary { op, left, right } => {
                let mut opt_ty = expected_ty;
                if opt_ty.is_none() {
                    if let AmirOperand::Copy(temp_id) | AmirOperand::Move(temp_id) = left {
                        opt_ty = self.get_temp_clif_type(*temp_id);
                    } else if let AmirOperand::Copy(temp_id) | AmirOperand::Move(temp_id) = right {
                        opt_ty = self.get_temp_clif_type(*temp_id);
                    }
                }
                let lhs = self.translate_operand(left, opt_ty);
                let rhs = self.translate_operand(right, opt_ty);
                self.translate_binary_op(*op, lhs, rhs, Some(left), Some(right))
            }
            AmirRvalue::Unary { op, operand } => {
                let val = self.translate_operand(operand, expected_ty);
                self.translate_unary_op(*op, val)
            }
            AmirRvalue::Load(place) => {
                if place.projections.is_empty() {
                    match self.local_map.get(&place.local) {
                        Some(var) => self.builder.use_var(*var),
                        None => {
                            self.record_ice(
                                "use of undeclared AMIR local in codegen",
                                self.local_span(place.local),
                            );
                            self.poison_i32()
                        }
                    }
                } else {
                    let mut ptr_val = if let Some(&var) = self.local_map.get(&place.local) {
                        self.builder.use_var(var)
                    } else {
                        self.record_ice(
                            "use of undeclared AMIR local in codegen",
                            self.local_span(place.local),
                        );
                        return self.poison_i32();
                    };

                    for proj in &place.projections {
                        match proj {
                            AmirProjection::Field(symbol_id) => {
                                let name = &self.symbol_table.get(*symbol_id).name;
                                let offset = match name.as_str() {
                                    "buf" => 0,
                                    "len" => 8,
                                    "cap" => 16,
                                    _ => {
                                        self.record_ice(
                                            format!(
                                                "unsupported struct field '{}' in codegen",
                                                name
                                            ),
                                            self.symbol_table.get(*symbol_id).span,
                                        );
                                        return self.poison_i32();
                                    }
                                };
                                let clif_ty = expected_ty.unwrap_or(self.ptr_type);
                                ptr_val = self.builder.ins().load(
                                    clif_ty,
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
                                let clif_ty = expected_ty.unwrap_or(self.ptr_type);
                                ptr_val = self.builder.ins().load(
                                    clif_ty,
                                    cranelift_codegen::ir::MemFlags::new(),
                                    elem_ptr,
                                    0,
                                );
                            }
                        }
                    }
                    ptr_val
                }
            }
            AmirRvalue::StructLiteral {
                struct_symbol: _,
                fields,
            } => {
                let Some(malloc_func_id) = self.malloc_func_id() else {
                    return self.poison_i32();
                };
                let local_ref = self
                    .module
                    .declare_func_in_func(malloc_func_id, self.builder.func);
                let size_val = self
                    .builder
                    .ins()
                    .iconst(self.ptr_type, (fields.len() * 8) as i64);
                let call_inst = self.builder.ins().call(local_ref, &[size_val]);
                let ptr_val = self.builder.inst_results(call_inst)[0];

                for (i, (name, op)) in fields.iter().enumerate() {
                    let field_idx = match name.as_str() {
                        "buf" => 0,
                        "len" => 1,
                        "cap" => 2,
                        _ => i,
                    };
                    let val = self.translate_operand(op, None);
                    let offset = (field_idx * 8) as i32;
                    self.builder.ins().store(
                        cranelift_codegen::ir::MemFlags::new(),
                        val,
                        ptr_val,
                        offset,
                    );
                }
                ptr_val
            }
            AmirRvalue::FieldAccess { base, field } => {
                let ptr_val = self.translate_operand(base, Some(self.ptr_type));
                let offset = (field * 8) as i32;
                let clif_ty = expected_ty.unwrap_or(self.ptr_type);
                self.builder.ins().load(
                    clif_ty,
                    cranelift_codegen::ir::MemFlags::new(),
                    ptr_val,
                    offset,
                )
            }
            AmirRvalue::Borrow(_) | AmirRvalue::BorrowMut(_) => {
                unimplemented!("Borrowing of places is not implemented in Cranelift JIT yet");
            }
            _ => {
                unimplemented!(
                    "Rvalue kind {:?} not implemented in Cranelift JIT yet",
                    rvalue
                );
            }
        }
    }

    pub(super) fn translate_unary_op(&mut self, op: UnaryOp, val: Value) -> Value {
        let ty = self.builder.func.dfg.value_type(val);
        let is_float = ty.is_float();

        match op {
            UnaryOp::Neg => {
                if is_float {
                    self.builder.ins().fneg(val)
                } else {
                    self.builder.ins().ineg(val)
                }
            }
            UnaryOp::Not => {
                let zero = self.builder.ins().iconst(ty, 0);
                self.builder
                    .ins()
                    .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, val, zero)
            }
            UnaryOp::BitNot => self.builder.ins().bnot(val),
            UnaryOp::Await => {
                unimplemented!("Unary operator Await not implemented in Cranelift JIT yet");
            }
            _ => {
                unimplemented!(
                    "Unary operator {:?} not implemented in Cranelift JIT yet",
                    op
                );
            }
        }
    }
}
