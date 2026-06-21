use crate::types::{ClifType, clif_type};
use arandu_semantics::SymbolTable;
use arandu_semantics::amir::{
    AmirBasicBlock, AmirConstant, AmirFunc, AmirOperand, AmirPlace, AmirRvalue, AmirStmt,
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

    pub fn translate(&mut self) {
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
    }

    fn translate_stmt(&mut self, stmt: &AmirStmt) {
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
                let clif_args: Vec<Value> = args
                    .iter()
                    .map(|arg| self.translate_operand(arg, None))
                    .collect();
                let call_inst = match callee {
                    AmirOperand::FunctionRef(sym_id) => {
                        let sym = self.symbol_table.get(*sym_id);
                        let func_id = self.func_ids.get(&sym.name).expect("Function not declared");
                        let local_ref = self
                            .module
                            .declare_func_in_func(*func_id, self.builder.func);
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
        if lhs.projections.is_empty() {
            if let Some(&var) = self.local_map.get(&lhs.local) {
                self.builder.def_var(var, val);
            }
        } else {
            unimplemented!("Projections are not implemented in Cranelift JIT yet");
        }
    }

    fn translate_operand(&mut self, operand: &AmirOperand, expected_ty: Option<Type>) -> Value {
        match operand {
            AmirOperand::Copy(temp_id) | AmirOperand::Move(temp_id) => {
                let var = self
                    .temp_map
                    .get(temp_id)
                    .expect("Use of undeclared temp variable");
                self.builder.use_var(*var)
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
                            let val: i64 = s.parse().unwrap_or_else(|_| {
                                panic!("ICE: literal inteiro inválido no literal pool: '{s}'")
                            });
                            let ty = expected_ty.unwrap_or(cranelift_codegen::ir::types::I32);
                            self.builder.ins().iconst(ty, val)
                        }
                        arandu_semantics::literal_pool::AmirLiteralEntry::Float(s) => {
                            let val: f64 = s.parse().unwrap();
                            self.builder.ins().f64const(val)
                        }
                        arandu_semantics::literal_pool::AmirLiteralEntry::Str(_) => {
                            unimplemented!("String literals in JIT are not implemented yet");
                        }
                        arandu_semantics::literal_pool::AmirLiteralEntry::Char(s) => {
                            let val = s.chars().next().unwrap() as i64;
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
        }
    }

    fn translate_rvalue(&mut self, rvalue: &AmirRvalue, expected_ty: Option<Type>) -> Value {
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
                self.translate_binary_op(*op, lhs, rhs)
            }
            AmirRvalue::Unary { op, operand } => {
                let val = self.translate_operand(operand, expected_ty);
                self.translate_unary_op(*op, val)
            }
            AmirRvalue::Load(place) => {
                if place.projections.is_empty() {
                    let var = self
                        .local_map
                        .get(&place.local)
                        .expect("Use of undeclared local variable");
                    self.builder.use_var(*var)
                } else {
                    unimplemented!("Projections are not implemented in Cranelift JIT yet");
                }
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

    fn translate_binary_op(&mut self, op: BinaryOp, lhs: Value, rhs: Value) -> Value {
        let ty = self.builder.func.dfg.value_type(lhs);
        let is_float = ty.is_float();

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
                } else {
                    self.builder.ins().sdiv(lhs, rhs)
                }
            }
            BinaryOp::Mod => {
                if is_float {
                    unimplemented!("Float remainder is not implemented")
                } else {
                    self.builder.ins().srem(lhs, rhs)
                }
            }
            BinaryOp::BitOr => self.builder.ins().bor(lhs, rhs),
            BinaryOp::BitAnd => self.builder.ins().band(lhs, rhs),
            BinaryOp::BitXor => self.builder.ins().bxor(lhs, rhs),
            BinaryOp::ShiftLeft => self.builder.ins().ishl(lhs, rhs),
            BinaryOp::ShiftRight => self.builder.ins().sshr(lhs, rhs),
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
                } else {
                    // TODO: tipos unsigned devem usar UnsignedLessThan
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
                } else {
                    // TODO: tipos unsigned devem usar UnsignedGreaterThan
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
                } else {
                    // TODO: tipos unsigned devem usar UnsignedLessThanOrEqual
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
                } else {
                    // TODO: tipos unsigned devem usar UnsignedGreaterThanOrEqual
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
