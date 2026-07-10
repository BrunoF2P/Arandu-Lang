//! AMIR → Cranelift IR function translator.
//!
//! [`FunctionTranslator`] walks an [`AmirFunc`] basic block by block,
//! emitting Cranelift IR instructions via a [`FunctionBuilder`]. The
//! [`AmirVisitor`] trait defines the visit callbacks used during traversal.

mod call;
mod compare;
mod expr;
mod memory;
mod operand;
mod place;
mod stmt;
mod terminator;

use arandu_base::span::Span;
use arandu_semantics::amir::{
    AmirBasicBlock, AmirConstant, AmirFunc, AmirOperand, AmirStmt, AmirTerminator, BlockId,
    InstrId, LocalId, TempId,
};
use arandu_semantics::passes::type_checker::types::{ArType, Primitive};
use arandu_semantics::{DiagCode, Diagnostic, SymbolTable};
use cranelift_codegen::ir::types::I64;
use cranelift_codegen::ir::{Block, InstBuilder, Type, Value};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_jit::JITModule;
use cranelift_module::FuncId;
use rustc_hash::FxHashMap;

use crate::types::{ClifType, clif_type, clif_types};

/// Visitor callbacks invoked while translating an [`AmirFunc`] to Cranelift IR.
pub trait AmirVisitor {
    /// Called once for each basic block before its statements are visited.
    fn visit_block(&mut self, block: &AmirBasicBlock);
    /// Called for each statement within the current block.
    fn visit_stmt(&mut self, stmt: &AmirStmt);
    /// Called for the block terminator after all statements have been visited.
    fn visit_terminator(&mut self, term: &AmirTerminator);
}

/// Translates a single [`AmirFunc`] into Cranelift IR using a [`FunctionBuilder`].
///
/// Holds all per-function compilation state: block/temp/local mappings,
/// string fat-pointer variables, and a deferred error slot so that
/// translation can continue after the first failure.
pub struct FunctionTranslator<'a, 'b> {
    pub builder: FunctionBuilder<'a>,
    pub module: &'b mut JITModule,
    pub symbol_table: &'b SymbolTable,
    pub func_ids: &'b FxHashMap<String, FuncId>,
    pub block_map: FxHashMap<BlockId, Block>,
    pub temp_map: FxHashMap<TempId, Variable>,
    pub local_map: FxHashMap<LocalId, Variable>,
    pub str_temp_map: FxHashMap<TempId, (Variable, Variable)>,
    pub str_local_map: FxHashMap<LocalId, (Variable, Variable)>,
    pub ptr_type: Type,
    pub literal_pool: &'b arandu_semantics::literal_pool::AmirLiteralPool,
    pub current_func: &'b AmirFunc,
    pub type_info: &'b arandu_semantics::TypeInfo,
    pub(crate) error: Option<Diagnostic>,
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
        type_info: &'b arandu_semantics::TypeInfo,
    ) -> Self {
        Self {
            builder,
            module,
            symbol_table,
            func_ids,
            block_map: FxHashMap::default(),
            temp_map: FxHashMap::default(),
            local_map: FxHashMap::default(),
            str_temp_map: FxHashMap::default(),
            str_local_map: FxHashMap::default(),
            ptr_type,
            literal_pool,
            current_func,
            type_info,
            error: None,
        }
    }

    pub(crate) fn get_temp_clif_type(&self, temp_id: TempId) -> Option<Type> {
        self.current_func
            .temps
            .get(temp_id.as_usize())
            .and_then(|t| {
                let ty = self.resolve_ty(t.ty);
                match clif_type(&ty, self.ptr_type) {
                    ClifType::Concrete(ty) => Some(ty),
                    ClifType::Void => None,
                }
            })
    }

    pub(crate) fn func_span(&self) -> Span {
        self.symbol_table.get(self.current_func.symbol).span
    }

    pub(crate) fn record_ice(&mut self, message: impl Into<String>, span: Span) {
        if self.error.is_none() {
            self.error = Some(Diagnostic::ice(DiagCode::ICEGEN001, message, span));
        }
    }

    pub(crate) fn record_error(&mut self, code: DiagCode, message: impl Into<String>, span: Span) {
        if self.error.is_none() {
            self.error = Some(Diagnostic::error(code, message, span));
        }
    }

    pub(crate) fn poison_i32(&mut self) -> Value {
        self.builder.ins().iconst(self.ptr_type, 0)
    }

    pub(crate) fn temp_span(&self, temp_id: TempId) -> Span {
        self.current_func
            .temps
            .get(temp_id.as_usize())
            .map(|temp| temp.span)
            .unwrap_or_else(|| self.func_span())
    }

    pub(crate) fn local_span(&self, local_id: LocalId) -> Span {
        self.current_func
            .locals
            .get(local_id.as_usize())
            .map(|local| local.span)
            .unwrap_or_else(|| self.func_span())
    }

    #[inline]
    pub(crate) fn resolve_ty(&self, id: arandu_semantics::types::TypeId) -> ArType {
        self.type_info.type_interner.resolve(id)
    }

    #[inline]
    pub(crate) fn temp_ar_ty(&self, temp_id: TempId) -> ArType {
        self.resolve_ty(self.current_func.temps[temp_id.as_usize()].ty)
    }

    #[inline]
    pub(crate) fn local_ar_ty(&self, local_id: LocalId) -> ArType {
        self.resolve_ty(self.current_func.locals[local_id.as_usize()].ty)
    }

    pub(crate) fn get_operand_ar_type(&self, op: &AmirOperand) -> ArType {
        match op {
            AmirOperand::Copy(temp_id) | AmirOperand::Move(temp_id) => self.temp_ar_ty(*temp_id),
            AmirOperand::Constant(c) => match c {
                AmirConstant::Bool(_) => ArType::Primitive(Primitive::Bool),
                AmirConstant::Nil => ArType::Void,
                AmirConstant::Pool(lit_id) => match self.literal_pool.get(*lit_id) {
                    arandu_semantics::literal_pool::AmirLiteralEntry::Int(_) => ArType::IntLiteral,
                    arandu_semantics::literal_pool::AmirLiteralEntry::Float(_) => {
                        ArType::FloatLiteral
                    }
                    arandu_semantics::literal_pool::AmirLiteralEntry::Str(_) => {
                        ArType::Primitive(Primitive::Str)
                    }
                    arandu_semantics::literal_pool::AmirLiteralEntry::Char(_) => {
                        ArType::Primitive(Primitive::Char)
                    }
                },
            },
            AmirOperand::FunctionRef(_) | AmirOperand::GlobalRef(_) => {
                if let AmirOperand::GlobalRef(symbol) = op {
                    if let Some(ty) = self.type_info.decl_type(*symbol) {
                        return ty.clone();
                    }
                }
                ArType::Error
            }
        }
    }

    #[tracing::instrument(level = "trace", target = "arandu_backend_cranelift", skip(self))]

    pub fn translate(&mut self) -> Result<(), Diagnostic> {
        for (idx, _block) in self.current_func.blocks.iter().enumerate() {
            let block_id = BlockId::from_usize(idx);
            let clif_block = self.builder.create_block();
            self.block_map.insert(block_id, clif_block);
        }

        for (idx, block) in self.current_func.blocks.iter().enumerate() {
            let block_id = BlockId::from_usize(idx);
            let clif_block = self.block_map[&block_id];
            if block_id.as_usize() > 0 {
                for param in &block.params {
                    let pty = self.resolve_ty(param.ty);
                    for &clif_ty in &clif_types(&pty, self.ptr_type) {
                        self.builder.append_block_param(clif_block, clif_ty);
                    }
                }
            }
        }

        let entry_clif = self.block_map[&BlockId::from_usize(0)];
        self.builder
            .append_block_params_for_function_params(entry_clif);

        for local in &self.current_func.locals {
            let lty = self.resolve_ty(local.ty);
            if matches!(lty, ArType::Primitive(Primitive::Str)) {
                let var_ptr = self.builder.declare_var(self.ptr_type);
                let var_len = self.builder.declare_var(I64);
                self.str_local_map.insert(local.id, (var_ptr, var_len));
            } else if let ClifType::Concrete(clif_ty) = clif_type(&lty, self.ptr_type) {
                let var = self.builder.declare_var(clif_ty);
                self.local_map.insert(local.id, var);
            }
        }

        for temp in &self.current_func.temps {
            let tty = self.resolve_ty(temp.ty);
            if matches!(tty, ArType::Primitive(Primitive::Str)) {
                let var_ptr = self.builder.declare_var(self.ptr_type);
                let var_len = self.builder.declare_var(I64);
                self.str_temp_map.insert(temp.id, (var_ptr, var_len));
            } else if let ClifType::Concrete(clif_ty) = clif_type(&tty, self.ptr_type) {
                let var = self.builder.declare_var(clif_ty);
                self.temp_map.insert(temp.id, var);
            }
        }

        let rpo = arandu_semantics::amir::reverse_post_order(self.current_func);
        for &block_id in &rpo {
            let block = self.current_func.block(block_id);
            self.visit_block(block);
        }

        self.builder.seal_all_blocks();

        if let Some(error) = self.error.take() {
            return Err(error);
        }
        Ok(())
    }
}

impl<'a, 'b> AmirVisitor for FunctionTranslator<'a, 'b> {
    fn visit_block(&mut self, block: &AmirBasicBlock) {
        if self.error.is_some() {
            return;
        }
        let clif_block = self.block_map[&block.id];
        self.builder.switch_to_block(clif_block);

        if block.id.as_usize() == 0 {
            for local in &self.current_func.locals {
                let lty = self.resolve_ty(local.ty);
                if matches!(lty, ArType::Primitive(Primitive::Str)) {
                    let &(var_ptr, var_len) = &self.str_local_map[&local.id];
                    let zero_ptr = self.builder.ins().iconst(self.ptr_type, 0);
                    let zero_len = self.builder.ins().iconst(I64, 0);
                    self.builder.def_var(var_ptr, zero_ptr);
                    self.builder.def_var(var_len, zero_len);
                } else if let Some(&var) = self.local_map.get(&local.id) {
                    let Some(clif_ty) = clif_type(&lty, self.ptr_type).concrete() else {
                        continue;
                    };
                    let zero = if clif_ty == cranelift_codegen::ir::types::F32 {
                        self.builder.ins().f32const(0.0)
                    } else if clif_ty == cranelift_codegen::ir::types::F64 {
                        self.builder.ins().f64const(0.0)
                    } else {
                        self.builder.ins().iconst(clif_ty, 0)
                    };
                    self.builder.def_var(var, zero);
                }
            }

            let clif_params = self.builder.block_params(clif_block).to_vec();
            let mut clif_slot_idx = 0;
            for &param_temp_id in &self.current_func.params {
                let param_ty = self.temp_ar_ty(param_temp_id);
                if matches!(&param_ty, ArType::Primitive(Primitive::Str)) {
                    let ptr_val = clif_params[clif_slot_idx];
                    let len_val = clif_params[clif_slot_idx + 1];
                    clif_slot_idx += 2;
                    if let Some(&(var_ptr, var_len)) = self.str_temp_map.get(&param_temp_id) {
                        self.builder.def_var(var_ptr, ptr_val);
                        self.builder.def_var(var_len, len_val);
                    }
                } else if let ClifType::Concrete(_) = clif_type(&param_ty, self.ptr_type) {
                    let val = clif_params[clif_slot_idx];
                    clif_slot_idx += 1;
                    if let Some(&var) = self.temp_map.get(&param_temp_id) {
                        self.builder.def_var(var, val);
                    }
                }
            }
        } else {
            let clif_params = self.builder.block_params(clif_block).to_vec();
            let mut clif_slot_idx = 0;
            for param in &block.params {
                let pty = self.resolve_ty(param.ty);
                if matches!(pty, ArType::Primitive(Primitive::Str)) {
                    let ptr_val = clif_params[clif_slot_idx];
                    let len_val = clif_params[clif_slot_idx + 1];
                    clif_slot_idx += 2;
                    if let Some(&(var_ptr, var_len)) = self.str_temp_map.get(&param.id) {
                        self.builder.def_var(var_ptr, ptr_val);
                        self.builder.def_var(var_len, len_val);
                    }
                } else if let ClifType::Concrete(_) = clif_type(&pty, self.ptr_type) {
                    let val = clif_params[clif_slot_idx];
                    clif_slot_idx += 1;
                    if let Some(&var) = self.temp_map.get(&param.id) {
                        self.builder.def_var(var, val);
                    }
                }
            }
        }

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
