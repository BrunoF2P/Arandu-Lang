use super::{DeferFrame, DeferKind, LowerCtx};
use crate::SymbolTable;
use crate::amir::{
    AmirConstant, AmirOperand, AmirPlace, AmirProjection, AmirRvalue, AmirStmt, AmirTerminator,
    TempId,
};
use crate::diagnostics::Diagnostic;
use crate::hir::{HirForClause, HirPlace, HirPlaceSuffix, HirSimpleStmt, HirStmt, HirStmtKind};
use crate::literal_pool::AmirLiteralEntry;
use crate::ops::{BinaryOp, SetOp};
use crate::passes::type_checker::types::{ArType, Primitive};

impl LowerCtx<'_> {
    pub(crate) fn lower_stmt(
        &mut self,
        stmt: &HirStmt,
        symbols: &SymbolTable,
    ) -> Result<(), Diagnostic> {
        if self.current_block.is_none() {
            return Ok(());
        }

        match &stmt.kind {
            HirStmtKind::VarDecl { bindings, value } => {
                let bindings_slice = self.hir.pool.bindings_list(*bindings);
                if bindings_slice.len() == 1 {
                    let b = &bindings_slice[0];
                    let local_id = self.new_local(b.ty.clone(), b.symbol, b.span);
                    let val_op = self.lower_expr(*value, None, symbols)?;
                    self.emit_store_place(
                        AmirPlace {
                            local: local_id,
                            projections: smallvec::SmallVec::new(),
                        },
                        val_op,
                    )?;
                } else {
                    let val_op = self.lower_expr(*value, None, symbols)?;
                    for (i, b) in bindings_slice.iter().enumerate() {
                        let local_id = self.new_local(b.ty.clone(), b.symbol, b.span);
                        let temp = self.new_temp(b.ty.clone());
                        self.emit_assign_temp(
                            temp,
                            AmirRvalue::FieldAccess {
                                base: val_op.clone(),
                                field: format!("_{i}"),
                            },
                        );
                        self.emit_store_place(
                            AmirPlace {
                                local: local_id,
                                projections: smallvec::SmallVec::new(),
                            },
                            AmirOperand::Copy(temp),
                        )?;
                    }
                }
            }
            HirStmtKind::Set { places, op, value } => {
                let val_op = self.lower_expr(*value, None, symbols)?;
                self.lower_set_places(self.hir.pool.places_list(*places), op, &val_op, symbols)?;
            }
            HirStmtKind::Return { values } => {
                let values_slice = self.hir.pool.expr_list(*values);
                let is_error = self.is_error_return(values_slice);
                self.exit_all_defer_frames(is_error, symbols)?;
                if values_slice.is_empty() {
                    self.set_terminator(AmirTerminator::Return);
                } else if values_slice.len() == 1 {
                    self.lower_expr(values_slice[0], Some(TempId(0)), symbols)?;
                    self.set_terminator(AmirTerminator::Return);
                } else {
                    let mut ops = Vec::new();
                    for &v in values_slice {
                        ops.push(self.lower_expr(v, None, symbols)?);
                    }
                    self.emit_assign_temp(TempId(0), AmirRvalue::Tuple { items: ops });
                    self.set_terminator(AmirTerminator::Return);
                }
                self.current_block = None;
            }
            HirStmtKind::Break => {
                if let Some((_, exit_block, defer_depth)) = self.loop_stack.last().copied() {
                    self.exit_defer_frames_from(defer_depth, false, symbols)?;
                    self.set_terminator(AmirTerminator::Goto(exit_block));
                    self.current_block = None;
                }
            }
            HirStmtKind::Continue => {
                if let Some((cont_block, _, defer_depth)) = self.loop_stack.last().copied() {
                    self.exit_defer_frames_from(defer_depth, false, symbols)?;
                    self.set_terminator(AmirTerminator::Goto(cont_block));
                    self.current_block = None;
                }
            }
            HirStmtKind::Expr(expr) => {
                self.lower_expr(*expr, None, symbols)?;
            }
            HirStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                let cond_op = self.lower_condition(condition, symbols)?;
                let bb_then = self.new_block();
                let bb_else = self.new_block();
                let bb_join = self.new_block();

                self.set_bool_branch(cond_op, bb_then, bb_else);

                // Then
                self.current_block = Some(bb_then);
                self.lower_block(*then_block, symbols)?;
                if self.current_block.is_some() {
                    self.set_terminator(AmirTerminator::Goto(bb_join));
                }

                // Else
                self.current_block = Some(bb_else);
                if let Some(eb) = else_block {
                    self.lower_block(*eb, symbols)?;
                }
                if self.current_block.is_some() {
                    self.set_terminator(AmirTerminator::Goto(bb_join));
                }

                self.current_block = Some(bb_join);
            }
            HirStmtKind::While { condition, body } => {
                let bb_cond = self.new_block();
                let bb_body = self.new_block();
                let bb_exit = self.new_block();

                self.set_terminator(AmirTerminator::Goto(bb_cond));

                self.current_block = Some(bb_cond);
                let cond_op = self.lower_condition(condition, symbols)?;
                self.set_bool_branch(cond_op, bb_body, bb_exit);

                let defer_depth = self.defer_frames.len();
                self.loop_stack.push((bb_cond, bb_exit, defer_depth));
                self.current_block = Some(bb_body);
                self.lower_block(*body, symbols)?;
                if self.current_block.is_some() {
                    self.set_terminator(AmirTerminator::Goto(bb_cond));
                }
                self.loop_stack.pop();

                self.current_block = Some(bb_exit);
            }
            HirStmtKind::For { clause, body } => match clause {
                HirForClause::In {
                    span: _,
                    bindings,
                    iterable,
                } => {
                    let iter_op = self.lower_expr(*iterable, None, symbols)?;

                    let idx_local = self.new_compiler_local(ArType::Primitive(Primitive::Int));
                    let zero_lit = self.intern_literal(AmirLiteralEntry::Int("0".to_string()));
                    self.emit_store_place(
                        AmirPlace {
                            local: idx_local,
                            projections: smallvec::SmallVec::new(),
                        },
                        AmirOperand::Constant(zero_lit),
                    )?;

                    let len_local = self.new_compiler_local(ArType::Primitive(Primitive::Int));
                    let len_temp = self.new_temp(ArType::Primitive(Primitive::Int));
                    self.emit_assign_temp(len_temp, AmirRvalue::Len(iter_op.clone()));
                    self.emit_store_place(
                        AmirPlace {
                            local: len_local,
                            projections: smallvec::SmallVec::new(),
                        },
                        AmirOperand::Copy(len_temp),
                    )?;

                    let bb_cond = self.new_block();
                    let bb_body = self.new_block();
                    let bb_step = self.new_block();
                    let bb_exit = self.new_block();

                    self.set_terminator(AmirTerminator::Goto(bb_cond));

                    self.current_block = Some(bb_cond);
                    let idx_op = self.load_place(
                        &AmirPlace {
                            local: idx_local,
                            projections: smallvec::SmallVec::new(),
                        },
                        ArType::Primitive(Primitive::Int),
                        &[],
                    )?;
                    let len_op = self.load_place(
                        &AmirPlace {
                            local: len_local,
                            projections: smallvec::SmallVec::new(),
                        },
                        ArType::Primitive(Primitive::Int),
                        &[],
                    )?;
                    let cond_tmp = self.new_temp(ArType::Primitive(Primitive::Bool));
                    self.emit_assign_temp(
                        cond_tmp,
                        AmirRvalue::Binary {
                            op: BinaryOp::Lt,
                            left: idx_op,
                            right: len_op,
                        },
                    );
                    self.set_bool_branch(AmirOperand::Copy(cond_tmp), bb_body, bb_exit);

                    let defer_depth = self.defer_frames.len();
                    self.loop_stack.push((bb_step, bb_exit, defer_depth));
                    self.current_block = Some(bb_body);

                    let bindings_slice = self.hir.pool.for_bindings_list(*bindings);
                    if let Some(binding) = bindings_slice.first() {
                        let local_id = self
                            .symbol_map
                            .get(&binding.symbol)
                            .copied()
                            .unwrap_or_else(|| {
                                self.new_local(binding.ty.clone(), binding.symbol, binding.span)
                            });
                        let idx_op2 = self.load_place(
                            &AmirPlace {
                                local: idx_local,
                                projections: smallvec::SmallVec::new(),
                            },
                            ArType::Primitive(Primitive::Int),
                            &[],
                        )?;
                        let elem_temp = self.new_temp(binding.ty.clone());
                        self.emit_assign_temp(
                            elem_temp,
                            AmirRvalue::IndexAccess {
                                base: iter_op.clone(),
                                index: idx_op2,
                            },
                        );
                        self.emit_store_place(
                            AmirPlace {
                                local: local_id,
                                projections: smallvec::SmallVec::new(),
                            },
                            AmirOperand::Copy(elem_temp),
                        )?;
                    }

                    self.lower_block(*body, symbols)?;
                    if self.current_block.is_some() {
                        self.set_terminator(AmirTerminator::Goto(bb_step));
                    }
                    self.loop_stack.pop();

                    self.current_block = Some(bb_step);
                    let idx_op3 = self.load_place(
                        &AmirPlace {
                            local: idx_local,
                            projections: smallvec::SmallVec::new(),
                        },
                        ArType::Primitive(Primitive::Int),
                        &[],
                    )?;
                    let one_lit = self.intern_literal(AmirLiteralEntry::Int("1".to_string()));
                    let next_idx = self.new_temp(ArType::Primitive(Primitive::Int));
                    self.emit_assign_temp(
                        next_idx,
                        AmirRvalue::Binary {
                            op: BinaryOp::Add,
                            left: idx_op3,
                            right: AmirOperand::Constant(one_lit),
                        },
                    );
                    self.emit_store_place(
                        AmirPlace {
                            local: idx_local,
                            projections: smallvec::SmallVec::new(),
                        },
                        AmirOperand::Copy(next_idx),
                    )?;
                    self.set_terminator(AmirTerminator::Goto(bb_cond));

                    self.current_block = Some(bb_exit);
                }
                HirForClause::CStyle {
                    init,
                    condition,
                    step,
                    ..
                } => {
                    if let Some(i) = init {
                        self.lower_simple_stmt(i, symbols)?;
                    }

                    let bb_cond = self.new_block();
                    let bb_body = self.new_block();
                    let bb_step = self.new_block();
                    let bb_exit = self.new_block();

                    self.set_terminator(AmirTerminator::Goto(bb_cond));

                    self.current_block = Some(bb_cond);
                    let cond_op = if let Some(c) = condition {
                        self.lower_expr(*c, None, symbols)?
                    } else {
                        AmirOperand::Constant(AmirConstant::Bool(true))
                    };
                    self.set_bool_branch(cond_op, bb_body, bb_exit);

                    let defer_depth = self.defer_frames.len();
                    self.loop_stack.push((bb_step, bb_exit, defer_depth));
                    self.current_block = Some(bb_body);
                    self.lower_block(*body, symbols)?;
                    if self.current_block.is_some() {
                        self.set_terminator(AmirTerminator::Goto(bb_step));
                    }
                    self.loop_stack.pop();

                    self.current_block = Some(bb_step);
                    if let Some(s) = step {
                        self.lower_simple_stmt(s, symbols)?;
                    }
                    if self.current_block.is_some() {
                        self.set_terminator(AmirTerminator::Goto(bb_cond));
                    }

                    self.current_block = Some(bb_exit);
                }
            },
            HirStmtKind::Match { value, arms } => {
                let bb_end = self.new_block();
                self.lower_match_stmt(*value, arms, bb_end, symbols)?;
            }
            HirStmtKind::Unsafe(b) => {
                self.lower_block(*b, symbols)?;
            }
            HirStmtKind::Defer(block) => {
                self.register_defer(self.hir.pool.block(*block), DeferKind::Defer);
            }
            HirStmtKind::ErrDefer(block) => {
                self.register_defer(self.hir.pool.block(*block), DeferKind::ErrDefer);
            }
            HirStmtKind::Free(expr) => {
                let op = self.lower_expr(*expr, None, symbols)?;
                self.push_stmt(AmirStmt::Free(op));
            }
        }
        Ok(())
    }

    pub(crate) fn lower_simple_stmt(
        &mut self,
        stmt: &HirSimpleStmt,
        symbols: &SymbolTable,
    ) -> Result<(), Diagnostic> {
        if self.current_block.is_none() {
            return Ok(());
        }
        match stmt {
            HirSimpleStmt::VarDecl { bindings, value } => {
                let bindings_slice = self.hir.pool.bindings_list(*bindings);
                if bindings_slice.len() == 1 {
                    let b = &bindings_slice[0];
                    let local_id = self.new_local(b.ty.clone(), b.symbol, b.span);
                    let val_op = self.lower_expr(*value, None, symbols)?;
                    self.emit_store_place(
                        AmirPlace {
                            local: local_id,
                            projections: smallvec::SmallVec::new(),
                        },
                        val_op,
                    )?;
                } else {
                    let val_op = self.lower_expr(*value, None, symbols)?;
                    for (i, b) in bindings_slice.iter().enumerate() {
                        let local_id = self.new_local(b.ty.clone(), b.symbol, b.span);
                        let temp = self.new_temp(b.ty.clone());
                        self.emit_assign_temp(
                            temp,
                            AmirRvalue::FieldAccess {
                                base: val_op.clone(),
                                field: format!("_{i}"),
                            },
                        );
                        self.emit_store_place(
                            AmirPlace {
                                local: local_id,
                                projections: smallvec::SmallVec::new(),
                            },
                            AmirOperand::Copy(temp),
                        )?;
                    }
                }
            }
            HirSimpleStmt::Set { places, op, value } => {
                let val_op = self.lower_expr(*value, None, symbols)?;
                self.lower_set_places(self.hir.pool.places_list(*places), op, &val_op, symbols)?;
            }
            HirSimpleStmt::Expr(expr) => {
                self.lower_expr(*expr, None, symbols)?;
            }
        }
        Ok(())
    }

    pub(crate) fn lower_set_places(
        &mut self,
        places: &[HirPlace],
        op: &SetOp,
        val_op: &AmirOperand,
        symbols: &SymbolTable,
    ) -> Result<(), Diagnostic> {
        if places.len() == 1 {
            let place = &places[0];
            if let Some(&local_id) = self.symbol_map.get(&place.root_symbol) {
                let projection_types: Vec<ArType> = place
                    .suffixes
                    .iter()
                    .map(|s| match s {
                        HirPlaceSuffix::Field { ty, .. } | HirPlaceSuffix::Index { ty, .. } => {
                            ty.clone()
                        }
                    })
                    .collect();
                let projections: Result<Vec<_>, Diagnostic> = place
                    .suffixes
                    .iter()
                    .map(|s| match s {
                        HirPlaceSuffix::Field {
                            field_symbol: Some(symbol),
                            ..
                        } => Ok(AmirProjection::Field(*symbol)),
                        HirPlaceSuffix::Field { span, name, .. } => Err(crate::Diagnostic::error(
                            crate::DiagCode::L001LoweringUnresolvedSymbol,
                            format!("cannot lower field projection `{name}`: symbol not resolved"),
                            *span,
                        )),
                        HirPlaceSuffix::Index { expr, .. } => {
                            Ok(AmirProjection::Index(self.lower_expr(*expr, None, symbols)?))
                        }
                    })
                    .collect();
                let amir_place = AmirPlace {
                    local: local_id,
                    projections: projections?.into(),
                };

                if *op == SetOp::Assign {
                    self.emit_store_place(amir_place, val_op.clone())?;
                } else {
                    let bin_op = match op {
                        SetOp::AddAssign => BinaryOp::Add,
                        SetOp::SubAssign => BinaryOp::Sub,
                        SetOp::MulAssign => BinaryOp::Mul,
                        SetOp::DivAssign => BinaryOp::Div,
                        SetOp::ModAssign => BinaryOp::Mod,
                        SetOp::BitAndAssign => BinaryOp::BitAnd,
                        SetOp::BitOrAssign => BinaryOp::BitOr,
                        SetOp::BitXorAssign => BinaryOp::BitXor,
                        SetOp::ShiftLeftAssign => BinaryOp::ShiftLeft,
                        SetOp::ShiftRightAssign => BinaryOp::ShiftRight,
                        _ => BinaryOp::Add,
                    };
                    let old_val =
                        self.load_place(&amir_place, place.ty.clone(), &projection_types)?;
                    let temp = self.new_temp(place.ty.clone());
                    self.emit_assign_temp(
                        temp,
                        AmirRvalue::Binary {
                            op: bin_op,
                            left: old_val,
                            right: val_op.clone(),
                        },
                    );
                    self.emit_store_place(amir_place, AmirOperand::Copy(temp))?;
                }
            }
        } else {
            for (i, place) in places.iter().enumerate() {
                if let Some(&local_id) = self.symbol_map.get(&place.root_symbol) {
                    let projections: Result<Vec<_>, Diagnostic> = place
                        .suffixes
                        .iter()
                        .map(|s| match s {
                            HirPlaceSuffix::Field {
                                field_symbol: Some(symbol),
                                ..
                            } => Ok(AmirProjection::Field(*symbol)),
                            HirPlaceSuffix::Field { span, name, .. } => {
                                Err(crate::Diagnostic::error(
                                    crate::DiagCode::L001LoweringUnresolvedSymbol,
                                    format!("cannot lower field projection `{name}`: symbol not resolved"),
                                    *span,
                                ))
                            }
                            HirPlaceSuffix::Index { expr, .. } => {
                                Ok(AmirProjection::Index(self.lower_expr(*expr, None, symbols)?))
                            }
                        })
                        .collect();
                    let amir_place = AmirPlace {
                        local: local_id,
                        projections: projections?.into(),
                    };

                    let temp_ty = place.ty.clone();
                    let temp = self.new_temp(temp_ty);
                    self.emit_assign_temp(
                        temp,
                        AmirRvalue::FieldAccess {
                            base: val_op.clone(),
                            field: format!("_{i}"),
                        },
                    );
                    self.emit_store_place(amir_place, AmirOperand::Copy(temp))?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn lower_block(
        &mut self,
        block: crate::hir::HirBlockId,
        symbols: &SymbolTable,
    ) -> Result<(), Diagnostic> {
        self.defer_frames.push(DeferFrame {
            entries: Vec::new(),
        });
        let blk = self.hir.pool.block(block);
        for &stmt_id in self.hir.pool.stmt_list(blk.statements) {
            let stmt = self.hir.pool.stmt(stmt_id);
            self.lower_stmt(stmt, symbols)?;
        }
        if self.current_block.is_some() {
            self.exit_current_defer_frame(false, symbols)?;
        }
        Ok(())
    }

    /// Lower a block as an expression: all statements are lowered, and if the
    /// last statement is an expression statement, its value is assigned to `target`.
    pub(crate) fn lower_block_as_expr(
        &mut self,
        block: crate::hir::HirBlockId,
        target: Option<TempId>,
        symbols: &SymbolTable,
    ) -> Result<(), Diagnostic> {
        self.defer_frames.push(DeferFrame {
            entries: Vec::new(),
        });
        let blk = self.hir.pool.block(block);
        let statements_slice = self.hir.pool.stmt_list(blk.statements);
        if statements_slice.is_empty() {
            if let Some(dest) = target {
                self.emit_assign_temp(
                    dest,
                    AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Nil)),
                );
            }
            if self.current_block.is_some() {
                self.exit_current_defer_frame(false, symbols)?;
            }
            return Ok(());
        }
        let last_idx = statements_slice.len() - 1;
        for (i, &stmt_id) in statements_slice.iter().enumerate() {
            let stmt = self.hir.pool.stmt(stmt_id);
            if i == last_idx {
                if let HirStmtKind::Expr(expr) = stmt.kind {
                    self.lower_expr(expr, target, symbols)?;
                } else {
                    self.lower_stmt(stmt, symbols)?;
                    if let Some(dest) = target {
                        self.emit_assign_temp(
                            dest,
                            AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Nil)),
                        );
                    }
                }
            } else {
                self.lower_stmt(stmt, symbols)?;
            }
        }
        if self.current_block.is_some() {
            self.exit_current_defer_frame(false, symbols)?;
        }
        Ok(())
    }
}
