use crate::types::{ClifType, ar_type_is_unsigned_integer, clif_type};
use arandu_base::span::Span;
use arandu_semantics::{DiagCode, Diagnostic, SymbolTable};
use arandu_semantics::amir::{
    AmirBasicBlock, AmirConstant, AmirFunc, AmirOperand, AmirPlace, AmirProjection, AmirRvalue, AmirStmt,
    AmirTerminator, BlockId, InstrId, LocalId, TempId,
};
use arandu_semantics::ops::{BinaryOp, UnaryOp};
use cranelift_codegen::ir::{Block, InstBuilder, TrapCode, Type, Value};
use cranelift_frontend::{FunctionBuilder, Switch, Variable};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};
use rustc_hash::FxHashMap;

pub trait AmirVisitor {
    fn visit_block(&mut self, block: &AmirBasicBlock);
    fn visit_stmt(&mut self, stmt: &AmirStmt);
    fn visit_terminator(&mut self, term: &AmirTerminator);
}

pub struct FunctionTranslator<'a, 'b> {
    pub builder: FunctionBuilder<'a>,
    pub module: &'b mut JITModule,
    pub symbol_table: &'b SymbolTable,
    pub func_ids: &'b FxHashMap<String, FuncId>,
    pub block_map: FxHashMap<BlockId, Block>,
    pub temp_map: FxHashMap<TempId, Variable>,
    pub local_map: FxHashMap<LocalId, Variable>,
    pub ptr_type: Type,
    pub literal_pool: &'b arandu_semantics::literal_pool::AmirLiteralPool,
    pub current_func: &'b AmirFunc,
    error: Option<Diagnostic>,
}

impl<'a, 'b> FunctionTranslator<'a, 'b> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        builder: FunctionBuilder<'a>,
        module: &'b mut JITModule,
        symbol_table: &'b SymbolTable,
        func_ids: &'b FxHashMap<String, FuncId>,
        ptr_type: Type,
        literal_pool: &'b arandu_semantics::literal_pool::AmirLiteralPool,
        current_func: &'b AmirFunc,
    ) -> Self {
        Self {
            builder,
            module,
            symbol_table,
            func_ids,
            block_map: FxHashMap::default(),
            temp_map: FxHashMap::default(),
            local_map: FxHashMap::default(),
            ptr_type,
            literal_pool,
            current_func,
            error: None,
        }
    }
}

impl<'a, 'b> FunctionTranslator<'a, 'b> {
    fn get_temp_clif_type(&self, temp_id: TempId) -> Option<Type> {
        self.current_func
            .temps
            .iter()
            .find(|t| t.id == temp_id)
            .and_then(|t| match clif_type(&t.ty, self.ptr_type) {
                ClifType::Concrete(ty) => Some(ty),
                ClifType::Void => None,
            })
    }

    fn func_span(&self) -> Span {
        self.symbol_table.get(self.current_func.symbol).span
    }

    fn record_ice(&mut self, message: impl Into<String>, span: Span) {
        if self.error.is_none() {
            self.error = Some(Diagnostic::ice(DiagCode::ICEGEN001, message, span));
        }
    }

    fn poison_i32(&mut self) -> Value {
        self.builder.ins().iconst(cranelift_codegen::ir::types::I32, 0)
    }

    fn temp_span(&self, temp_id: TempId) -> Span {
        self.current_func
            .temps
            .iter()
            .find(|temp| temp.id == temp_id)
            .map(|temp| temp.span)
            .unwrap_or_else(|| self.func_span())
    }

    fn local_span(&self, local_id: LocalId) -> Span {
        self.current_func
            .locals
            .iter()
            .find(|local| local.id == local_id)
            .map(|local| local.span)
            .unwrap_or_else(|| self.func_span())
    }

    pub fn translate(&mut self) -> Result<(), Diagnostic> {
        // Step 1: create all blocks
        for (idx, _) in self.current_func.blocks.iter().enumerate() {
            let block_id = BlockId::from_usize(idx);
            let clif_block = self.builder.create_block();
            self.block_map.insert(block_id, clif_block);
        }

        // Step 2: setup entry block
        let entry_clif = self.block_map[&BlockId::from_usize(0)];
        self.builder
            .append_block_params_for_function_params(entry_clif);
        self.builder.switch_to_block(entry_clif);

        // Step 3: declare all locals
        for local in &self.current_func.locals {
            if let ClifType::Concrete(clif_ty) = clif_type(&local.ty, self.ptr_type) {
                let var = self.builder.declare_var(clif_ty);
                self.local_map.insert(local.id, var);
            }
        }

        // Step 4: declare all temps
        for temp in &self.current_func.temps {
            if let ClifType::Concrete(clif_ty) = clif_type(&temp.ty, self.ptr_type) {
                let var = self.builder.declare_var(clif_ty);
                self.temp_map.insert(temp.id, var);
            }
        }

        // Step 5: define params
        let entry_params = self.builder.block_params(entry_clif).to_vec();
        for (i, &param_temp_id) in self.current_func.params.iter().enumerate() {
            if let Some(&var) = self.temp_map.get(&param_temp_id) {
                self.builder.def_var(var, entry_params[i]);
            }
        }

        // Step 6: visit blocks in reverse post-order
        let rpo = arandu_semantics::amir::reverse_post_order(self.current_func);
        for &block_id in &rpo {
            let block = self.current_func.block(block_id);
            self.visit_block(block);
        }

        // Step 7: seal all blocks
        self.builder.seal_all_blocks();

        if let Some(error) = self.error.take() {
            return Err(error);
        }
        Ok(())
    }

    fn translate_stmt(&mut self, stmt: &AmirStmt) {
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
                        let clif_ty = lhs.and_then(|temp| self.get_temp_clif_type(temp))
                            .unwrap_or(self.ptr_type);
                        let loaded_val = self.builder.ins().load(clif_ty, cranelift_codegen::ir::MemFlags::new(), ptr_val, 0);
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
                        self.builder.ins().store(cranelift_codegen::ir::MemFlags::new(), val_to_store, ptr_val, 0);
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
            // Free é no-op no JIT por enquanto.
            AmirStmt::Free(_) => {}
            // StorageLive e StorageDead são hints para tempos de vida das variáveis.
            // O Cranelift faz a sua própria análise de liveness/regalloc na stack,
            // então ignoramos esses hints por ora.
            AmirStmt::StorageLive(_) | AmirStmt::StorageDead(_) => {}
            // Destroy será usado no backend C para chamar destrutores de recursos.
            // No JIT atual, não temos destrutores, então é no-op.
            AmirStmt::Destroy(_) => {}
            AmirStmt::Nop => {}
        }
    }

    fn translate_store_place(&mut self, lhs: &AmirPlace, val: Value) {
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
                        ptr_val = self.builder.ins().load(self.ptr_type, cranelift_codegen::ir::MemFlags::new(), ptr_val, offset);
                    }
                    AmirProjection::Index(op) => {
                        let idx_val = self.translate_operand(op, Some(self.ptr_type));
                        let elem_size = self.builder.ins().iconst(self.ptr_type, 8);
                        let offset_val = self.builder.ins().imul(idx_val, elem_size);
                        let elem_ptr = self.builder.ins().iadd(ptr_val, offset_val);
                        ptr_val = self.builder.ins().load(self.ptr_type, cranelift_codegen::ir::MemFlags::new(), elem_ptr, 0);
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
                    self.builder.ins().store(cranelift_codegen::ir::MemFlags::new(), val, ptr_val, offset);
                }
                AmirProjection::Index(op) => {
                    let idx_val = self.translate_operand(op, Some(self.ptr_type));
                    let elem_size = self.builder.ins().iconst(self.ptr_type, 8);
                    let offset_val = self.builder.ins().imul(idx_val, elem_size);
                    let target_ptr = self.builder.ins().iadd(ptr_val, offset_val);
                    self.builder.ins().store(cranelift_codegen::ir::MemFlags::new(), val, target_ptr, 0);
                }
            }
        }
    }

    fn malloc_func_id(&mut self) -> Option<FuncId> {
        match self.func_ids.get("malloc") {
            Some(func_id) => Some(*func_id),
            None => {
                self.record_ice("malloc was not declared in the JIT module", self.func_span());
                None
            }
        }
    }

    fn translate_operand(&mut self, operand: &AmirOperand, expected_ty: Option<Type>) -> Value {
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
                                        format!("invalid integer literal in AMIR literal pool: '{s}'"),
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
                                        format!("invalid float literal in AMIR literal pool: '{s}'"),
                                        self.func_span(),
                                    );
                                    return self.poison_i32();
                                }
                            };
                            self.builder.ins().f64const(val)
                        }
                        arandu_semantics::literal_pool::AmirLiteralEntry::Str(s) => {
                            let str_bytes = s.as_bytes();
                            let data_id = match self
                                .module
                                .declare_data(
                                    &format!("str_lit_{}", lit_id.0),
                                    cranelift_module::Linkage::Local,
                                    false,
                                    false,
                                )
                            {
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
                            let local_data_ref = self.module.declare_data_in_func(data_id, self.builder.func);
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

    fn translate_rvalue(&mut self, rvalue: &AmirRvalue, expected_ty: Option<Type>) -> Value {
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
                                            format!("unsupported struct field '{}' in codegen", name),
                                            self.symbol_table.get(*symbol_id).span,
                                        );
                                        return self.poison_i32();
                                    }
                                };
                                let clif_ty = expected_ty.unwrap_or(self.ptr_type);
                                ptr_val = self.builder.ins().load(clif_ty, cranelift_codegen::ir::MemFlags::new(), ptr_val, offset);
                            }
                            AmirProjection::Index(op) => {
                                let idx_val = self.translate_operand(op, Some(self.ptr_type));
                                let elem_size = self.builder.ins().iconst(self.ptr_type, 8);
                                let offset_val = self.builder.ins().imul(idx_val, elem_size);
                                let elem_ptr = self.builder.ins().iadd(ptr_val, offset_val);
                                let clif_ty = expected_ty.unwrap_or(self.ptr_type);
                                ptr_val = self.builder.ins().load(clif_ty, cranelift_codegen::ir::MemFlags::new(), elem_ptr, 0);
                            }
                        }
                    }
                    ptr_val
                }
            }
            AmirRvalue::StructLiteral { struct_symbol: _, fields } => {
                let Some(malloc_func_id) = self.malloc_func_id() else {
                    return self.poison_i32();
                };
                let local_ref = self.module.declare_func_in_func(malloc_func_id, self.builder.func);
                let size_val = self.builder.ins().iconst(self.ptr_type, (fields.len() * 8) as i64);
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
                    self.builder.ins().store(cranelift_codegen::ir::MemFlags::new(), val, ptr_val, offset);
                }
                ptr_val
            }
            AmirRvalue::FieldAccess { base, field } => {
                let ptr_val = self.translate_operand(base, Some(self.ptr_type));
                let offset = (field * 8) as i32;
                let clif_ty = expected_ty.unwrap_or(self.ptr_type);
                self.builder.ins().load(clif_ty, cranelift_codegen::ir::MemFlags::new(), ptr_val, offset)
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

    fn operand_is_unsigned_integer(&self, operand: &AmirOperand) -> Option<bool> {
        let temp_id = match operand {
            AmirOperand::Copy(temp_id) | AmirOperand::Move(temp_id) => *temp_id,
            _ => return None,
        };
        self.current_func
            .temps
            .iter()
            .find(|temp| temp.id == temp_id)
            .map(|temp| ar_type_is_unsigned_integer(&temp.ty))
    }

    fn operands_are_unsigned(
        &self,
        left: Option<&AmirOperand>,
        right: Option<&AmirOperand>,
    ) -> bool {
        left.and_then(|op| self.operand_is_unsigned_integer(op))
            .or_else(|| right.and_then(|op| self.operand_is_unsigned_integer(op)))
            .unwrap_or(false)
    }

    fn translate_binary_op(
        &mut self,
        op: BinaryOp,
        lhs: Value,
        rhs: Value,
        left_operand: Option<&AmirOperand>,
        right_operand: Option<&AmirOperand>,
    ) -> Value {
        let ty = self.builder.func.dfg.value_type(lhs);
        let is_float = ty.is_float();
        let is_unsigned = self.operands_are_unsigned(left_operand, right_operand);

        match op {
            BinaryOp::Add => {
                if is_float {
                    self.builder.ins().fadd(lhs, rhs)
                } else {
                    self.builder.ins().iadd(lhs, rhs)
                }
            }
            BinaryOp::Sub => {
                if is_float {
                    self.builder.ins().fsub(lhs, rhs)
                } else {
                    self.builder.ins().isub(lhs, rhs)
                }
            }
            BinaryOp::Mul => {
                if is_float {
                    self.builder.ins().fmul(lhs, rhs)
                } else {
                    self.builder.ins().imul(lhs, rhs)
                }
            }
            BinaryOp::Div => {
                if is_float {
                    self.builder.ins().fdiv(lhs, rhs)
                } else if is_unsigned {
                    self.builder.ins().udiv(lhs, rhs)
                } else {
                    self.builder.ins().sdiv(lhs, rhs)
                }
            }
            BinaryOp::Mod => {
                if is_float {
                    unimplemented!("Float remainder is not implemented")
                } else if is_unsigned {
                    self.builder.ins().urem(lhs, rhs)
                } else {
                    self.builder.ins().srem(lhs, rhs)
                }
            }
            BinaryOp::BitOr => self.builder.ins().bor(lhs, rhs),
            BinaryOp::BitAnd => self.builder.ins().band(lhs, rhs),
            BinaryOp::BitXor => self.builder.ins().bxor(lhs, rhs),
            BinaryOp::ShiftLeft => self.builder.ins().ishl(lhs, rhs),
            BinaryOp::ShiftRight => {
                if is_unsigned {
                    self.builder.ins().ushr(lhs, rhs)
                } else {
                    self.builder.ins().sshr(lhs, rhs)
                }
            }
            BinaryOp::Equal => {
                if is_float {
                    self.builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::Equal,
                        lhs,
                        rhs,
                    )
                } else {
                    self.builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        lhs,
                        rhs,
                    )
                }
            }
            BinaryOp::NotEqual => {
                if is_float {
                    self.builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::NotEqual,
                        lhs,
                        rhs,
                    )
                } else {
                    self.builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                        lhs,
                        rhs,
                    )
                }
            }
            BinaryOp::Lt => {
                if is_float {
                    self.builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::LessThan,
                        lhs,
                        rhs,
                    )
                } else if is_unsigned {
                    self.builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::UnsignedLessThan,
                        lhs,
                        rhs,
                    )
                } else {
                    self.builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
                        lhs,
                        rhs,
                    )
                }
            }
            BinaryOp::Gt => {
                if is_float {
                    self.builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::GreaterThan,
                        lhs,
                        rhs,
                    )
                } else if is_unsigned {
                    self.builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::UnsignedGreaterThan,
                        lhs,
                        rhs,
                    )
                } else {
                    self.builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThan,
                        lhs,
                        rhs,
                    )
                }
            }
            BinaryOp::LtEqual => {
                if is_float {
                    self.builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::LessThanOrEqual,
                        lhs,
                        rhs,
                    )
                } else if is_unsigned {
                    self.builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::UnsignedLessThanOrEqual,
                        lhs,
                        rhs,
                    )
                } else {
                    self.builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::SignedLessThanOrEqual,
                        lhs,
                        rhs,
                    )
                }
            }
            BinaryOp::GtEqual => {
                if is_float {
                    self.builder.ins().fcmp(
                        cranelift_codegen::ir::condcodes::FloatCC::GreaterThanOrEqual,
                        lhs,
                        rhs,
                    )
                } else if is_unsigned {
                    self.builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,
                        lhs,
                        rhs,
                    )
                } else {
                    self.builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThanOrEqual,
                        lhs,
                        rhs,
                    )
                }
            }
            // Or/And são lógicos no Arandu, mas como o tipo `bool` é representado
            // como `I8` contendo estritamente 0 ou 1, os operadores bitwise `bor`/`band`
            // produzem o resultado correto (0 ou 1) de forma mais eficiente.
            BinaryOp::Or => self.builder.ins().bor(lhs, rhs),
            BinaryOp::And => self.builder.ins().band(lhs, rhs),
            BinaryOp::RangeExclusive | BinaryOp::RangeInclusive => {
                let Some(malloc_func_id) = self.malloc_func_id() else {
                    return self.poison_i32();
                };
                let local_ref = self.module.declare_func_in_func(malloc_func_id, self.builder.func);
                let size_val = self.builder.ins().iconst(self.ptr_type, 16);
                let call_inst = self.builder.ins().call(local_ref, &[size_val]);
                let ptr_val = self.builder.inst_results(call_inst)[0];

                self.builder.ins().store(cranelift_codegen::ir::MemFlags::new(), lhs, ptr_val, 0);
                self.builder.ins().store(cranelift_codegen::ir::MemFlags::new(), rhs, ptr_val, 8);
                ptr_val
            }
            _ => unimplemented!(
                "Binary operator {:?} not implemented in Cranelift JIT yet",
                op
            ),
        }
    }

    fn translate_unary_op(&mut self, op: UnaryOp, val: Value) -> Value {
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

    fn translate_terminator(&mut self, terminator: &AmirTerminator) {
        match terminator {
            AmirTerminator::Return => {
                let clif_ret = clif_type(&self.current_func.return_type, self.ptr_type);
                match clif_ret {
                    ClifType::Concrete(_) => {
                        // TempId(0) é o "return register" por convenção do AMIR lowering.
                        // Ver: crates/arandu_mir/src/lower_amir/func.rs linha 39:
                        // "Return register is TempId(0)"
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

impl<'a, 'b> AmirVisitor for FunctionTranslator<'a, 'b> {
    fn visit_block(&mut self, block: &AmirBasicBlock) {
        if self.error.is_some() {
            return;
        }
        let clif_block = self.block_map[&block.id];
        self.builder.switch_to_block(clif_block);

        for stmt_id in block.statements.iter_ids::<InstrId>() {
            let stmt = self.current_func.stmt(stmt_id);
            self.visit_stmt(stmt);
        }

        self.visit_terminator(&block.terminator);
    }

    fn visit_stmt(&mut self, stmt: &AmirStmt) {
        self.translate_stmt(stmt);
    }

    fn visit_terminator(&mut self, term: &AmirTerminator) {
        self.translate_terminator(term);
    }
}
