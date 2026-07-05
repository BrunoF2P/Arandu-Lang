use arandu_semantics::amir::{AmirConstant, AmirOperand, AmirPlace, AmirProjection, AmirRvalue};
use arandu_semantics::ops::UnaryOp;
use arandu_semantics::passes::type_checker::types::{ArType, Primitive};
use cranelift_codegen::ir::{InstBuilder, Type, Value};
use cranelift_module::Module;

use super::FunctionTranslator;

impl FunctionTranslator<'_, '_> {
    pub(super) fn translate_str_operand(&mut self, operand: &AmirOperand) -> (Value, Value) {
        if self.error.is_some() {
            return (self.poison_i32(), self.poison_i32());
        }

        match operand {
            AmirOperand::Copy(temp_id) | AmirOperand::Move(temp_id) => {
                if let Some(&(var_ptr, var_len)) = self.str_temp_map.get(temp_id) {
                    let ptr_val = self.builder.use_var(var_ptr);
                    let len_val = self.builder.use_var(var_len);
                    (ptr_val, len_val)
                } else if let Some(&var) = self.temp_map.get(temp_id) {
                    let ptr_val = self.builder.use_var(var);
                    let len_val = self
                        .builder
                        .ins()
                        .iconst(cranelift_codegen::ir::types::I64, 0);
                    (ptr_val, len_val)
                } else {
                    self.record_ice(
                        "use of undeclared AMIR temp in codegen",
                        self.temp_span(*temp_id),
                    );
                    (self.poison_i32(), self.poison_i32())
                }
            }
            AmirOperand::Constant(AmirConstant::Pool(lit_id)) => {
                let entry = self.literal_pool.get(*lit_id);
                if let arandu_semantics::literal_pool::AmirLiteralEntry::Str(s) = entry {
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
                                format!("failed to declare string literal in JIT module: {err:?}"),
                                self.func_span(),
                            );
                            return (self.poison_i32(), self.poison_i32());
                        }
                    };
                    let mut data_ctx = cranelift_module::DataDescription::new();
                    data_ctx.define(str_bytes.to_vec().into_boxed_slice());
                    let _ = self.module.define_data(data_id, &data_ctx);
                    let local_data_ref =
                        self.module.declare_data_in_func(data_id, self.builder.func);
                    let ptr_val = self
                        .builder
                        .ins()
                        .symbol_value(self.ptr_type, local_data_ref);
                    let len_val = self
                        .builder
                        .ins()
                        .iconst(cranelift_codegen::ir::types::I64, s.len() as i64);
                    (ptr_val, len_val)
                } else {
                    self.record_ice("expected string literal in pool", self.func_span());
                    (self.poison_i32(), self.poison_i32())
                }
            }
            _ => {
                self.record_ice(
                    "unsupported operand for translate_str_operand",
                    self.func_span(),
                );
                (self.poison_i32(), self.poison_i32())
            }
        }
    }

    pub(super) fn translate_str_rvalue(&mut self, rvalue: &AmirRvalue) -> (Value, Value) {
        if self.error.is_some() {
            return (self.poison_i32(), self.poison_i32());
        }

        match rvalue {
            AmirRvalue::Use(op) => self.translate_str_operand(op),
            AmirRvalue::Load(place) => {
                if place.projections.is_empty() {
                    if let Some(&(var_ptr, var_len)) = self.str_local_map.get(&place.local) {
                        (self.builder.use_var(var_ptr), self.builder.use_var(var_len))
                    } else {
                        self.record_ice(
                            "use of undeclared AMIR local str in codegen",
                            self.local_span(place.local),
                        );
                        (self.poison_i32(), self.poison_i32())
                    }
                } else {
                    let (ptr_val, offset) = self.translate_place_address_for_load(place);
                    let loaded_ptr = self.builder.ins().load(
                        self.ptr_type,
                        cranelift_codegen::ir::MemFlagsData::new(),
                        ptr_val,
                        offset,
                    );
                    let loaded_len = self.builder.ins().load(
                        cranelift_codegen::ir::types::I64,
                        cranelift_codegen::ir::MemFlagsData::new(),
                        ptr_val,
                        offset + self.ptr_type.bytes() as i32,
                    );
                    (loaded_ptr, loaded_len)
                }
            }
            AmirRvalue::FieldAccess { base, field } => {
                let ptr_val = self.translate_operand(base, Some(self.ptr_type));
                let base_ty = match base {
                    AmirOperand::Copy(temp_id) | AmirOperand::Move(temp_id) => {
                        &self.current_func.temps[temp_id.as_usize()].ty
                    }
                    _ => &arandu_semantics::types::ArType::Error,
                };
                let struct_ty = match base_ty {
                    arandu_semantics::types::ArType::Ptr(inner) => {
                        self.type_info.resolve_type_id(*inner)
                    }
                    other => other,
                };
                let pointer_width = self.ptr_type.bytes() as u64;
                let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                let layout =
                    engine.layout_of_type(struct_ty, &self.type_info.type_interner, self.type_info);
                let offset = layout.field_offsets[*field] as i32;

                let loaded_ptr = self.builder.ins().load(
                    self.ptr_type,
                    cranelift_codegen::ir::MemFlagsData::new(),
                    ptr_val,
                    offset,
                );
                let loaded_len = self.builder.ins().load(
                    cranelift_codegen::ir::types::I64,
                    cranelift_codegen::ir::MemFlagsData::new(),
                    ptr_val,
                    offset + pointer_width as i32,
                );
                (loaded_ptr, loaded_len)
            }
            AmirRvalue::EnumPayload {
                value,
                variant,
                index,
            } => {
                let ptr_val = self.translate_operand(value, Some(self.ptr_type));
                let pointer_width = self.ptr_type.bytes() as u64;

                let base_ty = match value {
                    AmirOperand::Copy(temp_id) | AmirOperand::Move(temp_id) => {
                        &self.current_func.temps[temp_id.as_usize()].ty
                    }
                    _ => &arandu_semantics::types::ArType::Error,
                };
                let enum_ty = match base_ty {
                    arandu_semantics::types::ArType::Ptr(inner) => {
                        self.type_info.resolve_type_id(*inner)
                    }
                    other => other,
                };
                let enum_id = match enum_ty {
                    ArType::Named(enum_id, _) => *enum_id,
                    _ => arandu_semantics::SymbolId(0),
                };

                let mut payload_offset = 0;
                if let Some(variants) =
                    arandu_semantics::layout::StructLayoutProvider::get_enum_variants(
                        self.type_info,
                        enum_id,
                    )
                {
                    let tag = self
                        .type_info
                        .enum_variant_tags
                        .get(variant)
                        .copied()
                        .unwrap_or(0);
                    if let Some(variant_shape) = variants.get(tag) {
                        if let Some(payload_ty_id) = variant_shape.payload_ty {
                            let payload_ty = self.type_info.resolve_type_id(payload_ty_id);
                            let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                            let payload_layout = engine.layout_of_type(
                                payload_ty,
                                &self.type_info.type_interner,
                                self.type_info,
                            );
                            if *index < payload_layout.field_offsets.len() {
                                payload_offset = payload_layout.field_offsets[*index] as i32;
                            }
                        }
                    }
                }

                let total_offset = pointer_width as i32 + payload_offset;
                let loaded_ptr = self.builder.ins().load(
                    self.ptr_type,
                    cranelift_codegen::ir::MemFlagsData::new(),
                    ptr_val,
                    total_offset,
                );
                let loaded_len = self.builder.ins().load(
                    cranelift_codegen::ir::types::I64,
                    cranelift_codegen::ir::MemFlagsData::new(),
                    ptr_val,
                    total_offset + self.ptr_type.bytes() as i32,
                );
                (loaded_ptr, loaded_len)
            }
            _ => {
                self.record_ice(
                    "unsupported rvalue kind returning str in codegen",
                    self.func_span(),
                );
                (self.poison_i32(), self.poison_i32())
            }
        }
    }

    pub(super) fn translate_place_address_for_load(&mut self, place: &AmirPlace) -> (Value, i32) {
        let mut ptr_val = if let Some(&var) = self.local_map.get(&place.local) {
            self.builder.use_var(var)
        } else if let Some(&(var_ptr, _)) = self.str_local_map.get(&place.local) {
            self.builder.use_var(var_ptr)
        } else {
            self.record_ice(
                "use of undeclared AMIR local in codegen",
                self.local_span(place.local),
            );
            return (self.poison_i32(), 0);
        };

        let mut current_ty = self.current_func.locals[place.local.as_usize()].ty.clone();

        for i in 0..place.projections.len().saturating_sub(1) {
            let proj = &place.projections[i];
            match proj {
                AmirProjection::Field(symbol_id) => {
                    let offset = self.translate_projection_offset(&mut current_ty, *symbol_id);
                    ptr_val = self.builder.ins().load(
                        self.ptr_type,
                        cranelift_codegen::ir::MemFlagsData::new(),
                        ptr_val,
                        offset,
                    );
                }
                AmirProjection::Index(op) => {
                    let idx_val = self.translate_operand(op, Some(self.ptr_type));
                    let inner_ty_id = match &current_ty {
                        ArType::Ptr(inner) | ArType::Slice(inner) | ArType::Array(_, inner) => {
                            *inner
                        }
                        _ => {
                            self.record_ice(
                                "indexing non-indexable type in codegen",
                                self.local_span(place.local),
                            );
                            return (self.poison_i32(), 0);
                        }
                    };
                    let inner_ty = self.type_info.resolve_type_id(inner_ty_id).clone();
                    current_ty = inner_ty;

                    let pointer_width = self.ptr_type.bytes() as u64;
                    let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                    let layout = engine.layout_of_type(
                        &current_ty,
                        &self.type_info.type_interner,
                        self.type_info,
                    );
                    let elem_size = self.builder.ins().iconst(self.ptr_type, layout.size as i64);
                    let offset_val = self.builder.ins().imul(idx_val, elem_size);
                    ptr_val = self.builder.ins().iadd(ptr_val, offset_val);
                    ptr_val = self.builder.ins().load(
                        self.ptr_type,
                        cranelift_codegen::ir::MemFlagsData::new(),
                        ptr_val,
                        0,
                    );
                }
            }
        }

        let Some(last_proj) = place.projections.last() else {
            return (ptr_val, 0);
        };
        match last_proj {
            AmirProjection::Field(symbol_id) => {
                let offset = self.translate_projection_offset(&mut current_ty, *symbol_id);
                (ptr_val, offset)
            }
            AmirProjection::Index(op) => {
                let idx_val = self.translate_operand(op, Some(self.ptr_type));
                let inner_ty_id = match &current_ty {
                    ArType::Ptr(inner) | ArType::Slice(inner) | ArType::Array(_, inner) => *inner,
                    _ => {
                        self.record_ice(
                            "indexing non-indexable type in codegen",
                            self.local_span(place.local),
                        );
                        return (self.poison_i32(), 0);
                    }
                };
                let inner_ty = self.type_info.resolve_type_id(inner_ty_id).clone();
                current_ty = inner_ty;

                let pointer_width = self.ptr_type.bytes() as u64;
                let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                let layout = engine.layout_of_type(
                    &current_ty,
                    &self.type_info.type_interner,
                    self.type_info,
                );
                let elem_size = self.builder.ins().iconst(self.ptr_type, layout.size as i64);
                let offset_val = self.builder.ins().imul(idx_val, elem_size);
                let target_ptr = self.builder.ins().iadd(ptr_val, offset_val);
                (target_ptr, 0)
            }
        }
    }

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
                            self.builder
                                .ins()
                                .symbol_value(self.ptr_type, local_data_ref)
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
            AmirOperand::FunctionRef(sym_id) => {
                let sym = self.symbol_table.get(*sym_id);
                let func_id = match self.func_ids.get(&sym.name) {
                    Some(func_id) => *func_id,
                    None => {
                        self.record_ice(
                            format!("function '{}' was not declared in the JIT module", sym.name),
                            sym.span,
                        );
                        return self.poison_i32();
                    }
                };
                let local_ref = self.module.declare_func_in_func(func_id, self.builder.func);
                self.builder.ins().func_addr(self.ptr_type, local_ref)
            }
            AmirOperand::GlobalRef(_) => {
                self.record_ice(
                    "GlobalRef as operand should not appear after BC.2.2 zero-payload tuple fix.",
                    self.func_span(),
                );
                self.poison_i32()
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
        expected_ar_type: Option<&ArType>,
    ) -> Value {
        if self.error.is_some() {
            return self.poison_i32();
        }

        match rvalue {
            AmirRvalue::Use(op) => self.translate_operand(op, expected_ty),
            AmirRvalue::Binary { op, left, right } => {
                let opt_ty = match op {
                    arandu_semantics::ops::BinaryOp::Add
                    | arandu_semantics::ops::BinaryOp::Sub
                    | arandu_semantics::ops::BinaryOp::Mul
                    | arandu_semantics::ops::BinaryOp::Div
                    | arandu_semantics::ops::BinaryOp::Mod => expected_ty,
                    _ => None,
                };
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
                    let (base_ptr, offset) = self.translate_place_address_for_load(place);
                    let clif_ty = expected_ty.unwrap_or(self.ptr_type);
                    self.builder.ins().load(
                        clif_ty,
                        cranelift_codegen::ir::MemFlagsData::new(),
                        base_ptr,
                        offset,
                    )
                }
            }

            AmirRvalue::StructLiteral {
                struct_symbol,
                fields,
            } => {
                let Some(malloc_func_id) = self.malloc_func_id() else {
                    return self.poison_i32();
                };
                let local_ref = self
                    .module
                    .declare_func_in_func(malloc_func_id, self.builder.func);

                let pointer_width = self.ptr_type.bytes() as u64;
                let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                let struct_ty = expected_ar_type.cloned().unwrap_or_else(|| {
                    arandu_semantics::types::ArType::Named(*struct_symbol, Vec::new())
                });
                let layout = engine.layout_of_type(
                    &struct_ty,
                    &self.type_info.type_interner,
                    self.type_info,
                );

                let size_val = self.builder.ins().iconst(self.ptr_type, layout.size as i64);
                let call_inst = self.builder.ins().call(local_ref, &[size_val]);
                let ptr_val = self.builder.inst_results(call_inst)[0];

                for (i, (name, op)) in fields.iter().enumerate() {
                    let field_idx = self
                        .type_info
                        .struct_field_indices
                        .get(struct_symbol)
                        .and_then(|m| m.get(name.as_str()).copied())
                        .unwrap_or(i);
                    let offset = layout.field_offsets[field_idx] as i32;
                    let op_ty = self.get_operand_ar_type(op);
                    if matches!(op_ty, ArType::Primitive(Primitive::Str)) {
                        let (elem_ptr, elem_len) = self.translate_str_operand(op);
                        self.builder.ins().store(
                            cranelift_codegen::ir::MemFlagsData::new(),
                            elem_ptr,
                            ptr_val,
                            offset,
                        );
                        self.builder.ins().store(
                            cranelift_codegen::ir::MemFlagsData::new(),
                            elem_len,
                            ptr_val,
                            offset + pointer_width as i32,
                        );
                    } else {
                        let val = self.translate_operand(op, None);
                        self.builder.ins().store(
                            cranelift_codegen::ir::MemFlagsData::new(),
                            val,
                            ptr_val,
                            offset,
                        );
                    }
                }
                ptr_val
            }
            AmirRvalue::Tuple { items } => {
                let Some(malloc_func_id) = self.malloc_func_id() else {
                    return self.poison_i32();
                };
                let local_ref = self
                    .module
                    .declare_func_in_func(malloc_func_id, self.builder.func);

                let pointer_width = self.ptr_type.bytes() as u64;
                let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                let tuple_ty = expected_ar_type.cloned().unwrap_or(ArType::Error);
                let layout =
                    engine.layout_of_type(&tuple_ty, &self.type_info.type_interner, self.type_info);

                let size_val = self.builder.ins().iconst(self.ptr_type, layout.size as i64);
                let call_inst = self.builder.ins().call(local_ref, &[size_val]);
                let ptr_val = self.builder.inst_results(call_inst)[0];

                for (i, op) in items.iter().enumerate() {
                    let offset = layout.field_offsets[i] as i32;
                    let op_ty = self.get_operand_ar_type(op);
                    if matches!(op_ty, ArType::Primitive(Primitive::Str)) {
                        let (elem_ptr, elem_len) = self.translate_str_operand(op);
                        self.builder.ins().store(
                            cranelift_codegen::ir::MemFlagsData::new(),
                            elem_ptr,
                            ptr_val,
                            offset,
                        );
                        self.builder.ins().store(
                            cranelift_codegen::ir::MemFlagsData::new(),
                            elem_len,
                            ptr_val,
                            offset + pointer_width as i32,
                        );
                    } else {
                        let val = self.translate_operand(op, None);
                        self.builder.ins().store(
                            cranelift_codegen::ir::MemFlagsData::new(),
                            val,
                            ptr_val,
                            offset,
                        );
                    }
                }
                ptr_val
            }
            AmirRvalue::Array { items } => {
                let Some(malloc_func_id) = self.malloc_func_id() else {
                    return self.poison_i32();
                };
                let local_ref = self
                    .module
                    .declare_func_in_func(malloc_func_id, self.builder.func);

                let pointer_width = self.ptr_type.bytes() as u64;
                let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                let array_ty = expected_ar_type.cloned().unwrap_or(ArType::Error);
                let layout =
                    engine.layout_of_type(&array_ty, &self.type_info.type_interner, self.type_info);

                let size_val = self.builder.ins().iconst(self.ptr_type, layout.size as i64);
                let call_inst = self.builder.ins().call(local_ref, &[size_val]);
                let ptr_val = self.builder.inst_results(call_inst)[0];

                let item_ar_ty = match &array_ty {
                    ArType::Array(_, inner) => self.type_info.resolve_type_id(*inner).clone(),
                    _ => ArType::Error,
                };
                let item_layout = engine.layout_of_type(
                    &item_ar_ty,
                    &self.type_info.type_interner,
                    self.type_info,
                );
                let item_size = item_layout.size as i32;

                for (i, op) in items.iter().enumerate() {
                    let offset = i as i32 * item_size;
                    let op_ty = self.get_operand_ar_type(op);
                    if matches!(op_ty, ArType::Primitive(Primitive::Str)) {
                        let (elem_ptr, elem_len) = self.translate_str_operand(op);
                        self.builder.ins().store(
                            cranelift_codegen::ir::MemFlagsData::new(),
                            elem_ptr,
                            ptr_val,
                            offset,
                        );
                        self.builder.ins().store(
                            cranelift_codegen::ir::MemFlagsData::new(),
                            elem_len,
                            ptr_val,
                            offset + pointer_width as i32,
                        );
                    } else {
                        let val = self.translate_operand(op, None);
                        self.builder.ins().store(
                            cranelift_codegen::ir::MemFlagsData::new(),
                            val,
                            ptr_val,
                            offset,
                        );
                    }
                }
                ptr_val
            }

            AmirRvalue::FieldAccess { base, field } => {
                let ptr_val = self.translate_operand(base, Some(self.ptr_type));
                let base_ty = match base {
                    AmirOperand::Copy(temp_id) | AmirOperand::Move(temp_id) => {
                        &self.current_func.temps[temp_id.as_usize()].ty
                    }
                    _ => &arandu_semantics::types::ArType::Error,
                };
                let struct_ty = match base_ty {
                    arandu_semantics::types::ArType::Ptr(inner) => {
                        self.type_info.resolve_type_id(*inner)
                    }
                    other => other,
                };
                let pointer_width = self.ptr_type.bytes() as u64;
                let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                let layout =
                    engine.layout_of_type(struct_ty, &self.type_info.type_interner, self.type_info);
                let offset = layout.field_offsets[*field] as i32;

                let clif_ty = expected_ty.unwrap_or(self.ptr_type);
                self.builder.ins().load(
                    clif_ty,
                    cranelift_codegen::ir::MemFlagsData::new(),
                    ptr_val,
                    offset,
                )
            }
            AmirRvalue::EnumConstruct {
                variant_tag,
                payload,
            } => {
                let Some(malloc_func_id) = self.malloc_func_id() else {
                    return self.poison_i32();
                };
                let local_ref = self
                    .module
                    .declare_func_in_func(malloc_func_id, self.builder.func);

                let pointer_width = self.ptr_type.bytes() as u64;
                let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                let enum_ty = expected_ar_type.cloned().unwrap_or(ArType::Error);
                let layout =
                    engine.layout_of_type(&enum_ty, &self.type_info.type_interner, self.type_info);

                let size_val = self.builder.ins().iconst(self.ptr_type, layout.size as i64);
                let call_inst = self.builder.ins().call(local_ref, &[size_val]);
                let ptr_val = self.builder.inst_results(call_inst)[0];

                let tag_val = self
                    .builder
                    .ins()
                    .iconst(self.ptr_type, *variant_tag as i64);
                self.builder.ins().store(
                    cranelift_codegen::ir::MemFlagsData::new(),
                    tag_val,
                    ptr_val,
                    0,
                );

                if let Some(op) = payload {
                    let op_ty = self.get_operand_ar_type(op);
                    if matches!(op_ty, ArType::Primitive(Primitive::Str)) {
                        let (elem_ptr, elem_len) = self.translate_str_operand(op);
                        self.builder.ins().store(
                            cranelift_codegen::ir::MemFlagsData::new(),
                            elem_ptr,
                            ptr_val,
                            pointer_width as i32,
                        );
                        self.builder.ins().store(
                            cranelift_codegen::ir::MemFlagsData::new(),
                            elem_len,
                            ptr_val,
                            (pointer_width * 2) as i32,
                        );
                    } else {
                        let val = self.translate_operand(op, None);
                        self.builder.ins().store(
                            cranelift_codegen::ir::MemFlagsData::new(),
                            val,
                            ptr_val,
                            pointer_width as i32,
                        );
                    }
                }

                ptr_val
            }
            AmirRvalue::Discriminant { value } => {
                let ptr_val = self.translate_operand(value, Some(self.ptr_type));
                self.builder.ins().load(
                    self.ptr_type,
                    cranelift_codegen::ir::MemFlagsData::new(),
                    ptr_val,
                    0,
                )
            }
            AmirRvalue::EnumPayload {
                value,
                variant,
                index,
            } => {
                let ptr_val = self.translate_operand(value, Some(self.ptr_type));
                let pointer_width = self.ptr_type.bytes() as u64;

                let base_ty = match value {
                    AmirOperand::Copy(temp_id) | AmirOperand::Move(temp_id) => {
                        &self.current_func.temps[temp_id.as_usize()].ty
                    }
                    _ => &arandu_semantics::types::ArType::Error,
                };
                let enum_ty = match base_ty {
                    arandu_semantics::types::ArType::Ptr(inner) => {
                        self.type_info.resolve_type_id(*inner)
                    }
                    other => other,
                };
                let enum_id = match enum_ty {
                    ArType::Named(enum_id, _) => *enum_id,
                    _ => arandu_semantics::SymbolId(0),
                };

                let mut payload_offset = 0;
                if let Some(variants) =
                    arandu_semantics::layout::StructLayoutProvider::get_enum_variants(
                        self.type_info,
                        enum_id,
                    )
                {
                    let tag = self
                        .type_info
                        .enum_variant_tags
                        .get(variant)
                        .copied()
                        .unwrap_or(0);
                    if let Some(variant_shape) = variants.get(tag) {
                        if let Some(payload_ty_id) = variant_shape.payload_ty {
                            let payload_ty = self.type_info.resolve_type_id(payload_ty_id);
                            let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                            let payload_layout = engine.layout_of_type(
                                payload_ty,
                                &self.type_info.type_interner,
                                self.type_info,
                            );
                            if *index < payload_layout.field_offsets.len() {
                                payload_offset = payload_layout.field_offsets[*index] as i32;
                            }
                        }
                    }
                }

                let total_offset = pointer_width as i32 + payload_offset;
                let clif_ty = expected_ty.unwrap_or(self.ptr_type);
                self.builder.ins().load(
                    clif_ty,
                    cranelift_codegen::ir::MemFlagsData::new(),
                    ptr_val,
                    total_offset,
                )
            }
            AmirRvalue::IndexAccess { base, index } => {
                let ptr_val = self.translate_operand(base, Some(self.ptr_type));
                let mut idx_val = self.translate_operand(index, None);
                let idx_ty = self.builder.func.dfg.value_type(idx_val);
                if idx_ty != self.ptr_type {
                    idx_val = self.builder.ins().uextend(self.ptr_type, idx_val);
                }

                let base_ty = self.get_operand_ar_type(base);
                let deref_ty = match &base_ty {
                    ArType::Ptr(inner) => self.type_info.resolve_type_id(*inner),
                    other => other,
                };
                let elem_ty = match deref_ty {
                    ArType::Array(_, elem) => self.type_info.resolve_type_id(*elem),
                    ArType::Slice(elem) => self.type_info.resolve_type_id(*elem),
                    _ => &ArType::Error,
                };

                let pointer_width = self.ptr_type.bytes() as u64;
                let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                let layout =
                    engine.layout_of_type(elem_ty, &self.type_info.type_interner, self.type_info);

                let elem_size = self.builder.ins().iconst(self.ptr_type, layout.size as i64);
                let offset_val = self.builder.ins().imul(idx_val, elem_size);
                let target_ptr = self.builder.ins().iadd(ptr_val, offset_val);

                let clif_ty = expected_ty.unwrap_or(self.ptr_type);
                self.builder.ins().load(
                    clif_ty,
                    cranelift_codegen::ir::MemFlagsData::new(),
                    target_ptr,
                    0,
                )
            }
            AmirRvalue::Borrow(place) | AmirRvalue::BorrowMut(place) => {
                let ty = &self.current_func.locals[place.local.as_usize()].ty;
                let is_memory_backed = !place.projections.is_empty()
                    || matches!(
                        ty,
                        ArType::Tuple(_)
                            | ArType::Array(_, _)
                            | ArType::Slice(_)
                            | ArType::Primitive(Primitive::Str)
                    )
                    || matches!(
                        ty,
                        ArType::Named(sym_id, _) if matches!(
                            self.symbol_table.get(*sym_id).kind,
                            arandu_semantics::SymbolKind::Struct | arandu_semantics::SymbolKind::Enum
                        )
                    );

                if is_memory_backed {
                    let (base_ptr, offset) = self.translate_place_address_for_load(place);
                    if offset == 0 {
                        base_ptr
                    } else {
                        let offset_val = self.builder.ins().iconst(self.ptr_type, offset as i64);
                        self.builder.ins().iadd(base_ptr, offset_val)
                    }
                } else {
                    self.record_ice(
                        "Borrowing of stack locals is not supported in Cranelift JIT yet (requires F2.0)",
                        self.local_span(place.local),
                    );
                    self.poison_i32()
                }
            }
            _ => {
                self.record_ice(
                    format!(
                        "Rvalue kind {:?} not implemented in Cranelift JIT yet",
                        rvalue
                    ),
                    self.func_span(),
                );
                self.poison_i32()
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
                self.record_ice(
                    "Await unary operator is not implemented in Cranelift JIT yet (requires Phase 3)",
                    self.func_span(),
                );
                self.poison_i32()
            }
            _ => {
                self.record_ice(
                    format!(
                        "Unary operator {:?} not implemented in Cranelift JIT yet",
                        op
                    ),
                    self.func_span(),
                );
                self.poison_i32()
            }
        }
    }
}
