mod compare;
mod expr;
mod stmt;

use arandu_base::span::Span;
use arandu_semantics::amir::{
    AmirBasicBlock, AmirFunc, AmirStmt, AmirTerminator, BlockId, InstrId, LocalId, TempId,
};
use arandu_semantics::{DiagCode, Diagnostic, SymbolTable};
use cranelift_codegen::ir::{Block, InstBuilder, Type, Value};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_jit::JITModule;
use cranelift_module::FuncId;
use rustc_hash::FxHashMap;

use crate::types::{ClifType, clif_type};

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

    pub(crate) fn get_temp_clif_type(&self, temp_id: TempId) -> Option<Type> {
        self.current_func
            .temps
            .iter()
            .find(|t| t.id == temp_id)
            .and_then(|t| match clif_type(&t.ty, self.ptr_type) {
                ClifType::Concrete(ty) => Some(ty),
                ClifType::Void => None,
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

    pub(crate) fn poison_i32(&mut self) -> Value {
        self.builder.ins().iconst(cranelift_codegen::ir::types::I32, 0)
    }

    pub(crate) fn temp_span(&self, temp_id: TempId) -> Span {
        self.current_func
            .temps
            .iter()
            .find(|temp| temp.id == temp_id)
            .map(|temp| temp.span)
            .unwrap_or_else(|| self.func_span())
    }

    pub(crate) fn local_span(&self, local_id: LocalId) -> Span {
        self.current_func
            .locals
            .iter()
            .find(|local| local.id == local_id)
            .map(|local| local.span)
            .unwrap_or_else(|| self.func_span())
    }

    pub fn translate(&mut self) -> Result<(), Diagnostic> {
        for (idx, _) in self.current_func.blocks.iter().enumerate() {
            let block_id = BlockId::from_usize(idx);
            let clif_block = self.builder.create_block();
            self.block_map.insert(block_id, clif_block);
        }

        let entry_clif = self.block_map[&BlockId::from_usize(0)];
        self.builder
            .append_block_params_for_function_params(entry_clif);
        self.builder.switch_to_block(entry_clif);

        for local in &self.current_func.locals {
            if let ClifType::Concrete(clif_ty) = clif_type(&local.ty, self.ptr_type) {
                let var = self.builder.declare_var(clif_ty);
                self.local_map.insert(local.id, var);
            }
        }

        for temp in &self.current_func.temps {
            if let ClifType::Concrete(clif_ty) = clif_type(&temp.ty, self.ptr_type) {
                let var = self.builder.declare_var(clif_ty);
                self.temp_map.insert(temp.id, var);
            }
        }

        let entry_params = self.builder.block_params(entry_clif).to_vec();
        for (i, &param_temp_id) in self.current_func.params.iter().enumerate() {
            if let Some(&var) = self.temp_map.get(&param_temp_id) {
                self.builder.def_var(var, entry_params[i]);
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
