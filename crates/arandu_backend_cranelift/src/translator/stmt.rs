use arandu_semantics::amir::{
    AmirOperand, AmirPlace, AmirProjection, AmirStmt, AmirTerminator, TempId,
};
use arandu_semantics::passes::type_checker::types::{ArType, Primitive};
use cranelift_codegen::ir::{BlockArg, InstBuilder, TrapCode, Type, Value};
use cranelift_frontend::Switch;
use cranelift_module::{FuncId, Module};

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
                        let local_ref =
                            self.module.declare_func_in_func(func_id, self.builder.func);

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
                    _ => unimplemented!("Indirect function calls not implemented yet"),
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

            let mut current_ty = self.current_func.locals[lhs.local.as_usize()].ty.clone();

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
                            other => other,
                        };
                        let inner_ty = match struct_ty {
                            arandu_semantics::types::ArType::Slice(inner)
                            | arandu_semantics::types::ArType::Array(_, inner)
                            | arandu_semantics::types::ArType::Ptr(inner) => {
                                self.type_info.resolve_type_id(*inner)
                            }
                            _ => &arandu_semantics::types::ArType::Error,
                        };
                        let pointer_width = self.ptr_type.bytes() as u64;
                        let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                        let layout = engine.layout_of_type(
                            inner_ty,
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
                        other => other,
                    };
                    let inner_ty = match struct_ty {
                        arandu_semantics::types::ArType::Slice(inner)
                        | arandu_semantics::types::ArType::Array(_, inner)
                        | arandu_semantics::types::ArType::Ptr(inner) => {
                            self.type_info.resolve_type_id(*inner)
                        }
                        _ => &arandu_semantics::types::ArType::Error,
                    };
                    let pointer_width = self.ptr_type.bytes() as u64;
                    let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
                    let layout = engine.layout_of_type(
                        inner_ty,
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

    pub(super) fn free_func_id(&mut self) -> Option<FuncId> {
        match self.func_ids.get("free") {
            Some(func_id) => Some(*func_id),
            None => {
                self.record_ice("free was not declared in the JIT module", self.func_span());
                None
            }
        }
    }

    pub(super) fn emit_free_ptr(&mut self, ptr_val: Value) {
        let Some(free_func_id) = self.free_func_id() else {
            return;
        };
        let local_ref = self
            .module
            .declare_func_in_func(free_func_id, self.builder.func);

        #[cfg(debug_assertions)]
        {
            let poison_val = self.builder.ins().iconst(self.ptr_type, 0xDE_i64);
            self.builder.ins().store(
                cranelift_codegen::ir::MemFlagsData::new(),
                poison_val,
                ptr_val,
                0,
            );
        }

        self.builder.ins().call(local_ref, &[ptr_val]);
    }

    pub(super) fn translate_terminator(&mut self, terminator: &AmirTerminator) {
        match terminator {
            AmirTerminator::Return => {
                if matches!(
                    self.current_func.return_type,
                    ArType::Primitive(Primitive::Str)
                ) {
                    let ret_temp = TempId::from_usize(0);
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
                let disc_val = self.translate_operand(discriminant, None);
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

    pub(super) fn translate_projection_offset(
        &self,
        current_ty: &mut arandu_semantics::types::ArType,
        symbol_id: arandu_semantics::SymbolId,
    ) -> i32 {
        let name = &self.symbol_table.get(symbol_id).name;

        let struct_ty = match &*current_ty {
            arandu_semantics::types::ArType::Ptr(inner) => self.type_info.resolve_type_id(*inner),
            other => other,
        };

        let (field_idx, next_ty) = if let arandu_semantics::types::ArType::Named(
            struct_symbol,
            generic_args,
        ) = struct_ty
        {
            let idx = self
                .type_info
                .struct_field_indices
                .get(struct_symbol)
                .and_then(|m| m.get(name.as_str()).copied())
                .unwrap_or(0);

            let fields_def = self.type_info.struct_fields.get(struct_symbol);
            let field_ty = fields_def
                .and_then(|m| m.get(name.as_str()).cloned())
                .unwrap_or(arandu_semantics::types::ArType::Error);

            let generic_params = self
                .type_info
                .generic_params
                .get(struct_symbol)
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
            let item_ty = if idx == 0 { *ok } else { *err };
            (idx, self.type_info.resolve_type_id(item_ty).clone())
        } else if let arandu_semantics::types::ArType::Option(inner) = struct_ty {
            let idx = if name == "some" { 1 } else { 0 };
            (idx, self.type_info.resolve_type_id(*inner).clone())
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
                        .unwrap_or_else(|| {
                            // Fallback in case it's not found (though U8 must be interned during check)
                            arandu_semantics::types::TypeId::from_usize(5)
                        }),
                )
            } else {
                arandu_semantics::types::ArType::Primitive(arandu_semantics::types::Primitive::U64)
            };
            (idx, item_ty)
        } else {
            (0, arandu_semantics::types::ArType::Error)
        };

        let pointer_width = self.ptr_type.bytes() as u64;
        let engine = arandu_semantics::layout::LayoutEngine::new(pointer_width);
        let layout =
            engine.layout_of_type(struct_ty, &self.type_info.type_interner, self.type_info);
        let offset = layout.field_offsets[field_idx] as i32;

        *current_ty = next_ty;
        offset
    }
}

fn substitute_projection_type(
    ty: &arandu_semantics::types::ArType,
    subst: &rustc_hash::FxHashMap<arandu_semantics::SymbolId, arandu_semantics::types::TypeId>,
    interner: &arandu_semantics::types::TypeInterner,
) -> arandu_semantics::types::ArType {
    match ty {
        arandu_semantics::types::ArType::Named(id, args) => {
            if let Some(&concrete_id) = subst.get(id) {
                interner.resolve(concrete_id).clone()
            } else {
                let new_args = args
                    .iter()
                    .map(|&arg_id| {
                        let arg_ty = interner.resolve(arg_id);
                        let substituted_arg = substitute_projection_type(arg_ty, subst, interner);
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
                    let substituted_param = substitute_projection_type(param_ty, subst, interner);
                    interner.lookup(&substituted_param).unwrap_or(param_id)
                })
                .collect();
            let ret_ty = interner.resolve(*ret);
            let substituted_ret = substitute_projection_type(ret_ty, subst, interner);
            let new_ret = interner.lookup(&substituted_ret).unwrap_or(*ret);
            arandu_semantics::types::ArType::Func(new_params, new_ret)
        }
        arandu_semantics::types::ArType::Nullable(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute_projection_type(inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            arandu_semantics::types::ArType::Nullable(new_inner)
        }
        arandu_semantics::types::ArType::Slice(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute_projection_type(inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            arandu_semantics::types::ArType::Slice(new_inner)
        }
        arandu_semantics::types::ArType::Array(len, inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute_projection_type(inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            arandu_semantics::types::ArType::Array(*len, new_inner)
        }
        arandu_semantics::types::ArType::Ptr(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute_projection_type(inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            arandu_semantics::types::ArType::Ptr(new_inner)
        }
        arandu_semantics::types::ArType::Tuple(tys) => {
            let new_tys = tys
                .iter()
                .map(|&ty_id| {
                    let item_ty = interner.resolve(ty_id);
                    let substituted_item = substitute_projection_type(item_ty, subst, interner);
                    interner.lookup(&substituted_item).unwrap_or(ty_id)
                })
                .collect();
            arandu_semantics::types::ArType::Tuple(new_tys)
        }
        arandu_semantics::types::ArType::Result(ok, err) => {
            let ok_ty = interner.resolve(*ok);
            let substituted_ok = substitute_projection_type(ok_ty, subst, interner);
            let new_ok = interner.lookup(&substituted_ok).unwrap_or(*ok);

            let err_ty = interner.resolve(*err);
            let substituted_err = substitute_projection_type(err_ty, subst, interner);
            let new_err = interner.lookup(&substituted_err).unwrap_or(*err);

            arandu_semantics::types::ArType::Result(new_ok, new_err)
        }
        arandu_semantics::types::ArType::Option(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute_projection_type(inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            arandu_semantics::types::ArType::Option(new_inner)
        }
        arandu_semantics::types::ArType::Coroutine(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute_projection_type(inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            arandu_semantics::types::ArType::Coroutine(new_inner)
        }
        arandu_semantics::types::ArType::Range(inner) => {
            let inner_ty = interner.resolve(*inner);
            let substituted_inner = substitute_projection_type(inner_ty, subst, interner);
            let new_inner = interner.lookup(&substituted_inner).unwrap_or(*inner);
            arandu_semantics::types::ArType::Range(new_inner)
        }
        other => other.clone(),
    }
}
