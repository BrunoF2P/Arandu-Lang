use arandu_semantics::amir::{AmirPlace, AmirProjection};
use arandu_semantics::passes::type_checker::types::ArType;
use cranelift_codegen::ir::{InstBuilder, Value};

use super::FunctionTranslator;

impl FunctionTranslator<'_, '_> {
    pub(super) fn translate_place_address_for_load(&mut self, place: &AmirPlace) -> (Value, i32) {
        // Scalar address-taken locals: address is the stack slot.
        // Pointer-valued aggregates: SSA local holds the base pointer.
        let mut ptr_val = if let Some(&slot) = self.local_stack_slots.get(&place.local) {
            self.builder.ins().stack_addr(self.ptr_type, slot, 0)
        } else if let Some(&var) = self.local_map.get(&place.local) {
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

        let mut current_ty = self.local_ar_ty(place.local);

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

    pub(super) fn translate_store_place(&mut self, lhs: &AmirPlace, val: Value) {
        if self.error.is_some() {
            return;
        }
        if lhs.projections.is_empty() {
            // Keep SSA var in sync for any non-memory path that still reads it,
            // and materialize to stack home when address-taken (F2.0).
            if let Some(&var) = self.local_map.get(&lhs.local) {
                self.builder.def_var(var, val);
            }
            if let Some(&slot) = self.local_stack_slots.get(&lhs.local) {
                let addr = self.builder.ins().stack_addr(self.ptr_type, slot, 0);
                self.builder
                    .ins()
                    .store(cranelift_codegen::ir::MemFlagsData::new(), val, addr, 0);
            } else if !self.local_map.contains_key(&lhs.local)
                && !self.str_local_map.contains_key(&lhs.local)
            {
                self.record_ice(
                    "use of undeclared AMIR local in codegen",
                    self.local_span(lhs.local),
                );
            }
        } else {
            let mut ptr_val = if let Some(&slot) = self.local_stack_slots.get(&lhs.local) {
                self.builder.ins().stack_addr(self.ptr_type, slot, 0)
            } else if let Some(&var) = self.local_map.get(&lhs.local) {
                self.builder.use_var(var)
            } else {
                self.record_ice(
                    "use of undeclared AMIR local in codegen",
                    self.local_span(lhs.local),
                );
                return;
            };

            let mut current_ty = self.local_ar_ty(lhs.local);

            for i in 0..lhs.projections.len() - 1 {
                let proj = &lhs.projections[i];
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

                        let struct_ty = match &current_ty {
                            arandu_semantics::types::ArType::Ptr(inner) => {
                                self.type_info.resolve_type_id(*inner)
                            }
                            other => other.clone(),
                        };
                        let inner_ty = match struct_ty {
                            arandu_semantics::types::ArType::Slice(inner)
                            | arandu_semantics::types::ArType::Array(_, inner)
                            | arandu_semantics::types::ArType::Ptr(inner) => {
                                self.type_info.resolve_type_id(inner)
                            }
                            _ => arandu_semantics::types::ArType::Error,
                        };
                        let pointer_width = self.ptr_type.bytes() as u64;
                        let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                        let layout = engine.layout_of_type(
                            &inner_ty,
                            &self.type_info.type_interner,
                            self.type_info,
                        );
                        let elem_size =
                            self.builder.ins().iconst(self.ptr_type, layout.size as i64);

                        let offset_val = self.builder.ins().imul(idx_val, elem_size);
                        let elem_ptr = self.builder.ins().iadd(ptr_val, offset_val);
                        ptr_val = self.builder.ins().load(
                            self.ptr_type,
                            cranelift_codegen::ir::MemFlagsData::new(),
                            elem_ptr,
                            0,
                        );
                        current_ty = inner_ty.clone();
                    }
                }
            }

            let Some(last_proj) = lhs.projections.last() else {
                return;
            };
            match last_proj {
                AmirProjection::Field(symbol_id) => {
                    let offset = self.translate_projection_offset(&mut current_ty, *symbol_id);
                    self.builder.ins().store(
                        cranelift_codegen::ir::MemFlagsData::new(),
                        val,
                        ptr_val,
                        offset,
                    );
                }
                AmirProjection::Index(op) => {
                    let idx_val = self.translate_operand(op, Some(self.ptr_type));

                    let struct_ty = match &current_ty {
                        arandu_semantics::types::ArType::Ptr(inner) => {
                            self.type_info.resolve_type_id(*inner)
                        }
                        other => other.clone(),
                    };
                    let inner_ty = match struct_ty {
                        arandu_semantics::types::ArType::Slice(inner)
                        | arandu_semantics::types::ArType::Array(_, inner)
                        | arandu_semantics::types::ArType::Ptr(inner) => {
                            self.type_info.resolve_type_id(inner)
                        }
                        _ => arandu_semantics::types::ArType::Error,
                    };
                    let pointer_width = self.ptr_type.bytes() as u64;
                    let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                    let layout = engine.layout_of_type(
                        &inner_ty,
                        &self.type_info.type_interner,
                        self.type_info,
                    );
                    let elem_size = self.builder.ins().iconst(self.ptr_type, layout.size as i64);

                    let offset_val = self.builder.ins().imul(idx_val, elem_size);
                    let target_ptr = self.builder.ins().iadd(ptr_val, offset_val);
                    self.builder.ins().store(
                        cranelift_codegen::ir::MemFlagsData::new(),
                        val,
                        target_ptr,
                        0,
                    );
                }
            }
        }
    }

    pub(super) fn translate_projection_offset(
        &self,
        current_ty: &mut arandu_semantics::types::ArType,
        symbol_id: arandu_semantics::SymbolId,
    ) -> i32 {
        let name = &self.symbol_table.get(symbol_id).name;

        let struct_ty = match &*current_ty {
            arandu_semantics::types::ArType::Ptr(inner) => self.type_info.resolve_type_id(*inner),
            other => other.clone(),
        };

        let (field_idx, next_ty) =
            if let arandu_semantics::types::ArType::Named(struct_symbol, ref generic_args) =
                struct_ty
            {
                let idx = self
                    .type_info
                    .struct_field_indices
                    .get(&struct_symbol)
                    .and_then(|m| m.get(name.as_str()).copied())
                    .unwrap_or(0);

                let fields_def = self.type_info.struct_fields.get(&struct_symbol);
                let field_ty = fields_def
                    .and_then(|m| m.get(name.as_str()).copied())
                    .map(|tid| self.type_info.resolve_type_id(tid))
                    .unwrap_or(arandu_semantics::types::ArType::Error);

                let generic_params = self
                    .type_info
                    .generic_params
                    .get(&struct_symbol)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]);
                let subst: rustc_hash::FxHashMap<
                    arandu_semantics::SymbolId,
                    arandu_semantics::types::TypeId,
                > = generic_params
                    .iter()
                    .copied()
                    .zip(generic_args.iter().copied())
                    .collect();

                let substituted =
                    substitute_projection_type(&field_ty, &subst, &self.type_info.type_interner);
                (idx, substituted)
            } else if let arandu_semantics::types::ArType::Result(ok, err) = struct_ty {
                let idx = if name == "ok" { 0 } else { 1 };
                let item_ty = if idx == 0 { ok } else { err };
                (idx, self.type_info.resolve_type_id(item_ty).clone())
            } else if let arandu_semantics::types::ArType::Option(inner) = struct_ty {
                let idx = if name == "some" { 1 } else { 0 };
                (idx, self.type_info.resolve_type_id(inner))
            } else if matches!(
                struct_ty,
                arandu_semantics::types::ArType::Primitive(arandu_semantics::types::Primitive::Str)
            ) || matches!(struct_ty, arandu_semantics::types::ArType::Slice(_))
            {
                let idx = match name.as_str() {
                    "buf" | "ptr" => 0,
                    "len" => 1,
                    _ => 0,
                };
                let item_ty = if idx == 0 {
                    arandu_semantics::types::ArType::Ptr(
                        self.type_info
                            .type_interner
                            .lookup(&arandu_semantics::types::ArType::Primitive(
                                arandu_semantics::types::Primitive::U8,
                            ))
                            .unwrap_or_else(|| arandu_semantics::types::TypeId::from_usize(5)),
                    )
                } else {
                    arandu_semantics::types::ArType::Primitive(
                        arandu_semantics::types::Primitive::U64,
                    )
                };
                (idx, item_ty)
            } else {
                (0, arandu_semantics::types::ArType::Error)
            };

        let pointer_width = self.ptr_type.bytes() as u64;
        let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
        let layout =
            engine.layout_of_type(&struct_ty, &self.type_info.type_interner, self.type_info);
        let offset = layout.field_offsets[field_idx] as i32;

        *current_ty = next_ty;
        offset
    }
}

pub(super) fn substitute_projection_type(
    ty: &arandu_semantics::types::ArType,
    subst: &rustc_hash::FxHashMap<arandu_semantics::SymbolId, arandu_semantics::types::TypeId>,
    interner: &arandu_semantics::types::TypeInterner,
) -> arandu_semantics::types::ArType {
    match ty {
        arandu_semantics::types::ArType::Named(id, args) => {
            if let Some(&concrete_id) = subst.get(id) {
                interner.resolve(concrete_id)
            } else {
                let new_args = args
                    .iter()
                    .map(|&arg_id| {
                        let arg_ty = interner.resolve(arg_id);
                        let substituted_arg = substitute_projection_type(&arg_ty, subst, interner);
                        interner.lookup(&substituted_arg).unwrap_or(arg_id)
                    })
                    .collect();
                arandu_semantics::types::ArType::Named(*id, new_args)
            }
        }
        arandu_semantics::types::ArType::Func(params, ret) => {
            let new_params = params
                .iter()
                .map(|&param_id| {
                    let param_ty = interner.resolve(param_id);
                    let substituted_param = substitute_projection_type(&param_ty, subst, interner);
                    interner.lookup(&substituted_param).unwrap_or(param_id)
                })
                .collect();
            let ret_ty = interner.resolve(*ret);
            let substituted_ret = substitute_projection_type(&ret_ty, subst, interner);
            let new_ret = interner.lookup(&substituted_ret).unwrap_or(*ret);
            arandu_semantics::types::ArType::Func(new_params, new_ret)
        }
        arandu_semantics::types::ArType::Nullable(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute_projection_type(&inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            arandu_semantics::types::ArType::Nullable(new_inner)
        }
        arandu_semantics::types::ArType::Slice(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute_projection_type(&inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            arandu_semantics::types::ArType::Slice(new_inner)
        }
        arandu_semantics::types::ArType::Array(len, inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute_projection_type(&inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            arandu_semantics::types::ArType::Array(*len, new_inner)
        }
        arandu_semantics::types::ArType::Ptr(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute_projection_type(&inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            arandu_semantics::types::ArType::Ptr(new_inner)
        }
        arandu_semantics::types::ArType::Tuple(tys) => {
            let new_tys = tys
                .iter()
                .map(|&ty_id| {
                    let item_ty = interner.resolve(ty_id);
                    let substituted_item = substitute_projection_type(&item_ty, subst, interner);
                    interner.lookup(&substituted_item).unwrap_or(ty_id)
                })
                .collect();
            arandu_semantics::types::ArType::Tuple(new_tys)
        }
        arandu_semantics::types::ArType::Result(ok, err) => {
            let ok_ty = interner.resolve(*ok);
            let substituted_ok = substitute_projection_type(&ok_ty, subst, interner);
            let new_ok = interner.lookup(&substituted_ok).unwrap_or(*ok);

            let err_ty = interner.resolve(*err);
            let substituted_err = substitute_projection_type(&err_ty, subst, interner);
            let new_err = interner.lookup(&substituted_err).unwrap_or(*err);

            arandu_semantics::types::ArType::Result(new_ok, new_err)
        }
        arandu_semantics::types::ArType::Option(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute_projection_type(&inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            arandu_semantics::types::ArType::Option(new_inner)
        }
        arandu_semantics::types::ArType::Coroutine(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute_projection_type(&inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            arandu_semantics::types::ArType::Coroutine(new_inner)
        }
        arandu_semantics::types::ArType::Range(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute_projection_type(&inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            arandu_semantics::types::ArType::Range(new_inner)
        }
        other => other.clone(),
    }
}
