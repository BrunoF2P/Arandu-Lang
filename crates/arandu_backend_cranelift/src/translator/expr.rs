use arandu_semantics::amir::{AmirConstant, AmirOperand, AmirRvalue};
use arandu_semantics::ops::UnaryOp;
use arandu_semantics::passes::type_checker::types::{ArType, Primitive};
use cranelift_codegen::ir::{InstBuilder, Type, Value};
use cranelift_module::Module;

use super::FunctionTranslator;

impl FunctionTranslator<'_, '_> {
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
                        self.temp_ar_ty(*temp_id)
                    }
                    _ => arandu_semantics::types::ArType::Error,
                };
                let struct_ty = match base_ty {
                    arandu_semantics::types::ArType::Ptr(inner) => {
                        self.type_info.resolve_type_id(inner)
                    }
                    other => other,
                };
                let pointer_width = self.ptr_type.bytes() as u64;
                let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                let layout = engine.layout_of_type(
                    &struct_ty,
                    &self.type_info.type_interner,
                    self.type_info,
                );
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
                        self.temp_ar_ty(*temp_id)
                    }
                    _ => arandu_semantics::types::ArType::Error,
                };
                let enum_ty = match base_ty {
                    arandu_semantics::types::ArType::Ptr(inner) => {
                        self.type_info.resolve_type_id(inner)
                    }
                    other => other,
                };
                let enum_id = match enum_ty {
                    ArType::Named(enum_id, _) => enum_id,
                    _ => arandu_semantics::SymbolId::DUMMY,
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
                                &payload_ty,
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
            AmirRvalue::StringInterp { parts } => self.translate_string_interp(parts),
            AmirRvalue::ToStr { value, src_ty } => self.translate_to_str(value, *src_ty),
            _ => {
                self.record_ice(
                    "unsupported rvalue kind returning str in codegen",
                    self.func_span(),
                );
                (self.poison_i32(), self.poison_i32())
            }
        }
    }

    /// Format a primitive via host ToStr helpers → `(ptr, len)`.
    fn translate_to_str(
        &mut self,
        value: &AmirOperand,
        src_ty: arandu_semantics::types::TypeId,
    ) -> (Value, Value) {
        let ar_ty = self.type_info.type_interner.resolve(src_ty);
        if matches!(ar_ty, ArType::Primitive(Primitive::Str)) {
            return self.translate_str_operand(value);
        }

        let i64_ty = cranelift_codegen::ir::types::I64;
        let helper_name = match &ar_ty {
            ArType::Primitive(Primitive::Bool) => "ar_jit_bool_to_str",
            ArType::Primitive(Primitive::Char) => "ar_jit_char_to_str",
            ArType::FloatLiteral => "ar_jit_f64_to_str",
            ArType::Primitive(p) if p.is_float() => "ar_jit_f64_to_str",
            ArType::IntLiteral => "ar_jit_i64_to_str",
            ArType::Primitive(p) if p.is_integer() && p.is_signed() => "ar_jit_i64_to_str",
            ArType::Primitive(p) if p.is_integer() => "ar_jit_u64_to_str",
            ArType::Err => "ar_jit_err_to_str",
            _ => {
                self.record_ice(
                    format!("ToStr v0.1 unsupported type in Cranelift: {ar_ty:?}"),
                    self.func_span(),
                );
                return (self.poison_i32(), self.poison_i32());
            }
        };

        // Stack slot for out_len: i64
        let len_slot = self
            .builder
            .create_sized_stack_slot(cranelift_codegen::ir::StackSlotData {
                kind: cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                size: 8,
                align_shift: 3,
                key: None,
            });
        let len_ptr = self.builder.ins().stack_addr(self.ptr_type, len_slot, 0);

        let arg = match helper_name {
            "ar_jit_bool_to_str" => {
                let v = self.translate_operand(value, Some(cranelift_codegen::ir::types::I8));
                // Ensure i8 width
                if self.builder.func.dfg.value_type(v) != cranelift_codegen::ir::types::I8 {
                    self.builder
                        .ins()
                        .ireduce(cranelift_codegen::ir::types::I8, v)
                } else {
                    v
                }
            }
            "ar_jit_char_to_str" => {
                let v = self.translate_operand(value, Some(cranelift_codegen::ir::types::I32));
                let vt = self.builder.func.dfg.value_type(v);
                if vt == cranelift_codegen::ir::types::I32 {
                    v
                } else if vt.bits() < 32 {
                    self.builder
                        .ins()
                        .uextend(cranelift_codegen::ir::types::I32, v)
                } else {
                    self.builder
                        .ins()
                        .ireduce(cranelift_codegen::ir::types::I32, v)
                }
            }
            "ar_jit_f64_to_str" => {
                let v = self.translate_operand(value, Some(cranelift_codegen::ir::types::F64));
                let vt = self.builder.func.dfg.value_type(v);
                if vt == cranelift_codegen::ir::types::F64 {
                    v
                } else if vt == cranelift_codegen::ir::types::F32 {
                    self.builder
                        .ins()
                        .fpromote(cranelift_codegen::ir::types::F64, v)
                } else {
                    v
                }
            }
            "ar_jit_i64_to_str" | "ar_jit_u64_to_str" => {
                let v = self.translate_operand(value, Some(i64_ty));
                let vt = self.builder.func.dfg.value_type(v);
                if vt == i64_ty {
                    v
                } else if vt.bits() < 64 {
                    if helper_name == "ar_jit_u64_to_str" {
                        self.builder.ins().uextend(i64_ty, v)
                    } else {
                        self.builder.ins().sextend(i64_ty, v)
                    }
                } else {
                    self.builder.ins().ireduce(i64_ty, v)
                }
            }
            "ar_jit_err_to_str" => self.translate_operand(value, Some(self.ptr_type)),
            _ => unreachable!(),
        };

        let Some(func_id) = self.func_ids.get(helper_name).copied() else {
            self.record_ice(
                format!("{helper_name} was not declared in the JIT module"),
                self.func_span(),
            );
            return (self.poison_i32(), self.poison_i32());
        };
        let local_ref = self.module.declare_func_in_func(func_id, self.builder.func);
        let call = self.builder.ins().call(local_ref, &[arg, len_ptr]);
        let ptr = self.builder.inst_results(call)[0];
        let len = self.builder.ins().stack_load(i64_ty, len_slot, 0);
        (ptr, len)
    }

    /// Compare two `str` fat pointers for equality (`==` / `!=`).
    /// Equal when lengths match and `memcmp` reports zero (empty strings included).
    fn translate_str_eq(
        &mut self,
        left: &AmirOperand,
        right: &AmirOperand,
        op: arandu_semantics::ops::BinaryOp,
    ) -> Value {
        let (l_ptr, l_len) = self.translate_str_operand(left);
        let (r_ptr, r_len) = self.translate_str_operand(right);
        let i8_ty = cranelift_codegen::ir::types::I8;
        let i32_ty = cranelift_codegen::ir::types::I32;
        let i64_ty = cranelift_codegen::ir::types::I64;

        let len_eq =
            self.builder
                .ins()
                .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, l_len, r_len);

        // If lengths differ → not equal. If both zero-length → equal.
        // Else memcmp(l_ptr, r_ptr, len) == 0.
        let zero_i64 = self.builder.ins().iconst(i64_ty, 0);
        let len_nonzero = self.builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::NotEqual,
            l_len,
            zero_i64,
        );

        let Some(memcmp_id) = self.memcmp_func_id() else {
            return self.poison_i32();
        };
        let memcmp_ref = self
            .module
            .declare_func_in_func(memcmp_id, self.builder.func);
        // memcmp size is size_t (= pointer width).
        let size = if self.ptr_type == i64_ty {
            l_len
        } else if self.ptr_type.bits() < 64 {
            self.builder.ins().ireduce(self.ptr_type, l_len)
        } else {
            self.builder.ins().uextend(self.ptr_type, l_len)
        };
        let call = self.builder.ins().call(memcmp_ref, &[l_ptr, r_ptr, size]);
        let cmp = self.builder.inst_results(call)[0];
        let zero_i32 = self.builder.ins().iconst(i32_ty, 0);
        let bytes_eq = self.builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            cmp,
            zero_i32,
        );

        // content_eq = !len_nonzero || bytes_eq  (icmp results are already i8 bools)
        let not_nonzero = self.builder.ins().bxor_imm(len_nonzero, 1);
        let content_eq = self.builder.ins().bor(not_nonzero, bytes_eq);
        let eq = self.builder.ins().band(len_eq, content_eq);
        let _ = i8_ty;

        match op {
            arandu_semantics::ops::BinaryOp::Equal => eq,
            arandu_semantics::ops::BinaryOp::NotEqual => self.builder.ins().bxor_imm(eq, 1),
            _ => self.poison_i32(),
        }
    }

    /// Concatenate `str` fat-pointer parts via `malloc` + `memcpy`.
    /// Returns `(ptr, len)` for the newly allocated buffer (not freed; debug/JIT lifetime).
    fn translate_string_interp(&mut self, parts: &[AmirOperand]) -> (Value, Value) {
        let i64_ty = cranelift_codegen::ir::types::I64;
        if parts.is_empty() {
            let empty_ptr = self.builder.ins().iconst(self.ptr_type, 0);
            let empty_len = self.builder.ins().iconst(i64_ty, 0);
            return (empty_ptr, empty_len);
        }

        // Materialize each part as (ptr, len).
        let mut part_vals: Vec<(Value, Value)> = Vec::with_capacity(parts.len());
        for part in parts {
            part_vals.push(self.translate_str_operand(part));
        }

        // total = sum of lengths (i64)
        let mut total = self.builder.ins().iconst(i64_ty, 0);
        for &(_, len) in &part_vals {
            total = self.builder.ins().iadd(total, len);
        }

        // malloc(total + 1) for trailing NUL safety
        let one = self.builder.ins().iconst(i64_ty, 1);
        let alloc_size_i64 = self.builder.ins().iadd(total, one);
        let alloc_size = if self.ptr_type == i64_ty {
            alloc_size_i64
        } else if self.ptr_type.bits() < 64 {
            self.builder.ins().ireduce(self.ptr_type, alloc_size_i64)
        } else {
            self.builder.ins().uextend(self.ptr_type, alloc_size_i64)
        };

        let Some(malloc_id) = self.malloc_func_id() else {
            return (self.poison_i32(), self.poison_i32());
        };
        let malloc_ref = self
            .module
            .declare_func_in_func(malloc_id, self.builder.func);
        let call = self.builder.ins().call(malloc_ref, &[alloc_size]);
        let buf = self.builder.inst_results(call)[0];

        let Some(memcpy_id) = self.memcpy_func_id() else {
            return (self.poison_i32(), self.poison_i32());
        };
        let memcpy_ref = self
            .module
            .declare_func_in_func(memcpy_id, self.builder.func);

        // Copy each part into the buffer.
        let mut offset_i64 = self.builder.ins().iconst(i64_ty, 0);
        for &(src_ptr, src_len) in &part_vals {
            let offset_ptr = if self.ptr_type == i64_ty {
                offset_i64
            } else if self.ptr_type.bits() < 64 {
                self.builder.ins().ireduce(self.ptr_type, offset_i64)
            } else {
                self.builder.ins().uextend(self.ptr_type, offset_i64)
            };
            let dest = self.builder.ins().iadd(buf, offset_ptr);
            let size = if self.ptr_type == i64_ty {
                src_len
            } else if self.ptr_type.bits() < 64 {
                self.builder.ins().ireduce(self.ptr_type, src_len)
            } else {
                self.builder.ins().uextend(self.ptr_type, src_len)
            };
            self.builder.ins().call(memcpy_ref, &[dest, src_ptr, size]);
            offset_i64 = self.builder.ins().iadd(offset_i64, src_len);
        }

        // Write trailing NUL at buf + total
        let zero_byte = self
            .builder
            .ins()
            .iconst(cranelift_codegen::ir::types::I8, 0);
        let total_ptr = if self.ptr_type == i64_ty {
            total
        } else if self.ptr_type.bits() < 64 {
            self.builder.ins().ireduce(self.ptr_type, total)
        } else {
            self.builder.ins().uextend(self.ptr_type, total)
        };
        let end_ptr = self.builder.ins().iadd(buf, total_ptr);
        self.builder.ins().store(
            cranelift_codegen::ir::MemFlagsData::new(),
            zero_byte,
            end_ptr,
            0,
        );

        (buf, total)
    }

    /// Box a scalar into a heap cell for `T?` (null-or-pointer ABI).
    fn box_nullable_scalar(&mut self, val: Value, inner: &ArType) -> Value {
        let Some(malloc_id) = self.malloc_func_id() else {
            return self.poison_i32();
        };
        let malloc_ref = self
            .module
            .declare_func_in_func(malloc_id, self.builder.func);
        let pointer_width = self.ptr_type.bytes() as u64;
        let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
        let layout = engine.layout_of_type(inner, &self.type_info.type_interner, self.type_info);
        let size = self
            .builder
            .ins()
            .iconst(self.ptr_type, layout.size.max(1) as i64);
        let call = self.builder.ins().call(malloc_ref, &[size]);
        let ptr = self.builder.inst_results(call)[0];
        self.builder
            .ins()
            .store(cranelift_codegen::ir::MemFlagsData::new(), val, ptr, 0);
        ptr
    }

    /// Load a boxed scalar from a non-null `T?` handle.
    fn unbox_nullable_scalar(&mut self, handle: Value, inner: &ArType) -> Value {
        let clif = match crate::types::clif_type(inner, self.ptr_type) {
            crate::types::ClifType::Concrete(t) => t,
            crate::types::ClifType::Void => return self.poison_i32(),
        };
        self.builder
            .ins()
            .load(clif, cranelift_codegen::ir::MemFlagsData::new(), handle, 0)
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

        // ── Nullable handle ABI ──────────────────────────────────────────
        // `T?` is always a pointer: null = nil; non-null = object ptr or
        // boxed scalar. Box/unbox keeps `int? = 0` distinct from `nil`.
        if let Some(ArType::Nullable(inner_id)) = expected_ar_type {
            let inner = self.type_info.type_interner.resolve(*inner_id);
            if matches!(
                rvalue,
                AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Nil))
            ) {
                return self.builder.ins().iconst(self.ptr_type, 0);
            }
            // Already a nullable handle (copy/move or nested) → pass through.
            if let AmirRvalue::Use(op) = rvalue {
                let op_ty = self.get_operand_ar_type(op);
                if matches!(op_ty, ArType::Nullable(_)) {
                    return self.translate_operand(op, Some(self.ptr_type));
                }
            }
            // Produce the inner value, then box scalars.
            let inner_clif = match crate::types::clif_type(&inner, self.ptr_type) {
                crate::types::ClifType::Concrete(t) => Some(t),
                crate::types::ClifType::Void => None,
            };
            let raw = self.translate_rvalue_inner(rvalue, inner_clif, Some(&inner));
            if inner.needs_nullable_box() {
                return self.box_nullable_scalar(raw, &inner);
            }
            return raw;
        }

        // Unbox when assigning a `T?` handle into a non-nullable `T` (e.g. `??`).
        if let AmirRvalue::Use(op) = rvalue {
            let op_ty = self.get_operand_ar_type(op);
            if let ArType::Nullable(inner_id) = &op_ty {
                let inner = self.type_info.type_interner.resolve(*inner_id);
                if expected_ar_type.is_none_or(|e| !matches!(e, ArType::Nullable(_)))
                    && inner.needs_nullable_box()
                {
                    let handle = self.translate_operand(op, Some(self.ptr_type));
                    return self.unbox_nullable_scalar(handle, &inner);
                }
            }
        }

        self.translate_rvalue_inner(rvalue, expected_ty, expected_ar_type)
    }

    fn translate_rvalue_inner(
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
                // `str` equality uses fat pointers + memcmp (not scalar icmp).
                let left_is_str = matches!(
                    self.get_operand_ar_type(left),
                    ArType::Primitive(Primitive::Str)
                );
                let right_is_str = matches!(
                    self.get_operand_ar_type(right),
                    ArType::Primitive(Primitive::Str)
                );
                if left_is_str || right_is_str {
                    match op {
                        arandu_semantics::ops::BinaryOp::Equal
                        | arandu_semantics::ops::BinaryOp::NotEqual => {
                            return self.translate_str_eq(left, right, *op);
                        }
                        _ => {
                            self.record_ice(
                                "unsupported binary op on str in codegen",
                                self.func_span(),
                            );
                            return self.poison_i32();
                        }
                    }
                }
                let opt_ty = match op {
                    arandu_semantics::ops::BinaryOp::Add
                    | arandu_semantics::ops::BinaryOp::Sub
                    | arandu_semantics::ops::BinaryOp::Mul
                    | arandu_semantics::ops::BinaryOp::Div
                    | arandu_semantics::ops::BinaryOp::Mod => expected_ty,
                    // Comparisons (incl. `x == nil` / `x != nil`): prefer the
                    // non-constant side's ABI type so Nil is a zero of matching width.
                    arandu_semantics::ops::BinaryOp::Equal
                    | arandu_semantics::ops::BinaryOp::NotEqual
                    | arandu_semantics::ops::BinaryOp::Lt
                    | arandu_semantics::ops::BinaryOp::LtEqual
                    | arandu_semantics::ops::BinaryOp::Gt
                    | arandu_semantics::ops::BinaryOp::GtEqual => {
                        let left_ty = match left {
                            AmirOperand::Copy(t) | AmirOperand::Move(t) => {
                                self.get_temp_clif_type(*t)
                            }
                            _ => None,
                        };
                        let right_ty = match right {
                            AmirOperand::Copy(t) | AmirOperand::Move(t) => {
                                self.get_temp_clif_type(*t)
                            }
                            _ => None,
                        };
                        left_ty.or(right_ty).or(expected_ty)
                    }
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
                    ArType::Array(_, inner) => self.type_info.resolve_type_id(*inner),
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
                        self.temp_ar_ty(*temp_id)
                    }
                    _ => arandu_semantics::types::ArType::Error,
                };
                // Unwrap ptr / nullable so layout sees the struct/tuple payload.
                let struct_ty = match base_ty {
                    arandu_semantics::types::ArType::Ptr(inner)
                    | arandu_semantics::types::ArType::Nullable(inner) => {
                        self.type_info.resolve_type_id(inner)
                    }
                    other => other,
                };
                let pointer_width = self.ptr_type.bytes() as u64;
                let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                let layout = engine.layout_of_type(
                    &struct_ty,
                    &self.type_info.type_interner,
                    self.type_info,
                );
                let Some(&off) = layout.field_offsets.get(*field) else {
                    // Dead `p?.field` access branch with nil/ZST base, or incomplete layout.
                    return self.poison_i32();
                };
                let offset = off as i32;

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
                    // ZST payloads (void / typeck error) only need the discriminant tag.
                    // `Err` is a message handle (pointer) and is stored like other scalars.
                    if matches!(op_ty, ArType::Void | ArType::Error) {
                        // no payload bytes
                    } else if matches!(op_ty, ArType::Primitive(Primitive::Str)) {
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
                        self.temp_ar_ty(*temp_id)
                    }
                    _ => arandu_semantics::types::ArType::Error,
                };
                let enum_ty = match base_ty {
                    arandu_semantics::types::ArType::Ptr(inner) => {
                        self.type_info.resolve_type_id(inner)
                    }
                    other => other,
                };
                let enum_id = match enum_ty {
                    ArType::Named(enum_id, _) => enum_id,
                    _ => arandu_semantics::SymbolId::DUMMY,
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
                                &payload_ty,
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
                    other => other.clone(),
                };
                let elem_ty = match deref_ty {
                    ArType::Array(_, elem) => self.type_info.resolve_type_id(elem),
                    ArType::Slice(elem) => self.type_info.resolve_type_id(elem),
                    _ => ArType::Error,
                };

                let pointer_width = self.ptr_type.bytes() as u64;
                let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                let layout =
                    engine.layout_of_type(&elem_ty, &self.type_info.type_interner, self.type_info);

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
                let ty = self.local_ar_ty(place.local);
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
                            self.symbol_table.get(sym_id).kind,
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
                    self.record_error(
                        arandu_semantics::DiagCode::U001FeatureNotSupported,
                        "borrow de variável escalar sem endereço físico ainda não suportado — requer F2.0/address_taken (Fase 3)",
                        self.local_span(place.local),
                    );
                    self.poison_i32()
                }
            }
            AmirRvalue::Len(op) => self.translate_len(op, expected_ty),
            AmirRvalue::Alloc(op) => self.translate_alloc(op),
            AmirRvalue::ToStr { .. } | AmirRvalue::StringInterp { .. } => {
                // Fat-pointer results must go through translate_str_rvalue.
                self.record_ice(
                    "ToStr/StringInterp must be lowered via str rvalue path",
                    self.func_span(),
                );
                self.poison_i32()
            }
        }
    }

    /// `Len` for array (constant), `str` fat-pointer (SSA pair), slice (memory fat ptr).
    fn translate_len(&mut self, op: &AmirOperand, expected_ty: Option<Type>) -> Value {
        let op_ty = self.get_operand_ar_type(op);
        let i64_ty = cranelift_codegen::ir::types::I64;
        let result_ty = expected_ty.unwrap_or(self.ptr_type);

        match op_ty {
            ArType::Array(len, _) => {
                let v = self.builder.ins().iconst(i64_ty, len as i64);
                self.cast_int_width(v, result_ty)
            }
            ArType::Primitive(Primitive::Str) => {
                // Dual-value Str ABI: reuse the str operand path for temps + literals.
                let (_, len_val) = self.translate_str_operand(op);
                self.cast_int_width(len_val, result_ty)
            }
            ArType::Slice(_) => {
                // Slice fat pointer in memory: {ptr @0, len @pointer_width}.
                let base = self.translate_operand(op, Some(self.ptr_type));
                let len_off = self.ptr_type.bytes() as i32;
                let len_val = self.builder.ins().load(
                    i64_ty,
                    cranelift_codegen::ir::MemFlagsData::new(),
                    base,
                    len_off,
                );
                self.cast_int_width(len_val, result_ty)
            }
            _ => {
                self.record_ice(
                    format!("Len not supported for type {op_ty:?}"),
                    self.func_span(),
                );
                self.poison_i32()
            }
        }
    }

    /// Byte-count heap allocation via `malloc` (RC-RVALUE-GAPS).
    fn translate_alloc(&mut self, op: &AmirOperand) -> Value {
        let size_val = self.translate_operand(op, Some(self.ptr_type));
        let Some(malloc_id) = self.malloc_func_id() else {
            return self.poison_i32();
        };
        let malloc_ref = self
            .module
            .declare_func_in_func(malloc_id, self.builder.func);
        let call = self.builder.ins().call(malloc_ref, &[size_val]);
        self.builder.inst_results(call)[0]
    }

    fn cast_int_width(&mut self, val: Value, target: Type) -> Value {
        let src = self.builder.func.dfg.value_type(val);
        if src == target {
            return val;
        }
        if src.bits() < target.bits() {
            self.builder.ins().uextend(target, val)
        } else if src.bits() > target.bits() {
            self.builder.ins().ireduce(target, val)
        } else {
            val
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
                self.record_error(
                    arandu_semantics::DiagCode::U001FeatureNotSupported,
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
