use super::CEmitter;
use arandu_middle::amir::{AmirFunc, AmirOperand, AmirRvalue};
use arandu_middle::ops::{BinaryOp, UnaryOp};
use arandu_middle::types::{ArType, Primitive};
use std::fmt::Write;

impl<'a> CEmitter<'a> {
    pub(super) fn emit_rvalue(
        &mut self,
        rvalue: &AmirRvalue,
        func: &AmirFunc,
        expected_ar_type: &ArType,
        expected_c_type: &str,
    ) {
        match rvalue {
            AmirRvalue::Use(op) => {
                let op_str = self.format_operand(op, func);
                let _ = write!(&mut self.output, "{}", op_str);
            }
            AmirRvalue::Binary { op, left, right } => {
                let left_str = self.format_operand(left, func);
                let right_str = self.format_operand(right, func);
                let op_str = match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::Div => "/",
                    BinaryOp::Mod => "%",
                    BinaryOp::Equal => "==",
                    BinaryOp::NotEqual => "!=",
                    BinaryOp::Lt => "<",
                    BinaryOp::LtEqual => "<=",
                    BinaryOp::Gt => ">",
                    BinaryOp::GtEqual => ">=",
                    BinaryOp::And => "&&",
                    BinaryOp::Or => "||",
                    BinaryOp::BitAnd => "&",
                    BinaryOp::BitOr => "|",
                    BinaryOp::BitXor => "^",
                    BinaryOp::ShiftLeft => "<<",
                    BinaryOp::ShiftRight => ">>",
                    _ => "?",
                };
                let _ = write!(&mut self.output, "{} {} {}", left_str, op_str, right_str);
            }
            AmirRvalue::FieldAccess { base, field } => {
                let base_ty = match base {
                    AmirOperand::Copy(t) | AmirOperand::Move(t) => self.temp_ty(func, *t),
                    _ => {
                        let _ = write!(&mut self.output, "/* unsupported base operand */");
                        return;
                    }
                };
                let struct_ty = match base_ty {
                    ArType::Ptr(inner) => self.interner.resolve(inner),
                    other => other,
                };
                let layout = self
                    .layout
                    .layout_of_type(&struct_ty, self.interner, self.provider);
                let offset = layout.field_offsets[*field];

                let base_temp = match base {
                    AmirOperand::Copy(t) | AmirOperand::Move(t) => t.as_usize(),
                    _ => 0,
                };
                let _ = write!(
                    &mut self.output,
                    "*({}*)((uint8_t*)&t{} + {})",
                    expected_c_type, base_temp, offset
                );
            }
            AmirRvalue::Discriminant { value } => {
                let base_temp = match value {
                    AmirOperand::Copy(t) | AmirOperand::Move(t) => t.as_usize(),
                    _ => {
                        let _ = write!(&mut self.output, "/* unsupported base */");
                        return;
                    }
                };
                let _ = write!(
                    &mut self.output,
                    "*(int64_t*)((uint8_t*)&t{} + 0)",
                    base_temp
                );
            }
            AmirRvalue::EnumPayload {
                value,
                variant: _,
                index: _,
            } => {
                let base_temp = match value {
                    AmirOperand::Copy(t) | AmirOperand::Move(t) => t.as_usize(),
                    _ => {
                        let _ = write!(&mut self.output, "/* unsupported base */");
                        return;
                    }
                };

                let base_ty = self.interner.resolve(func.temps[base_temp].ty);
                let enum_ty = match base_ty {
                    ArType::Ptr(inner) => self.interner.resolve(inner),
                    other => other,
                };
                let enum_id = match enum_ty {
                    ArType::Named(id, _) => id,
                    _ => arandu_middle::SymbolId::DUMMY,
                };

                let mut payload_offset = 0;
                if arandu_middle::layout::StructLayoutProvider::get_enum_variants(
                    self.provider,
                    enum_id,
                )
                .is_some()
                {
                    // Tag is pointer-width on the target layout (i686 → 4, host64 → 8).
                    let tag_size = self.layout.pointer_width() as usize;
                    payload_offset = tag_size;
                }
                let _ = write!(
                    &mut self.output,
                    "*({}*)((uint8_t*)&t{} + {})",
                    expected_c_type, base_temp, payload_offset
                );
            }
            AmirRvalue::EnumConstruct {
                variant_tag,
                payload,
            } => {
                if let Some(p) = payload {
                    let payload_str = self.format_operand(p, func);
                    let payload_ty = match expected_ar_type {
                        ArType::Named(id, _) => self
                            .provider
                            .get_enum_variants(*id)
                            .and_then(|variants| {
                                variants.get(*variant_tag).and_then(|v| v.payload_ty)
                            })
                            .map(|ty_id| self.interner.resolve(ty_id))
                            .unwrap_or(ArType::Error),
                        ArType::Option(inner) => {
                            if *variant_tag == 1 {
                                self.interner.resolve(*inner)
                            } else {
                                ArType::Error
                            }
                        }
                        ArType::Result(ok, err) => {
                            if *variant_tag == 0 {
                                self.interner.resolve(*ok)
                            } else {
                                self.interner.resolve(*err)
                            }
                        }
                        _ => ArType::Error,
                    };
                    let payload_c_ty = self.format_type(&payload_ty);
                    let _ = write!(
                        &mut self.output,
                        "*({expected_c_type}*)&(struct {{ int64_t tag; {payload_c_ty} payload; }}){{ {}, {} }}",
                        variant_tag, payload_str
                    );
                } else {
                    let _ = write!(
                        &mut self.output,
                        "*({expected_c_type}*)&(struct {{ int64_t tag; }}){{ {} }}",
                        variant_tag
                    );
                }
            }
            AmirRvalue::StructLiteral {
                struct_symbol,
                fields,
            } => {
                let _ = write!(&mut self.output, "*({expected_c_type}*)&(struct {{");
                let struct_ty = arandu_middle::types::ArType::Named(*struct_symbol, Vec::new());
                let layout = self
                    .layout
                    .layout_of_type(&struct_ty, self.interner, self.provider);
                let field_defs = self.provider.get_struct_fields(*struct_symbol);
                let mut resolved_fields = Vec::new();
                for (i, (name, op)) in fields.iter().enumerate() {
                    let field_idx = match self.provider.get_struct_field_indices(*struct_symbol) {
                        Some(indices) => indices.get(name.as_str()).copied().unwrap_or(i),
                        None => i,
                    };
                    let offset = layout.field_offsets.get(field_idx).copied().unwrap_or(0);
                    let field_ty = field_defs
                        .and_then(|m| m.get(name.as_str()).copied())
                        .map(|tid| self.interner.resolve(tid))
                        .unwrap_or(ArType::Error);
                    let field_c_ty = self.format_type(&field_ty);
                    let op_str = self.format_operand(op, func);
                    resolved_fields.push((offset, field_c_ty, op_str));
                }
                resolved_fields.sort_by_key(|f| f.0);

                for (offset, field_c_ty, _) in &resolved_fields {
                    let _ = write!(&mut self.output, " {} f_{};", field_c_ty, offset);
                }
                let _ = write!(&mut self.output, "}}){{");
                for (i, (_, _, op_str)) in resolved_fields.iter().enumerate() {
                    if i > 0 {
                        let _ = write!(&mut self.output, ", ");
                    }
                    let _ = write!(&mut self.output, "{}", op_str);
                }
                let _ = write!(&mut self.output, "}}");
            }
            AmirRvalue::Unary { op, operand } => {
                let op_val = self.format_operand(operand, func);
                match op {
                    UnaryOp::Neg => {
                        let _ = write!(&mut self.output, "-{}", op_val);
                    }
                    UnaryOp::Not => {
                        let _ = write!(&mut self.output, "!{}", op_val);
                    }
                    UnaryOp::BitNot => {
                        let _ = write!(&mut self.output, "~{}", op_val);
                    }
                    // A3.6: await = block_on until Ready; disc@0, payload@8.
                    UnaryOp::Await => {
                        let _ = write!(
                            &mut self.output,
                            "({{ uint8_t* __ar_aw = (uint8_t*)({op_val}); \
                             {expected_c_type} __ar_av; \
                             if (*(uint32_t*)__ar_aw == 0) {{ \
                               __ar_av = *({expected_c_type}*)(__ar_aw + 8); \
                             }} else {{ \
                               __ar_av = ({expected_c_type})ar_co_block_on_i64(__ar_aw); \
                             }} \
                             __ar_av; }})"
                        );
                    }
                    _ => {
                        let _ = write!(&mut self.output, "{}", op_val);
                    }
                }
            }
            AmirRvalue::Load(place) => {
                let place_str = self.format_place(place, func);
                let _ = write!(&mut self.output, "{}", place_str);
            }
            AmirRvalue::Borrow(place) => {
                let place_str = self.format_place(place, func);
                let _ = write!(&mut self.output, "&{}", place_str);
            }
            AmirRvalue::BorrowMut(place) => {
                let place_str = self.format_place(place, func);
                let _ = write!(&mut self.output, "&{}", place_str);
            }
            AmirRvalue::Array { items } => {
                let elem_ty = match expected_ar_type {
                    ArType::Array(_, inner) => self.interner.resolve(*inner),
                    _ => ArType::Error,
                };
                let elem_c_ty = self.format_type(&elem_ty);
                let _ = write!(&mut self.output, "*({expected_c_type}*)&({elem_c_ty}[]){{");
                for (i, op) in items.iter().enumerate() {
                    if i > 0 {
                        let _ = write!(&mut self.output, ", ");
                    }
                    let op_str = self.format_operand(op, func);
                    let _ = write!(&mut self.output, "{}", op_str);
                }
                let _ = write!(&mut self.output, "}}");
            }
            AmirRvalue::Tuple { items } => {
                let tys = match expected_ar_type {
                    ArType::Tuple(tys) => tys.as_slice(),
                    _ => &[],
                };
                let _ = write!(&mut self.output, "*({expected_c_type}*)&(struct {{");
                for (i, _) in items.iter().enumerate() {
                    let field_ty = self.interner.resolve(tys[i]);
                    let field_c_ty = self.format_type(&field_ty);
                    let _ = write!(&mut self.output, " {} f_{};", field_c_ty, i);
                }
                let _ = write!(&mut self.output, "}}){{");
                for (i, op) in items.iter().enumerate() {
                    if i > 0 {
                        let _ = write!(&mut self.output, ", ");
                    }
                    let op_str = self.format_operand(op, func);
                    let _ = write!(&mut self.output, "{}", op_str);
                }
                let _ = write!(&mut self.output, "}}");
            }
            AmirRvalue::Len(op) => {
                let op_str = self.format_operand(op, func);
                let op_ty = match op {
                    AmirOperand::Copy(t) | AmirOperand::Move(t) => self.temp_ty(func, *t),
                    _ => ArType::Error,
                };
                if matches!(op_ty, ArType::Primitive(Primitive::Str)) {
                    // LayoutEngine Str fat pointer: second field is len.
                    let _ = write!(&mut self.output, "({}).len", op_str);
                } else if matches!(op_ty, ArType::Slice(_)) {
                    // Slice fat pointer: len offset from LayoutEngine (not magic +8).
                    let off = self.layout.fat_ptr_len_offset();
                    let len_ty = if self.layout.fat_ptr_len_size() == 4 {
                        "int32_t"
                    } else {
                        "int64_t"
                    };
                    let _ = write!(
                        &mut self.output,
                        "*({len_ty}*)((uint8_t*)&{op_str} + {off})"
                    );
                } else if let ArType::Array(len, _) = op_ty {
                    let _ = write!(&mut self.output, "{}", len);
                } else {
                    let _ = write!(&mut self.output, "/* unsupported Len operand */");
                }
            }
            AmirRvalue::IndexAccess { base, index } => {
                let base_ty = match base {
                    AmirOperand::Copy(t) | AmirOperand::Move(t) => self.temp_ty(func, *t),
                    _ => ArType::Error,
                };
                let elem_ty = match &base_ty {
                    ArType::Array(_, inner) | ArType::Slice(inner) | ArType::Ptr(inner) => {
                        self.interner.resolve(*inner)
                    }
                    _ => ArType::Error,
                };
                let elem_c_ty = self.format_type(&elem_ty);
                let base_str = self.format_operand(base, func);
                let index_str = self.format_operand(index, func);

                if matches!(base_ty, ArType::Ptr(_)) {
                    let _ = write!(
                        &mut self.output,
                        "(({}*){})[{}]",
                        elem_c_ty, base_str, index_str
                    );
                } else if matches!(base_ty, ArType::Slice(_)) {
                    let _ = write!(
                        &mut self.output,
                        "(({}*)(*(void**)((uint8_t*)&{} + 0)))[{}]",
                        elem_c_ty, base_str, index_str
                    );
                } else {
                    let _ = write!(
                        &mut self.output,
                        "(({}*)&{})[{}]",
                        elem_c_ty, base_str, index_str
                    );
                }
            }
            AmirRvalue::Alloc(op) => {
                // Byte-count allocation via libc malloc (RC-RVALUE-GAPS).
                let size_str = self.format_operand(op, func);
                let _ = write!(&mut self.output, "malloc((size_t)({}))", size_str);
            }
            // Heap CoroutineReady (stack:true is multi-stmt in emit_stmt).
            // A3.6 layout: disc@0 (u32 Ready=0), payload@8.
            AmirRvalue::CoroutineReady {
                value,
                payload_ty,
                stack: false,
            } => {
                let payload_ar = self.interner.resolve(*payload_ty);
                let payload_c = self.format_type(&payload_ar);
                let v = self.format_operand(value, func);
                let payload_size = self
                    .layout
                    .layout_of_type(&payload_ar, self.interner, self.provider)
                    .size
                    .max(1);
                let size = 8 + payload_size;
                let _ = write!(
                    &mut self.output,
                    "({{ uint8_t* __ar_co = (uint8_t*)malloc({size}); \
                     *(uint32_t*)__ar_co = 0; \
                     *({payload_c}*)(__ar_co + 8) = ({payload_c})({v}); \
                     (void*)__ar_co; }})"
                );
            }
            AmirRvalue::CoroutineReady { stack: true, .. } => {
                // Should have been handled as multi-stmt Assign; fallback null.
                let _ = write!(&mut self.output, "((void*)0)");
            }
            // A3.4: pin-free index (LocalId), not a raw address.
            AmirRvalue::RelativeBorrow { local, .. } => {
                let _ = write!(&mut self.output, "((void*)(uintptr_t){})", local.as_usize());
            }
            AmirRvalue::GenInsert { value } => {
                let v = self.format_operand(value, func);
                let _ = write!(&mut self.output, "ar_gen_insert_i64((int64_t)({v}))");
            }
            AmirRvalue::GenGet { gen_ref } => {
                let r = self.format_operand(gen_ref, func);
                let _ = write!(&mut self.output, "ar_gen_get_i64((int64_t)({r}))");
            }
            AmirRvalue::GenRemove { gen_ref } => {
                let r = self.format_operand(gen_ref, func);
                let _ = write!(&mut self.output, "ar_gen_remove_i64((int64_t)({r}))");
            }
            AmirRvalue::StringInterp { parts } => {
                // Emit a call to the runtime helper: ar_str_concat_n(n, part0, part1, ..., partN-1)
                // Each part must already be of type ArStr.
                let n = parts.len();
                let part_strs: Vec<String> =
                    parts.iter().map(|p| self.format_operand(p, func)).collect();
                let _ = write!(
                    &mut self.output,
                    "ar_str_concat_n({}, {})",
                    n,
                    part_strs.join(", ")
                );
            }
            AmirRvalue::ToStr { value, src_ty } => {
                use arandu_middle::types::{ArType, Primitive};
                let src = self.interner.resolve(*src_ty);
                let val = self.format_operand(value, func);
                match &src {
                    ArType::Primitive(Primitive::Str) => {
                        let _ = write!(&mut self.output, "{val}");
                    }
                    ArType::Primitive(Primitive::Bool) => {
                        let _ = write!(&mut self.output, "ar_bool_to_str({val})");
                    }
                    ArType::Primitive(Primitive::Char) => {
                        let _ = write!(&mut self.output, "ar_char_to_str((uint32_t)({val}))");
                    }
                    ArType::FloatLiteral => {
                        let _ = write!(&mut self.output, "ar_f64_to_str((double)({val}))");
                    }
                    ArType::Primitive(p) if p.is_float() => {
                        let _ = write!(&mut self.output, "ar_f64_to_str((double)({val}))");
                    }
                    ArType::IntLiteral => {
                        let _ = write!(&mut self.output, "ar_i64_to_str((int64_t)({val}))");
                    }
                    ArType::Primitive(p) if p.is_integer() && p.is_signed() => {
                        let _ = write!(&mut self.output, "ar_i64_to_str((int64_t)({val}))");
                    }
                    ArType::Primitive(p) if p.is_integer() => {
                        let _ = write!(&mut self.output, "ar_u64_to_str((uint64_t)({val}))");
                    }
                    other => {
                        let _ = write!(
                            &mut self.output,
                            "/* unsupported ToStr {:?} */ ar_str_pack((const uint8_t*)\"\", 0)",
                            other
                        );
                    }
                }
            }
        }
    }
}
