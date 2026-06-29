use super::{LowerCtx, amir_unsupported};
use crate::amir::{
    AmirConstant, AmirOperand, AmirPlace, AmirRvalue, AmirStmt, AmirTerminator, TempId,
};
use crate::diagnostics::Diagnostic;
use crate::literal_pool::AmirLiteralEntry;
use crate::ops::BinaryOp;
use crate::passes::type_checker::types::{ArType, Primitive, is_option_type, result_ok_err};
use crate::{SymbolKind, SymbolTable};

use crate::hir::{HirExpr, HirExprId, HirExprKind, ResultCtorVariant};

fn resolve_method_target(
    callee: &HirExpr,
    pool: &crate::hir::HirPool,
    symbols: &SymbolTable,
) -> Option<crate::SymbolId> {
    let (base_id, field) = match &callee.kind {
        HirExprKind::Field { base, field } | HirExprKind::SafeField { base, field } => {
            (*base, field)
        }
        _ => return None,
    };

    let base_expr = pool.expr(base_id);
    let base_ty = &base_expr.ty;
    let struct_id = match base_ty {
        ArType::Nullable(inner) => {
            arandu_middle::types::type_interner::with_resolved_type(*inner, |inner_ty| {
                match inner_ty {
                    ArType::Named(id, _) => Some(*id),
                    ArType::Ptr(ptr_inner) => {
                        arandu_middle::types::type_interner::with_resolved_type(
                            *ptr_inner,
                            |ptr_inner_ty| match ptr_inner_ty {
                                ArType::Named(id, _) => Some(*id),
                                _ => None,
                            },
                        )
                    }
                    _ => None,
                }
            })
        }
        ArType::Named(id, _) => Some(*id),
        ArType::Ptr(inner) => {
            arandu_middle::types::type_interner::with_resolved_type(*inner, |inner_ty| {
                match inner_ty {
                    ArType::Named(id, _) => Some(*id),
                    _ => None,
                }
            })
        }
        _ => None,
    }?;

    let struct_name = symbols.get(struct_id).name.clone();
    symbols.lookup_associated_member(&struct_name, field)
}

impl LowerCtx<'_> {
    fn resolve_field_index(&self, base_ty: &ArType, field: &str) -> usize {
        if let Ok(idx) = field.parse::<usize>() {
            return idx;
        }
        if field.starts_with('_')
            && let Ok(idx) = field[1..].parse::<usize>()
        {
            return idx;
        }
        let struct_id = match base_ty {
            ArType::Nullable(inner) => {
                arandu_middle::types::type_interner::with_resolved_type(*inner, |inner_ty| {
                    match inner_ty {
                        ArType::Named(id, _) => Some(*id),
                        ArType::Ptr(ptr_inner) => {
                            arandu_middle::types::type_interner::with_resolved_type(
                                *ptr_inner,
                                |ptr_inner_ty| match ptr_inner_ty {
                                    ArType::Named(id, _) => Some(*id),
                                    _ => None,
                                },
                            )
                        }
                        _ => None,
                    }
                })
            }
            ArType::Named(id, _) => Some(*id),
            ArType::Ptr(inner) => {
                arandu_middle::types::type_interner::with_resolved_type(*inner, |inner_ty| {
                    match inner_ty {
                        ArType::Named(id, _) => Some(*id),
                        _ => None,
                    }
                })
            }
            _ => None,
        };
        struct_id
            .and_then(|sid| self.tc.type_info.struct_field_indices.get(&sid))
            .and_then(|m| m.get(field).copied())
            .unwrap_or(0)
    }

    pub(crate) fn expr_is_nil(expr: &HirExpr) -> bool {
        matches!(expr.kind, HirExprKind::Nil)
    }

    pub(crate) fn expr_is_result_err(&self, expr: &HirExpr) -> bool {
        match &expr.kind {
            HirExprKind::ResultCtor {
                variant: ResultCtorVariant::Err,
                ..
            } => true,
            HirExprKind::ResultCtor {
                variant: ResultCtorVariant::Ok | ResultCtorVariant::Some,
                ..
            } => false,
            HirExprKind::Nil => false,
            _ => matches!(expr.ty, ArType::Err) && !Self::expr_is_nil(expr),
        }
    }

    pub(crate) fn is_error_return(&self, values: &[HirExprId]) -> bool {
        if result_ok_err(&self.func_return_type).is_none() {
            return false;
        }
        match values.len() {
            0 => false,
            1 => self.expr_is_result_err(self.hir.pool.expr(values[0])),
            _ => false,
        }
    }

    pub(crate) fn lower_result_ok_field(&mut self, base: AmirOperand, dest: TempId) {
        self.emit_assign_temp(dest, AmirRvalue::FieldAccess { base, field: 0 });
    }

    pub(crate) fn lower_result_err_field(
        &mut self,
        base: AmirOperand,
        err_ty: ArType,
        dest: TempId,
    ) {
        self.emit_assign_temp(dest, AmirRvalue::FieldAccess { base, field: 1 });
        let _ = err_ty;
    }

    pub(crate) fn lower_try_result(
        &mut self,
        inner_id: HirExprId,
        target: Option<TempId>,
        expr_ty: ArType,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        let inner = self.hir.pool.expr(inner_id);
        let (_, err_ty) = result_ok_err(&inner.ty).expect("try_result on non-result");
        let base = self.lower_expr(inner_id, None, symbols)?;
        if self.current_block.is_none() {
            let dest = target.unwrap_or_else(|| self.new_temp(expr_ty));
            return Ok(AmirOperand::Copy(dest));
        }

        let err_tmp = self.new_temp(err_ty.clone());
        self.lower_result_err_field(base.clone(), err_ty, err_tmp);

        let cond_tmp = self.new_temp(ArType::Primitive(Primitive::Bool));
        self.emit_assign_temp(
            cond_tmp,
            AmirRvalue::Binary {
                op: BinaryOp::NotEqual,
                left: AmirOperand::Copy(err_tmp),
                right: AmirOperand::Constant(AmirConstant::Nil),
            },
        );

        let bb_return_err = self.new_block();
        let bb_continue = self.new_block();

        self.set_terminator(AmirTerminator::Branch {
            condition: AmirOperand::Copy(cond_tmp),
            if_true: bb_return_err,
            if_false: bb_continue,
        });

        self.current_block = Some(bb_return_err);
        self.exit_all_defer_frames(true, symbols)?;
        self.emit_assign_temp(TempId(0), AmirRvalue::Use(AmirOperand::Copy(err_tmp)));
        self.set_terminator(AmirTerminator::Return);
        self.current_block = None;

        self.current_block = Some(bb_continue);
        let dest = target.unwrap_or_else(|| self.new_temp(expr_ty));
        self.lower_result_ok_field(base, dest);
        Ok(AmirOperand::Copy(dest))
    }

    pub(crate) fn lower_try_nullable(
        &mut self,
        inner_id: HirExprId,
        target: Option<TempId>,
        expr_ty: ArType,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        let base = self.lower_expr(inner_id, None, symbols)?;
        if self.current_block.is_none() {
            let dest = target.unwrap_or_else(|| self.new_temp(expr_ty));
            return Ok(AmirOperand::Copy(dest));
        }

        let cond_tmp = self.new_temp(ArType::Primitive(Primitive::Bool));
        self.emit_assign_temp(
            cond_tmp,
            AmirRvalue::Binary {
                op: BinaryOp::Equal,
                left: base.clone(),
                right: AmirOperand::Constant(AmirConstant::Nil),
            },
        );

        let bb_return_nil = self.new_block();
        let bb_continue = self.new_block();

        self.set_terminator(AmirTerminator::Branch {
            condition: AmirOperand::Copy(cond_tmp),
            if_true: bb_return_nil,
            if_false: bb_continue,
        });

        self.current_block = Some(bb_return_nil);
        self.exit_all_defer_frames(true, symbols)?;
        self.emit_assign_temp(
            TempId(0),
            AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Nil)),
        );
        self.set_terminator(AmirTerminator::Return);
        self.current_block = None;

        self.current_block = Some(bb_continue);
        let dest = target.unwrap_or_else(|| self.new_temp(expr_ty));
        self.emit_assign_temp(dest, AmirRvalue::Use(base));
        Ok(AmirOperand::Copy(dest))
    }

    pub(crate) fn lower_expr(
        &mut self,
        expr_id: HirExprId,
        target: Option<TempId>,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        let expr = self.hir.pool.expr(expr_id);
        match &expr.kind {
            HirExprKind::Int(v) => {
                let op =
                    AmirOperand::Constant(self.intern_literal(AmirLiteralEntry::Int(v.clone())));
                if let Some(dest) = target {
                    self.emit_assign_temp(dest, AmirRvalue::Use(op.clone()));
                }
                Ok(op)
            }
            HirExprKind::Float(v) => {
                let op =
                    AmirOperand::Constant(self.intern_literal(AmirLiteralEntry::Float(v.clone())));
                if let Some(dest) = target {
                    self.emit_assign_temp(dest, AmirRvalue::Use(op.clone()));
                }
                Ok(op)
            }
            HirExprKind::Bool(v) => {
                let op = AmirOperand::Constant(AmirConstant::Bool(*v));
                if let Some(dest) = target {
                    self.emit_assign_temp(dest, AmirRvalue::Use(op.clone()));
                }
                Ok(op)
            }
            HirExprKind::Str(v) => {
                let op =
                    AmirOperand::Constant(self.intern_literal(AmirLiteralEntry::Str(v.clone())));
                if let Some(dest) = target {
                    self.emit_assign_temp(dest, AmirRvalue::Use(op.clone()));
                }
                Ok(op)
            }
            HirExprKind::Char(v) => {
                let op =
                    AmirOperand::Constant(self.intern_literal(AmirLiteralEntry::Char(v.clone())));
                if let Some(dest) = target {
                    self.emit_assign_temp(dest, AmirRvalue::Use(op.clone()));
                }
                Ok(op)
            }
            HirExprKind::Nil => {
                let op = AmirOperand::Constant(AmirConstant::Nil);
                if let Some(dest) = target {
                    self.emit_assign_temp(dest, AmirRvalue::Use(op.clone()));
                }
                Ok(op)
            }
            HirExprKind::Path { symbol } => {
                let op = if let Some(&local_id) = self.symbol_map.get(symbol) {
                    let place = AmirPlace {
                        local: local_id,
                        projections: smallvec::SmallVec::new(),
                    };
                    self.load_place(&place, expr.ty.clone(), &[])
                } else {
                    let sym = symbols.get(*symbol);
                    Ok(match sym.kind {
                        SymbolKind::Func
                        | SymbolKind::ExternFunc
                        | SymbolKind::AssociatedFunc
                        | SymbolKind::NamespaceMember => AmirOperand::FunctionRef(*symbol),
                        _ => AmirOperand::GlobalRef(*symbol),
                    })
                }?;
                if let Some(dest) = target {
                    let rhs = self.consume_operand(op.clone())?;
                    self.emit_assign_temp(dest, AmirRvalue::Use(rhs));
                }
                Ok(op)
            }
            HirExprKind::TypePath { member_symbol, .. } => {
                let op = if let Some(&local_id) = self.symbol_map.get(member_symbol) {
                    let place = AmirPlace {
                        local: local_id,
                        projections: smallvec::SmallVec::new(),
                    };
                    self.load_place(&place, expr.ty.clone(), &[])
                } else {
                    Ok(AmirOperand::GlobalRef(*member_symbol))
                }?;
                if let Some(dest) = target {
                    let rhs = self.consume_operand(op.clone())?;
                    self.emit_assign_temp(dest, AmirRvalue::Use(rhs));
                }
                Ok(op)
            }
            HirExprKind::Generic { callee, .. } => self.lower_expr(*callee, target, symbols),
            HirExprKind::Alloc { expr: inner } => {
                let inner_op = self.lower_expr(*inner, None, symbols)?;
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                self.emit_assign_temp(dest, AmirRvalue::Alloc(inner_op));
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::Binary { op, left, right } => {
                let l_op = self.lower_expr(*left, None, symbols)?;
                let r_op = self.lower_expr(*right, None, symbols)?;
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                self.emit_assign_temp(
                    dest,
                    AmirRvalue::Binary {
                        op: *op,
                        left: l_op,
                        right: r_op,
                    },
                );
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::Unary { op, expr: sub_expr } => {
                let sub_op = self.lower_expr(*sub_expr, None, symbols)?;
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                self.emit_assign_temp(
                    dest,
                    AmirRvalue::Unary {
                        op: *op,
                        operand: sub_op,
                    },
                );
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::Field { base, field } => {
                let base_op = self.lower_expr(*base, None, symbols)?;
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                let base_expr = self.hir.pool.expr(*base);
                let field_idx = self.resolve_field_index(&base_expr.ty, field);
                self.emit_assign_temp(
                    dest,
                    AmirRvalue::FieldAccess {
                        base: base_op,
                        field: field_idx,
                    },
                );
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::Index { base, index } => {
                let base_op = self.lower_expr(*base, None, symbols)?;
                let idx_op = self.lower_expr(*index, None, symbols)?;
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                self.emit_assign_temp(
                    dest,
                    AmirRvalue::IndexAccess {
                        base: base_op,
                        index: idx_op,
                    },
                );
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::Array { items } => {
                let items_slice = self.hir.pool.expr_list(*items);
                let mut item_ops = Vec::with_capacity(items_slice.len());
                for &item in items_slice {
                    item_ops.push(self.lower_expr(item, None, symbols)?);
                }
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                self.emit_assign_temp(dest, AmirRvalue::Array { items: item_ops });
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::Call { callee, args, .. } => {
                let callee_expr = self.hir.pool.expr(*callee);
                let method_target = resolve_method_target(callee_expr, &self.hir.pool, symbols);
                let callee_op = if let Some(method_symbol) = method_target {
                    AmirOperand::FunctionRef(method_symbol)
                } else {
                    self.lower_expr(*callee, None, symbols)?
                };
                let args_slice = self.hir.pool.expr_list(*args);
                let mut arg_ops = Vec::with_capacity(args_slice.len() + 1);
                if method_target.is_some()
                    && let HirExprKind::Field { base, .. } | HirExprKind::SafeField { base, .. } =
                        &callee_expr.kind
                {
                    let base_op = self.lower_expr(*base, None, symbols)?;
                    arg_ops.push(base_op);
                }
                for &arg in args_slice {
                    let arg_op = self.lower_expr(arg, None, symbols)?;
                    arg_ops.push(self.consume_operand(arg_op)?);
                }
                let dest = if matches!(expr.ty, ArType::Void) {
                    None
                } else {
                    Some(target.unwrap_or_else(|| self.new_temp(expr.ty.clone())))
                };
                self.push_stmt(AmirStmt::Call {
                    lhs: dest,
                    callee: callee_op,
                    args: arg_ops.into(),
                });
                Ok(dest.map_or(AmirOperand::Constant(AmirConstant::Nil), AmirOperand::Copy))
            }
            HirExprKind::StructLiteral {
                struct_symbol,
                fields,
            } => {
                let fields_slice = self.hir.pool.field_inits_list(*fields);
                let mut field_ops = Vec::with_capacity(fields_slice.len());
                for f in fields_slice {
                    field_ops.push((f.name.clone(), self.lower_expr(f.value, None, symbols)?));
                }
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                self.emit_assign_temp(
                    dest,
                    AmirRvalue::StructLiteral {
                        struct_symbol: *struct_symbol,
                        fields: field_ops,
                    },
                );
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::If {
                condition,
                then_block,
                else_block,
            } => {
                let cond_op = self.lower_condition(condition, symbols)?;
                if self.current_block.is_none() {
                    let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                    return Ok(AmirOperand::Copy(dest));
                }
                let bb_then = self.new_block();
                let bb_else = self.new_block();
                let bb_join = self.new_block();

                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));

                self.set_bool_branch(cond_op, bb_then, bb_else);

                // Then branch
                self.current_block = Some(bb_then);
                self.lower_block_as_expr(*then_block, Some(dest), symbols)?;
                if self.current_block.is_some() {
                    self.set_terminator(AmirTerminator::Goto(bb_join));
                }

                // Else branch
                self.current_block = Some(bb_else);
                self.lower_block_as_expr(*else_block, Some(dest), symbols)?;
                if self.current_block.is_some() {
                    self.set_terminator(AmirTerminator::Goto(bb_join));
                }

                // Join
                self.current_block = Some(bb_join);
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::Cast { expr: sub_expr, .. } => {
                let sub_op = self.lower_expr(*sub_expr, None, symbols)?;
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                self.emit_assign_temp(dest, AmirRvalue::Use(sub_op));
                Ok(AmirOperand::Copy(dest))
            }

            HirExprKind::Match { value, arms } => {
                self.lower_match(*value, arms, target, expr.ty.clone(), symbols)
            }
            HirExprKind::ResultCtor { variant, value } => {
                let val_op = self.lower_expr(*value, None, symbols)?;
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                match variant {
                    ResultCtorVariant::Ok => {
                        self.emit_assign_temp(
                            dest,
                            AmirRvalue::Tuple {
                                items: vec![val_op, AmirOperand::Constant(AmirConstant::Nil)],
                            },
                        );
                    }
                    ResultCtorVariant::Err => {
                        self.emit_assign_temp(
                            dest,
                            AmirRvalue::Tuple {
                                items: vec![AmirOperand::Constant(AmirConstant::Nil), val_op],
                            },
                        );
                    }
                    ResultCtorVariant::Some => {
                        self.emit_assign_temp(dest, AmirRvalue::Use(val_op));
                    }
                }
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::Try { expr: inner } => {
                let inner_expr = self.hir.pool.expr(*inner);
                if result_ok_err(&inner_expr.ty).is_some() {
                    self.lower_try_result(*inner, target, expr.ty.clone(), symbols)
                } else if is_option_type(&inner_expr.ty)
                    || matches!(inner_expr.ty, ArType::Nullable(_))
                {
                    self.lower_try_nullable(*inner, target, expr.ty.clone(), symbols)
                } else {
                    self.lower_try_result(*inner, target, expr.ty.clone(), symbols)
                }
            }
            HirExprKind::SafeField { base, field } => {
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                let base_op = self.lower_expr(*base, None, symbols)?;
                if self.current_block.is_none() {
                    return Ok(AmirOperand::Copy(dest));
                }

                let cond_tmp = self.new_temp(ArType::Primitive(Primitive::Bool));
                self.emit_assign_temp(
                    cond_tmp,
                    AmirRvalue::Binary {
                        op: BinaryOp::Equal,
                        left: base_op.clone(),
                        right: AmirOperand::Constant(AmirConstant::Nil),
                    },
                );

                let bb_null = self.new_block();
                let bb_access = self.new_block();
                let bb_join = self.new_block();

                self.set_terminator(AmirTerminator::Branch {
                    condition: AmirOperand::Copy(cond_tmp),
                    if_true: bb_null,
                    if_false: bb_access,
                });

                self.current_block = Some(bb_null);
                self.emit_assign_temp(
                    dest,
                    AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Nil)),
                );
                self.set_terminator(AmirTerminator::Goto(bb_join));

                self.current_block = Some(bb_access);
                let base_expr = self.hir.pool.expr(*base);
                let field_idx = self.resolve_field_index(&base_expr.ty, field);
                self.emit_assign_temp(
                    dest,
                    AmirRvalue::FieldAccess {
                        base: base_op,
                        field: field_idx,
                    },
                );
                self.set_terminator(AmirTerminator::Goto(bb_join));

                self.current_block = Some(bb_join);
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::SafeIndex { base, index } => {
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                let base_op = self.lower_expr(*base, None, symbols)?;
                if self.current_block.is_none() {
                    return Ok(AmirOperand::Copy(dest));
                }

                let cond_tmp = self.new_temp(ArType::Primitive(Primitive::Bool));
                self.emit_assign_temp(
                    cond_tmp,
                    AmirRvalue::Binary {
                        op: BinaryOp::Equal,
                        left: base_op.clone(),
                        right: AmirOperand::Constant(AmirConstant::Nil),
                    },
                );

                let bb_null = self.new_block();
                let bb_access = self.new_block();
                let bb_join = self.new_block();

                self.set_terminator(AmirTerminator::Branch {
                    condition: AmirOperand::Copy(cond_tmp),
                    if_true: bb_null,
                    if_false: bb_access,
                });

                self.current_block = Some(bb_null);
                self.emit_assign_temp(
                    dest,
                    AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Nil)),
                );
                self.set_terminator(AmirTerminator::Goto(bb_join));

                self.current_block = Some(bb_access);
                let index_op = self.lower_expr(*index, None, symbols)?;
                self.emit_assign_temp(
                    dest,
                    AmirRvalue::IndexAccess {
                        base: base_op,
                        index: index_op,
                    },
                );
                if self.current_block.is_some() {
                    self.set_terminator(AmirTerminator::Goto(bb_join));
                }

                self.current_block = Some(bb_join);
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::NullCoalesce { left, right } => {
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                let left_op = self.lower_expr(*left, None, symbols)?;
                if self.current_block.is_none() {
                    return Ok(AmirOperand::Copy(dest));
                }

                let cond_tmp = self.new_temp(ArType::Primitive(Primitive::Bool));
                self.emit_assign_temp(
                    cond_tmp,
                    AmirRvalue::Binary {
                        op: BinaryOp::NotEqual,
                        left: left_op.clone(),
                        right: AmirOperand::Constant(AmirConstant::Nil),
                    },
                );

                let bb_left = self.new_block();
                let bb_right = self.new_block();
                let bb_join = self.new_block();

                self.set_terminator(AmirTerminator::Branch {
                    condition: AmirOperand::Copy(cond_tmp),
                    if_true: bb_left,
                    if_false: bb_right,
                });

                self.current_block = Some(bb_left);
                self.emit_assign_temp(dest, AmirRvalue::Use(left_op));
                self.set_terminator(AmirTerminator::Goto(bb_join));

                self.current_block = Some(bb_right);
                self.lower_expr(*right, Some(dest), symbols)?;
                if self.current_block.is_some() {
                    self.set_terminator(AmirTerminator::Goto(bb_join));
                }

                self.current_block = Some(bb_join);
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::Catch { .. } => Err(amir_unsupported(
                expr.span,
                "`catch` handler",
                "v0.2 CATCH: AMIR catch lowering",
            )),
            HirExprKind::Lambda { .. } => Err(amir_unsupported(
                expr.span,
                "lambda/closure",
                "v0.3 LAMBDA: closure lowering",
            )),
            HirExprKind::AsyncBlock { .. } => Err(amir_unsupported(
                expr.span,
                "async block",
                "v0.3 ASYNC: effect flags and async lowering",
            )),
            HirExprKind::UnsafeBlock { .. } => Err(amir_unsupported(
                expr.span,
                "unsafe block expression",
                "v0.2 UNSAFE: unsafe legality and lowering",
            )),
            HirExprKind::Error => {
                let dest = self.new_temp(ArType::Error);
                Ok(AmirOperand::Copy(dest))
            }
        }
    }
}
