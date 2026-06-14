use super::{DeferKind, LowerCtx, MoveState};
use crate::amir::program::extend_block_range;
use crate::amir::{
    AmirBasicBlock, AmirConstant, AmirLocal, AmirOperand, AmirPlace, AmirRvalue, AmirStmt,
    AmirTemp, AmirTerminator, BlockId, LocalId, TempId,
};
use crate::diagnostics::Diagnostic;
use crate::hir::HirBlock;
use crate::layout::DenseRange;
use crate::literal_pool::AmirLiteralEntry;
use crate::passes::type_checker::types::{ArType, Primitive};
use crate::{SymbolId, SymbolTable};
use arandu_lexer::Span;

impl LowerCtx<'_> {
    pub(crate) fn next_local_id(&self) -> LocalId {
        LocalId::from_usize(self.locals.len())
    }

    pub(crate) fn next_temp_id(&self) -> TempId {
        TempId::from_usize(self.temps.len())
    }

    pub(crate) fn intern_literal(&mut self, entry: AmirLiteralEntry) -> AmirConstant {
        AmirConstant::Pool(self.literal_pool.intern(entry))
    }

    pub(crate) fn new_temp(&mut self, ty: ArType) -> TempId {
        let id = self.next_temp_id();
        self.temps.push(AmirTemp {
            id,
            ty,
            span: Span::new(0, 0, 0),
        });
        self.temp_states.push(MoveState::Available);
        self.temp_origins.push(None);
        id
    }

    pub(crate) fn new_local(&mut self, ty: ArType, symbol: SymbolId, span: Span) -> LocalId {
        let id = self.next_local_id();
        self.locals.push(AmirLocal {
            id,
            ty,
            symbol: Some(symbol),
            span,
            use_span: None,
        });
        self.local_states.push(MoveState::Available);
        self.symbol_map.insert(symbol, id);
        id
    }

    pub(crate) fn new_compiler_local(&mut self, ty: ArType) -> LocalId {
        let id = self.next_local_id();
        self.locals.push(AmirLocal {
            id,
            ty,
            symbol: None,
            span: Span::new(0, 0, 0),
            use_span: None,
        });
        self.local_states.push(MoveState::Available);
        id
    }

    pub(crate) fn operand_type(&self, op: &AmirOperand) -> ArType {
        match op {
            AmirOperand::Copy(temp_id) | AmirOperand::Move(temp_id) => {
                self.temps[temp_id.as_usize()].ty.clone()
            }
            AmirOperand::Constant(c) => match c {
                AmirConstant::Bool(_) => ArType::Primitive(Primitive::Bool),
                AmirConstant::Nil => ArType::Error,
                AmirConstant::Pool(_) => ArType::Error,
            },
            _ => ArType::Error,
        }
    }

    pub(crate) fn new_block(&mut self) -> BlockId {
        let id = BlockId::from_usize(self.blocks.len());
        self.blocks.push(AmirBasicBlock {
            id,
            statements: DenseRange::empty(),
            terminator: AmirTerminator::Unreachable,
            successors: Vec::new(),
            predecessors: Vec::new(),
        });
        id
    }

    pub(crate) fn push_stmt(&mut self, stmt: AmirStmt) {
        if let Some(curr) = self.current_block {
            let id = self.stmts.push(stmt);
            extend_block_range(&mut self.blocks[curr.as_usize()].statements, id);
        }
    }

    pub(crate) fn set_terminator(&mut self, term: AmirTerminator) {
        if let Some(curr) = self.current_block {
            self.blocks[curr.as_usize()].terminator = term;
        }
    }

    pub(crate) fn set_bool_branch(
        &mut self,
        condition: AmirOperand,
        if_true: BlockId,
        if_false: BlockId,
    ) {
        self.set_terminator(AmirTerminator::SwitchInt {
            discriminant: condition,
            targets: vec![(1, if_true)],
            otherwise: if_false,
        });
    }

    pub(crate) fn lower_defer_block(
        &mut self,
        block: &HirBlock,
        symbols: &SymbolTable,
    ) -> Result<(), Diagnostic> {
        for stmt_id in &block.statements {
            let stmt = self.hir.pool.stmt(*stmt_id);
            self.lower_stmt(stmt, symbols)?;
        }
        Ok(())
    }

    pub(crate) fn exit_current_defer_frame(
        &mut self,
        include_errdefer: bool,
        symbols: &SymbolTable,
    ) -> Result<(), Diagnostic> {
        if let Some(frame) = self.defer_frames.pop() {
            if include_errdefer {
                for (block, kind) in frame.entries.iter().rev() {
                    if *kind == DeferKind::ErrDefer {
                        self.lower_defer_block(block, symbols)?;
                    }
                }
            }
            for (block, kind) in frame.entries.iter().rev() {
                if *kind == DeferKind::Defer {
                    self.lower_defer_block(block, symbols)?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn exit_defer_frames_from(
        &mut self,
        target_depth: usize,
        include_errdefer: bool,
        symbols: &SymbolTable,
    ) -> Result<(), Diagnostic> {
        while self.defer_frames.len() > target_depth {
            self.exit_current_defer_frame(include_errdefer, symbols)?;
        }
        Ok(())
    }

    pub(crate) fn exit_all_defer_frames(
        &mut self,
        include_errdefer: bool,
        symbols: &SymbolTable,
    ) -> Result<(), Diagnostic> {
        self.exit_defer_frames_from(0, include_errdefer, symbols)
    }

    pub(crate) fn register_defer(&mut self, block: &HirBlock, kind: DeferKind) {
        if let Some(frame) = self.defer_frames.last_mut() {
            frame.entries.push((block.clone(), kind));
        }
    }

    pub(crate) fn emit_assign_temp(&mut self, temp: TempId, rhs: AmirRvalue) {
        self.push_stmt(AmirStmt::Assign { lhs: temp, rhs });
    }

    pub(crate) fn emit_store_place(
        &mut self,
        lhs: AmirPlace,
        rhs: AmirOperand,
    ) -> Result<(), Diagnostic> {
        let rhs = self.consume_operand(rhs)?;
        if lhs.projections.is_empty() {
            self.local_states[lhs.local.as_usize()] = MoveState::Available;
        }
        self.push_stmt(AmirStmt::Store { lhs, rhs });
        Ok(())
    }

    pub(crate) fn load_place(
        &mut self,
        place: &AmirPlace,
        ty: ArType,
        _projection_types: &[ArType],
    ) -> Result<AmirOperand, Diagnostic> {
        let temp = self.new_temp(ty);
        self.emit_assign_temp(temp, AmirRvalue::Load(place.clone()));
        if place.projections.is_empty() {
            self.temp_origins[temp.as_usize()] = Some(place.local);
        }
        Ok(AmirOperand::Copy(temp))
    }

    pub(crate) fn consume_operand(&mut self, op: AmirOperand) -> Result<AmirOperand, Diagnostic> {
        let AmirOperand::Copy(temp) = op else {
            return Ok(op);
        };
        let idx = temp.as_usize();
        if self.temp_states[idx] == MoveState::Moved {
            return Err(self.move_diag(format!("use of moved temporary _{idx}")));
        }
        let ty = self.temps[idx].ty.clone();
        if ty.is_copy_v01() {
            return Ok(AmirOperand::Copy(temp));
        }
        self.temp_states[idx] = MoveState::Moved;
        Ok(AmirOperand::Move(temp))
    }

    pub(crate) fn move_diag(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic::error(
            crate::DiagCode::U001FeatureNotSupported,
            message.into(),
            Span::new(0, 0, 0),
        )
    }
}
