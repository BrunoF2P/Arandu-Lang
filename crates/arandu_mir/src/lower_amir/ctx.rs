use super::{DeferKind, LowerCtx, MoveState};
use crate::amir::program::extend_block_range;
use crate::amir::{
    AmirBasicBlock, AmirConstant, AmirLocal, AmirOperand, AmirPlace, AmirProjection, AmirRvalue,
    AmirStmt, AmirTemp, AmirTerminator, BlockId, BlockParam, LocalId, TempId,
};
use crate::diagnostics::Diagnostic;
use crate::hir::HirBlock;
use crate::layout::DenseRange;
use crate::literal_pool::AmirLiteralEntry;
use crate::passes::type_checker::types::{ArType, Primitive};
use crate::{SymbolId, SymbolTable};
use arandu_lexer::Span;
use rustc_hash::FxHashMap;

impl LowerCtx<'_> {
    pub(crate) fn next_local_id(&self) -> LocalId {
        LocalId::from_usize(self.locals.len())
    }

    pub(crate) fn next_temp_id(&self) -> TempId {
        TempId::from_usize(self.temps.len())
    }

    #[allow(dead_code)] // general entry point; call sites prefer typed intern_literal_* helpers
    pub(crate) fn intern_literal(&mut self, entry: AmirLiteralEntry) -> AmirConstant {
        AmirConstant::Pool(self.literal_pool.intern(entry))
    }

    #[inline]
    pub(crate) fn intern_literal_int(&mut self, s: impl Into<smol_str::SmolStr>) -> AmirConstant {
        AmirConstant::Pool(self.literal_pool.intern_int(s))
    }

    #[inline]
    pub(crate) fn intern_literal_float(&mut self, s: impl Into<smol_str::SmolStr>) -> AmirConstant {
        AmirConstant::Pool(self.literal_pool.intern_float(s))
    }

    #[inline]
    pub(crate) fn intern_literal_str(&mut self, s: impl Into<smol_str::SmolStr>) -> AmirConstant {
        AmirConstant::Pool(self.literal_pool.intern_str(s))
    }

    #[inline]
    pub(crate) fn intern_literal_char(&mut self, s: impl Into<smol_str::SmolStr>) -> AmirConstant {
        AmirConstant::Pool(self.literal_pool.intern_char(s))
    }

    pub(crate) fn intern_ty(&self, ty: ArType) -> crate::types::TypeId {
        self.intern_ty_ref(&ty)
    }

    pub(crate) fn intern_ty_ref(&self, ty: &crate::types::ArType) -> crate::types::TypeId {
        self.tc.type_info.type_interner.intern_ref(ty)
    }

    /// Resolve a HIR/AMIR `TypeId` to an owned `ArType` (clones from the interner).
    #[inline]
    pub(crate) fn resolve_ty(&self, id: crate::types::TypeId) -> ArType {
        self.tc.type_info.type_interner.resolve(id)
    }

    /// Borrow the interned type without cloning.
    #[inline]
    pub(crate) fn with_ty<R>(&self, id: crate::types::TypeId, f: impl FnOnce(&ArType) -> R) -> R {
        self.tc.type_info.type_interner.with_type(id, f)
    }

    /// Allocate a temp from an already-interned type id (no `ArType` clone).
    pub(crate) fn new_temp_id(&mut self, ty: crate::types::TypeId) -> TempId {
        let is_copy = self.tc.type_info.type_interner.is_copy_v01(ty);
        let is_nullable = self
            .tc
            .type_info
            .type_interner
            .with_type(ty, |t| matches!(t, ArType::Nullable(_)));
        let id = self.next_temp_id();
        let span = if Self::span_is_usable(self.current_span) {
            self.current_span
        } else {
            Span::new(0, 0, 0)
        };
        self.temps.push(AmirTemp {
            id,
            ty,
            is_copy,
            is_nullable,
            span,
        });
        self.temp_states.push(MoveState::Available);
        self.temp_origins.push(None);
        id
    }

    /// Allocate a local from an already-interned type id.
    pub(crate) fn new_local_id(
        &mut self,
        ty: crate::types::TypeId,
        symbol: SymbolId,
        span: Span,
    ) -> LocalId {
        let is_memory = self
            .tc
            .type_info
            .type_interner
            .with_type(ty, super::is_memory_type);
        let id = self.next_local_id();
        self.locals.push(AmirLocal {
            id,
            ty,
            is_memory,
            symbol: Some(symbol),
            span,
            use_span: None,
        });
        self.local_states.push(MoveState::Available);
        self.symbol_map.insert(symbol, id);
        id
    }

    /// Non-empty source span (start != end). Empty spans are treated as unknown.
    #[inline]
    pub(crate) fn span_is_usable(span: Span) -> bool {
        span.start != span.end
    }

    /// Best available span for diagnostics: prefer non-empty `preferred`, else `current_span`.
    #[inline]
    pub(crate) fn diag_span(&self, preferred: Span) -> Span {
        if Self::span_is_usable(preferred) {
            preferred
        } else if Self::span_is_usable(self.current_span) {
            self.current_span
        } else {
            Span::new(0, 0, 0)
        }
    }

    /// Run `f` with `current_span` set to `span` when usable (restores previous after).
    pub(crate) fn with_span<R>(&mut self, span: Span, f: impl FnOnce(&mut Self) -> R) -> R {
        let prev = self.current_span;
        if Self::span_is_usable(span) {
            self.current_span = span;
        }
        let out = f(self);
        self.current_span = prev;
        out
    }

    pub(crate) fn new_temp(&mut self, ty: ArType) -> TempId {
        self.new_temp_ref(&ty)
    }

    /// Intern from borrow — avoids cloning `ArType` when the caller only has a ref
    /// (e.g. `expr.ty`).
    pub(crate) fn new_temp_ref(&mut self, ty: &ArType) -> TempId {
        let is_copy = ty.is_copy_v01();
        let is_nullable = matches!(ty, ArType::Nullable(_));
        let ty = self.intern_ty_ref(ty);
        let id = self.next_temp_id();
        let span = if Self::span_is_usable(self.current_span) {
            self.current_span
        } else {
            Span::new(0, 0, 0)
        };
        self.temps.push(AmirTemp {
            id,
            ty,
            is_copy,
            is_nullable,
            span,
        });
        self.temp_states.push(MoveState::Available);
        self.temp_origins.push(None);
        id
    }

    pub(crate) fn new_local(&mut self, ty: ArType, symbol: SymbolId, span: Span) -> LocalId {
        self.new_local_ref(&ty, symbol, span)
    }

    pub(crate) fn new_local_ref(
        &mut self,
        ty: &ArType,
        symbol: SymbolId,
        span: Span,
    ) -> LocalId {
        let is_memory = super::is_memory_type(ty);
        let ty = self.intern_ty_ref(ty);
        let id = self.next_local_id();
        self.locals.push(AmirLocal {
            id,
            ty,
            is_memory,
            symbol: Some(symbol),
            span,
            use_span: None,
        });
        self.local_states.push(MoveState::Available);
        self.symbol_map.insert(symbol, id);
        id
    }

    pub(crate) fn new_compiler_local(&mut self, ty: ArType) -> LocalId {
        let is_memory = super::is_memory_type(&ty);
        let ty = self.intern_ty(ty);
        let id = self.next_local_id();
        self.locals.push(AmirLocal {
            id,
            ty,
            is_memory,
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
                self.resolve_ty(self.temps[temp_id.as_usize()].ty)
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
            params: Vec::new(),
            statements: DenseRange::empty(),
            terminator: AmirTerminator::Unreachable,
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
            match &term {
                AmirTerminator::Goto { target, .. } => {
                    self.add_predecessor(curr, *target);
                }
                AmirTerminator::Branch {
                    if_true, if_false, ..
                } => {
                    self.add_predecessor(curr, *if_true);
                    self.add_predecessor(curr, *if_false);
                }
                AmirTerminator::SwitchInt {
                    targets, otherwise, ..
                } => {
                    for (_, dest, _) in targets {
                        self.add_predecessor(curr, *dest);
                    }
                    self.add_predecessor(curr, otherwise.0);
                }
                _ => {}
            }
            self.blocks[curr.as_usize()].terminator = term;
        }
    }

    pub(crate) fn set_bool_branch(
        &mut self,
        condition: AmirOperand,
        if_true: BlockId,
        if_false: BlockId,
    ) {
        let true_args = self.build_target_args(if_true);
        let false_args = self.build_target_args(if_false);
        self.set_terminator(AmirTerminator::Branch {
            condition,
            if_true,
            true_args,
            if_false,
            false_args,
        });
    }

    pub(crate) fn emit_goto(&mut self, target: BlockId) {
        let args = self.build_target_args(target);
        self.set_terminator(AmirTerminator::Goto { target, args });
    }

    pub(crate) fn emit_switch_int(
        &mut self,
        discriminant: AmirOperand,
        targets: Vec<(i128, BlockId)>,
        otherwise: BlockId,
    ) {
        let mut target_args_list = Vec::new();
        for (val, target) in targets {
            let args = self.build_target_args(target);
            target_args_list.push((val, target, args));
        }
        let otherwise_args = self.build_target_args(otherwise);
        self.set_terminator(AmirTerminator::SwitchInt {
            discriminant,
            targets: target_args_list,
            otherwise: (otherwise, otherwise_args),
        });
    }

    pub(crate) fn lower_defer_block(
        &mut self,
        block: &HirBlock,
        symbols: &SymbolTable,
    ) -> Result<(), Diagnostic> {
        for &stmt_id in self.hir.pool.stmt_list(block.statements) {
            if self.current_block.is_none() {
                break;
            }
            let stmt = self.hir.pool.stmt(stmt_id);
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
        // Projection store reads the base local (field/index write).
        if !lhs.projections.is_empty() {
            self.note_local_use(lhs.local, self.current_span);
        }
        if lhs.projections.is_empty() {
            self.local_states[lhs.local.as_usize()] = MoveState::Available;
            if let Some(block) = self.current_block {
                self.write_variable(block, lhs.local, rhs.clone());
            }
        }
        self.push_stmt(AmirStmt::Store { lhs, rhs });
        Ok(())
    }

    /// Record the latest source use of a local (S-USE-SPAN). Analyses prefer
    /// `use_span` over declaration span so O008/move point at the use site.
    pub(crate) fn note_local_use(&mut self, local: LocalId, span: Span) {
        // Skip empty spans so synthetic lowers don't wipe a real use site.
        if !Self::span_is_usable(span) {
            return;
        }
        if let Some(loc) = self.locals.get_mut(local.as_usize()) {
            loc.use_span = Some(span);
        }
    }

    /// Note use of the stack origin of a temp, if any (move / free / call args).
    pub(crate) fn note_temp_origin_use(&mut self, temp: TempId) {
        if let Some(Some(local)) = self.temp_origins.get(temp.as_usize()) {
            self.note_local_use(*local, self.current_span);
        }
    }

    pub(crate) fn load_place(
        &mut self,
        place: &AmirPlace,
        ty: crate::types::TypeId,
    ) -> Result<AmirOperand, Diagnostic> {
        self.note_local_use(place.local, self.current_span);
        let temp = self.new_temp_id(ty);
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
        // Consuming a non-copy temp is a use of its origin local at this site.
        self.note_temp_origin_use(temp);
        if self.temps[idx].is_copy {
            return Ok(AmirOperand::Copy(temp));
        }
        self.temp_states[idx] = MoveState::Moved;
        Ok(AmirOperand::Move(temp))
    }

    pub(crate) fn move_diag(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic::error(
            crate::DiagCode::U001FeatureNotSupported,
            message.into(),
            self.diag_span(self.current_span),
        )
    }

    // --- SSA / OSSA Block Arguments & Braun et al. Helpers ---

    pub(crate) fn add_predecessor(&mut self, from: BlockId, to: BlockId) {
        self.predecessors.entry(to).or_default().push(from);
    }

    pub(crate) fn seal_block(&mut self, block: BlockId) {
        if self.sealed_blocks.contains(&block) {
            return;
        }
        self.sealed_blocks.insert(block);

        // Resolve incomplete phis
        if let Some(incomplete) = self.incomplete_phis.remove(&block) {
            for (local, temp_id) in incomplete {
                self.add_block_parameter_operands(block, local, temp_id);
                self.simplify_phi(block, local, temp_id);
            }
        }
    }

    /// If `local` is `T?` and `value` is a non-Nil constant, assign through a
    /// temp so codegen can box the scalar (prevents bare `0` becoming `nil`).
    fn materialize_nullable_const(
        &mut self,
        local: LocalId,
        value: AmirOperand,
    ) -> AmirOperand {
        let ty = self.resolve_ty(self.locals[local.as_usize()].ty);
        if !matches!(ty, ArType::Nullable(_)) {
            return value;
        }
        match &value {
            AmirOperand::Constant(AmirConstant::Nil) => value,
            AmirOperand::Constant(_) => {
                let t = self.new_temp(ty);
                self.emit_assign_temp(t, AmirRvalue::Use(value));
                AmirOperand::Copy(t)
            }
            _ => value,
        }
    }

    pub(crate) fn write_variable(&mut self, block: BlockId, local: LocalId, value: AmirOperand) {
        let value = self.materialize_nullable_const(local, value);
        self.current_def.insert((block, local), value);
    }

    /// Current basic block, or ICE diagnostic if lowering lost block context.
    pub(crate) fn require_block(&self) -> Result<BlockId, Diagnostic> {
        self.current_block.ok_or_else(|| {
            Diagnostic::ice(
                crate::DiagCode::ICEGEN001,
                "AMIR lower: no current basic block",
                self.diag_span(self.current_span),
            )
        })
    }

    pub(crate) fn write_variable_source(
        &mut self,
        local: LocalId,
        value: AmirOperand,
    ) -> Result<(), Diagnostic> {
        let block = self.require_block()?;
        let value = self.materialize_nullable_const(local, value);
        self.write_variable(block, local, value);
        // Emit a dummy Store statement
        self.push_stmt(AmirStmt::Store {
            lhs: AmirPlace {
                local,
                projections: smallvec::smallvec![],
            },
            rhs: value,
        });
        Ok(())
    }

    pub(crate) fn read_variable(&mut self, block: BlockId, local: LocalId) -> AmirOperand {
        if let Some(val) = self.current_def.get(&(block, local)) {
            val.clone()
        } else {
            self.read_variable_recursive(block, local)
        }
    }

    pub(crate) fn read_variable_source(
        &mut self,
        local: LocalId,
    ) -> Result<AmirOperand, Diagnostic> {
        let block = self.require_block()?;
        let val = self.read_variable(block, local);

        self.note_local_use(local, self.current_span);

        // Emit a dummy Load statement
        let ty_id = self.locals[local.as_usize()].ty;
        let ty = self.resolve_ty(ty_id);
        let is_copy = ty.is_copy_v01();
        let is_nullable = matches!(ty, ArType::Nullable(_));
        let is_mem = super::is_memory_type(&ty);
        let temp = self.next_temp_id();
        let span = if Self::span_is_usable(self.current_span) {
            self.current_span
        } else {
            Span::new(0, 0, 0)
        };
        self.temps.push(AmirTemp {
            id: temp,
            ty: ty_id,
            is_copy,
            is_nullable,
            span,
        });
        self.temp_states.push(MoveState::Available);
        self.temp_origins.push(Some(local));
        // For `T?`, never redirect a non-Nil constant into the use site: bare
        // `0` would collapse with `nil` under `ne 0, nil`. Materialize via
        // Assign so codegen can box the scalar into a handle.
        let keep_materialized = is_nullable
            && matches!(
                &val,
                AmirOperand::Constant(c) if !matches!(c, AmirConstant::Nil)
            );

        if keep_materialized {
            self.push_stmt(AmirStmt::Assign {
                lhs: temp,
                rhs: AmirRvalue::Use(val),
            });
        } else {
            self.push_stmt(AmirStmt::Assign {
                lhs: temp,
                rhs: AmirRvalue::Load(AmirPlace {
                    local,
                    projections: smallvec::smallvec![],
                }),
            });
            // Register redirection so that rewrite_all_operands replaces temp with the actual SSA value (val)
            // Only do this for register types; memory types must always load from their actual memory place.
            if !is_mem {
                self.redirected_temps.insert(temp, val);
            }
        }

        Ok(AmirOperand::Copy(temp))
    }

    pub(crate) fn read_variable_recursive(
        &mut self,
        block: BlockId,
        local: LocalId,
    ) -> AmirOperand {
        let val = if !self.sealed_blocks.contains(&block) {
            // Block is not sealed: generate a placeholder block parameter
            let ty_id = self.locals[local.as_usize()].ty;
            let ty = self.resolve_ty(ty_id);
            let is_copy = ty.is_copy_v01();
            let temp_id = self.new_temp(ty);
            self.temp_origins[temp_id.as_usize()] = Some(local);
            let from_name = self.locals[local.as_usize()]
                .symbol
                .map(|sym| self.tc.symbols.get(sym).name.clone());
            let param = BlockParam {
                id: temp_id,
                local,
                ty: ty_id,
                from: from_name,
                moved: !is_copy,
            };
            self.blocks[block.as_usize()].params.push(param);
            let op = AmirOperand::Copy(temp_id);
            self.incomplete_phis
                .entry(block)
                .or_default()
                .push((local, temp_id));
            op
        } else {
            let preds = self.predecessors.get(&block).cloned().unwrap_or_default();
            if preds.len() == 1 {
                self.read_variable(preds[0], local)
            } else {
                let ty_id = self.locals[local.as_usize()].ty;
                let ty = self.resolve_ty(ty_id);
                let is_copy = ty.is_copy_v01();
                let temp_id = self.new_temp(ty);
                self.temp_origins[temp_id.as_usize()] = Some(local);
                let from_name = self.locals[local.as_usize()]
                    .symbol
                    .map(|sym| self.tc.symbols.get(sym).name.clone());
                let param = BlockParam {
                    id: temp_id,
                    local,
                    ty: ty_id,
                    from: from_name,
                    moved: !is_copy,
                };
                self.blocks[block.as_usize()].params.push(param);
                let op = AmirOperand::Copy(temp_id);
                self.write_variable(block, local, op);
                self.add_block_parameter_operands(block, local, temp_id);
                op
            }
        };
        self.write_variable(block, local, val.clone());
        val
    }

    fn add_block_parameter_operands(&mut self, block: BlockId, local: LocalId, _temp_id: TempId) {
        let preds = self.predecessors.get(&block).cloned().unwrap_or_default();
        for pred in preds {
            let val = self.read_variable(pred, local);
            self.append_terminator_arg(pred, block, val);
        }
    }

    fn simplify_phi(&mut self, block: BlockId, local: LocalId, temp_id: TempId) -> AmirOperand {
        let preds = self.predecessors.get(&block).cloned().unwrap_or_default();
        if preds.is_empty() {
            return AmirOperand::Copy(temp_id);
        }

        let mut unique_val: Option<AmirOperand> = None;
        for pred in preds {
            let val = self.read_variable(pred, local);
            if val == AmirOperand::Copy(temp_id) || val == AmirOperand::Move(temp_id) {
                continue;
            }
            if let Some(ref prev) = unique_val {
                if *prev != val {
                    return AmirOperand::Copy(temp_id);
                }
            } else {
                unique_val = Some(val);
            }
        }

        let val = unique_val.unwrap_or(AmirOperand::Constant(AmirConstant::Nil));
        // Keep `T?` temps that hold a non-Nil constant materialized — bare
        // integer constants must not replace a nullable handle (0 ≠ nil).
        if self.temps[temp_id.as_usize()].is_nullable
            && matches!(
                &val,
                AmirOperand::Constant(c) if !matches!(c, AmirConstant::Nil)
            )
        {
            return AmirOperand::Copy(temp_id);
        }
        self.redirected_temps.insert(temp_id, val.clone());
        val
    }

    pub(crate) fn eliminate_trivial_phis(&mut self) {
        loop {
            let mut changed = false;
            for block_idx in 0..self.blocks.len() {
                let block_id = BlockId::from_usize(block_idx);
                let params = self.blocks[block_idx].params.clone();
                for (param_idx, p) in params.into_iter().enumerate() {
                    if self.redirected_temps.contains_key(&p.id) {
                        continue;
                    }

                    let preds = self
                        .predecessors
                        .get(&block_id)
                        .cloned()
                        .unwrap_or_default();
                    if preds.is_empty() {
                        continue;
                    }

                    let mut unique_val: Option<AmirOperand> = None;
                    let mut is_trivial = true;

                    for pred in preds {
                        if let Some(arg) = self.get_terminator_arg(pred, block_id, param_idx) {
                            let resolved = Self::resolve_operand(&self.redirected_temps, arg);
                            if resolved == AmirOperand::Copy(p.id)
                                || resolved == AmirOperand::Move(p.id)
                            {
                                continue;
                            }
                            if let Some(ref prev) = unique_val {
                                if *prev != resolved {
                                    is_trivial = false;
                                    break;
                                }
                            } else {
                                unique_val = Some(resolved);
                            }
                        } else {
                            is_trivial = false;
                            break;
                        }
                    }

                    if is_trivial {
                        let val = unique_val.unwrap_or(AmirOperand::Constant(AmirConstant::Nil));
                        if self.temps[p.id.as_usize()].is_nullable
                            && matches!(
                                &val,
                                AmirOperand::Constant(c) if !matches!(c, AmirConstant::Nil)
                            )
                        {
                            // leave block param in place; codegen boxes the constant
                            continue;
                        }
                        self.redirected_temps.insert(p.id, val);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    pub(crate) fn prune_eliminated_parameters(&mut self) {
        for block_idx in 0..self.blocks.len() {
            let block_id = BlockId::from_usize(block_idx);
            let old_params = std::mem::take(&mut self.blocks[block_idx].params);
            let mut keep_indices = Vec::new();
            let mut new_params = Vec::new();
            for (i, p) in old_params.into_iter().enumerate() {
                if !self.redirected_temps.contains_key(&p.id) {
                    keep_indices.push(i);
                    new_params.push(p);
                }
            }
            self.blocks[block_idx].params = new_params;

            let preds = self
                .predecessors
                .get(&block_id)
                .cloned()
                .unwrap_or_default();
            for pred in preds {
                self.prune_terminator_args(pred, block_id, &keep_indices);
            }
        }
    }

    pub(crate) fn rewrite_all_operands(&mut self) {
        // Rewrite all statements
        for stmt_idx in 0..self.stmts.len() {
            let stmt_id = crate::amir::stmt::InstrId::from_usize(stmt_idx);
            if let Some(stmt) = self.stmts.get_mut(stmt_id) {
                Self::resolve_stmt(&self.redirected_temps, stmt);
            }
        }

        // Rewrite all block terminators
        for block_idx in 0..self.blocks.len() {
            let mut term = std::mem::replace(
                &mut self.blocks[block_idx].terminator,
                AmirTerminator::Unreachable,
            );
            Self::resolve_terminator(&self.redirected_temps, &mut term);
            self.blocks[block_idx].terminator = term;
        }
    }

    pub(crate) fn resolve_operand(
        redirected_temps: &FxHashMap<TempId, AmirOperand>,
        op: AmirOperand,
    ) -> AmirOperand {
        match op {
            AmirOperand::Copy(t) => {
                if let Some(&repl) = redirected_temps.get(&t) {
                    Self::resolve_operand(redirected_temps, repl)
                } else {
                    op
                }
            }
            AmirOperand::Move(t) => {
                if let Some(&repl) = redirected_temps.get(&t) {
                    let resolved = Self::resolve_operand(redirected_temps, repl);
                    match resolved {
                        AmirOperand::Copy(rt) => AmirOperand::Move(rt),
                        other => other,
                    }
                } else {
                    op
                }
            }
            _ => op,
        }
    }

    fn resolve_rvalue(redirected_temps: &FxHashMap<TempId, AmirOperand>, rval: &mut AmirRvalue) {
        match rval {
            AmirRvalue::Use(op) => {
                *op = Self::resolve_operand(redirected_temps, *op);
            }
            AmirRvalue::Binary { left, right, .. } => {
                *left = Self::resolve_operand(redirected_temps, *left);
                *right = Self::resolve_operand(redirected_temps, *right);
            }
            AmirRvalue::Unary { operand, .. } => {
                *operand = Self::resolve_operand(redirected_temps, *operand);
            }
            AmirRvalue::FieldAccess { base, .. } => {
                *base = Self::resolve_operand(redirected_temps, *base);
            }
            AmirRvalue::StructLiteral { fields, .. } => {
                for (_, op) in fields {
                    *op = Self::resolve_operand(redirected_temps, *op);
                }
            }
            AmirRvalue::IndexAccess { base, index } => {
                *base = Self::resolve_operand(redirected_temps, *base);
                *index = Self::resolve_operand(redirected_temps, *index);
            }
            AmirRvalue::Array { items } => {
                for op in items {
                    *op = Self::resolve_operand(redirected_temps, *op);
                }
            }
            AmirRvalue::Tuple { items } => {
                for op in items {
                    *op = Self::resolve_operand(redirected_temps, *op);
                }
            }
            AmirRvalue::Discriminant { value } => {
                *value = Self::resolve_operand(redirected_temps, *value);
            }
            AmirRvalue::EnumPayload { value, .. } => {
                *value = Self::resolve_operand(redirected_temps, *value);
            }
            AmirRvalue::EnumConstruct { payload, .. } => {
                if let Some(op) = payload {
                    *op = Self::resolve_operand(redirected_temps, *op);
                }
            }

            AmirRvalue::Len(value) => {
                *value = Self::resolve_operand(redirected_temps, *value);
            }
            AmirRvalue::Alloc(value) => {
                *value = Self::resolve_operand(redirected_temps, *value);
            }
            AmirRvalue::Load(place) => {
                Self::resolve_place(redirected_temps, place);
            }
            AmirRvalue::Borrow(place) => {
                Self::resolve_place(redirected_temps, place);
            }
            AmirRvalue::BorrowMut(place) => {
                Self::resolve_place(redirected_temps, place);
            }
            AmirRvalue::StringInterp { parts } => {
                for op in parts {
                    *op = Self::resolve_operand(redirected_temps, *op);
                }
            }
            AmirRvalue::ToStr { value, .. } => {
                *value = Self::resolve_operand(redirected_temps, *value);
            }
        }
    }

    fn resolve_place(redirected_temps: &FxHashMap<TempId, AmirOperand>, place: &mut AmirPlace) {
        for proj in &mut place.projections {
            if let AmirProjection::Index(op) = proj {
                *op = Self::resolve_operand(redirected_temps, *op);
            }
        }
    }

    fn resolve_stmt(redirected_temps: &FxHashMap<TempId, AmirOperand>, stmt: &mut AmirStmt) {
        match stmt {
            AmirStmt::Assign { lhs: _, rhs } => {
                Self::resolve_rvalue(redirected_temps, rhs);
            }
            AmirStmt::Store { lhs, rhs } => {
                Self::resolve_place(redirected_temps, lhs);
                *rhs = Self::resolve_operand(redirected_temps, rhs.clone());
            }
            AmirStmt::Call {
                lhs: _,
                callee,
                args,
            } => {
                *callee = Self::resolve_operand(redirected_temps, callee.clone());
                for arg in args {
                    *arg = Self::resolve_operand(redirected_temps, *arg);
                }
            }
            AmirStmt::Free(op) => {
                *op = Self::resolve_operand(redirected_temps, *op);
            }
            AmirStmt::Destroy(place) => {
                Self::resolve_place(redirected_temps, place);
            }
            _ => {}
        }
    }

    fn resolve_terminator(
        redirected_temps: &FxHashMap<TempId, AmirOperand>,
        term: &mut AmirTerminator,
    ) {
        match term {
            AmirTerminator::Goto { target: _, args } => {
                for arg in args {
                    *arg = Self::resolve_operand(redirected_temps, *arg);
                }
            }
            AmirTerminator::Branch {
                condition,
                if_true: _,
                true_args,
                if_false: _,
                false_args,
            } => {
                *condition = Self::resolve_operand(redirected_temps, condition.clone());
                for arg in true_args {
                    *arg = Self::resolve_operand(redirected_temps, *arg);
                }
                for arg in false_args {
                    *arg = Self::resolve_operand(redirected_temps, *arg);
                }
            }
            AmirTerminator::SwitchInt {
                discriminant,
                targets,
                otherwise,
            } => {
                *discriminant = Self::resolve_operand(redirected_temps, discriminant.clone());
                for (_, _, target_args) in targets {
                    for arg in target_args {
                        *arg = Self::resolve_operand(redirected_temps, *arg);
                    }
                }
                for arg in &mut otherwise.1 {
                    *arg = Self::resolve_operand(redirected_temps, *arg);
                }
            }
            _ => {}
        }
    }

    fn get_terminator_arg(
        &self,
        pred: BlockId,
        target_block: BlockId,
        param_idx: usize,
    ) -> Option<AmirOperand> {
        let term = &self.blocks[pred.as_usize()].terminator;
        match term {
            AmirTerminator::Goto { target, args } => {
                if *target == target_block {
                    args.get(param_idx).cloned()
                } else {
                    None
                }
            }
            AmirTerminator::Branch {
                if_true,
                true_args,
                if_false,
                false_args,
                ..
            } => {
                if *if_true == target_block {
                    true_args.get(param_idx).cloned()
                } else if *if_false == target_block {
                    false_args.get(param_idx).cloned()
                } else {
                    None
                }
            }
            AmirTerminator::SwitchInt {
                targets, otherwise, ..
            } => {
                for (_, dest, target_args) in targets {
                    if *dest == target_block {
                        return target_args.get(param_idx).cloned();
                    }
                }
                if otherwise.0 == target_block {
                    otherwise.1.get(param_idx).cloned()
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn append_terminator_arg(&mut self, pred: BlockId, target_block: BlockId, val: AmirOperand) {
        let term = &mut self.blocks[pred.as_usize()].terminator;
        match term {
            AmirTerminator::Goto { target, args } if *target == target_block => {
                args.push(val);
            }
            AmirTerminator::Branch {
                if_true,
                true_args,
                if_false,
                false_args,
                ..
            } => {
                if *if_true == target_block {
                    true_args.push(val.clone());
                }
                if *if_false == target_block {
                    false_args.push(val);
                }
            }
            AmirTerminator::SwitchInt {
                targets, otherwise, ..
            } => {
                for (_, dest, target_args) in targets {
                    if *dest == target_block {
                        target_args.push(val.clone());
                    }
                }
                if otherwise.0 == target_block {
                    otherwise.1.push(val);
                }
            }
            _ => {}
        }
    }

    fn prune_terminator_args(
        &mut self,
        pred: BlockId,
        target_block: BlockId,
        keep_indices: &[usize],
    ) {
        let term = &mut self.blocks[pred.as_usize()].terminator;
        match term {
            AmirTerminator::Goto { target, args } if *target == target_block => {
                *args = keep_indices.iter().map(|&i| args[i].clone()).collect();
            }
            AmirTerminator::Branch {
                if_true,
                true_args,
                if_false,
                false_args,
                ..
            } => {
                if *if_true == target_block {
                    *true_args = keep_indices.iter().map(|&i| true_args[i].clone()).collect();
                }
                if *if_false == target_block {
                    *false_args = keep_indices
                        .iter()
                        .map(|&i| false_args[i].clone())
                        .collect();
                }
            }
            AmirTerminator::SwitchInt {
                targets, otherwise, ..
            } => {
                for (_, dest, target_args) in targets {
                    if *dest == target_block {
                        *target_args = keep_indices
                            .iter()
                            .map(|&i| target_args[i].clone())
                            .collect();
                    }
                }
                if otherwise.0 == target_block {
                    otherwise.1 = keep_indices
                        .iter()
                        .map(|&i| otherwise.1[i].clone())
                        .collect();
                }
            }
            _ => {}
        }
    }

    pub(crate) fn build_target_args(&mut self, target: BlockId) -> Vec<AmirOperand> {
        let params = self.blocks[target.as_usize()].params.clone();
        let mut args = Vec::new();
        let Some(curr) = self.current_block else {
            return args;
        };
        for p in params {
            let arg = self.read_variable(curr, p.local);
            args.push(arg);
        }
        args
    }
}
