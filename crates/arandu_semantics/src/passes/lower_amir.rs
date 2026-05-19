use crate::amir::*;
use crate::cfg::compute_cfg_edges;
use crate::diagnostics::{DiagCode, Diagnostic, Severity};
use crate::hir::*;
use crate::literal_pool::{AmirLiteralEntry, AmirLiteralPool};
use crate::ops::{BinaryOp, SetOp};
use crate::passes::type_checker::types::{ArType, Primitive};
use crate::{SymbolId, SymbolTable, SymbolKind, TypeCheckResult};
use arandu_lexer::Span;
use smallvec::SmallVec;
use std::collections::HashMap;

pub fn lower_to_amir(
    tc: &TypeCheckResult,
    hir: &HirProgram,
) -> Result<AmirProgram, Vec<Diagnostic>> {
    if tc.diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return Err(tc.diagnostics.clone());
    }

    let mut funcs = Vec::new();
    let mut diagnostics = Vec::new();
    let mut literal_pool = AmirLiteralPool::default();

    for decl in &hir.decls {
        if let HirDecl::Func(
            f @ HirFunc {
                body: Some(body), ..
            },
        ) = decl
        {
            match lower_func(f, body, &tc.symbols, &mut literal_pool) {
                Ok(amir_f) => funcs.push(amir_f),
                Err(diag) => diagnostics.push(diag),
            }
        }
    }

    if diagnostics.is_empty() {
        Ok(AmirProgram {
            funcs,
            literal_pool,
        })
    } else {
        Err(diagnostics)
    }
}

fn amir_unsupported(span: Span, feature: &str) -> Diagnostic {
    Diagnostic::error(
        DiagCode::L002AmirUnsupportedFeature,
        format!("AMIR v0.1: {feature} is not yet supported"),
        span,
    )
}

struct LowerCtx<'a> {
    locals: Vec<AmirLocal>,
    blocks: Vec<AmirBasicBlock>,
    current_block: Option<BlockId>,
    symbol_map: HashMap<SymbolId, LocalId>,
    loop_stack: Vec<(BlockId, BlockId)>, // (continue_block, exit_block)
    literal_pool: &'a mut AmirLiteralPool,
}

impl<'a> LowerCtx<'a> {
    fn next_local_id(&self) -> LocalId {
        LocalId::from_usize(self.locals.len())
    }

    fn intern_literal(&mut self, entry: AmirLiteralEntry) -> AmirConstant {
        AmirConstant::Pool(self.literal_pool.intern(entry))
    }

    fn new_temp(&mut self, ty: ArType) -> LocalId {
        let id = self.next_local_id();
        self.locals.push(AmirLocal {
            id,
            ty,
            symbol: None,
        });
        id
    }

    fn new_local(&mut self, ty: ArType, symbol: SymbolId) -> LocalId {
        let id = self.next_local_id();
        self.locals.push(AmirLocal {
            id,
            ty,
            symbol: Some(symbol),
        });
        self.symbol_map.insert(symbol, id);
        id
    }

    fn new_block(&mut self) -> BlockId {
        let id = BlockId::from_usize(self.blocks.len());
        self.blocks.push(AmirBasicBlock {
            id,
            statements: Vec::new(),
            terminator: AmirTerminator::Unreachable,
            successors: Vec::new(),
            predecessors: Vec::new(),
        });
        id
    }

    fn push_stmt(&mut self, stmt: AmirStmt) {
        if let Some(curr) = self.current_block {
            self.blocks[curr.as_usize()].statements.push(stmt);
        }
    }

    fn set_terminator(&mut self, term: AmirTerminator) {
        if let Some(curr) = self.current_block {
            self.blocks[curr.as_usize()].terminator = term;
        }
    }

    fn set_bool_branch(&mut self, condition: AmirOperand, if_true: BlockId, if_false: BlockId) {
        self.set_terminator(AmirTerminator::Branch {
            condition,
            if_true,
            if_false,
        });
    }

    fn emit_assign_local(&mut self, local: LocalId, rhs: AmirRvalue) {
        self.push_stmt(AmirStmt::Assign {
            lhs: AmirPlace {
                local,
                projections: SmallVec::new(),
            },
            rhs,
        });
    }

    fn emit_assign_place(&mut self, lhs: AmirPlace, rhs: AmirRvalue) {
        self.push_stmt(AmirStmt::Assign { lhs, rhs });
    }

    fn load_place(
        &mut self,
        place: &AmirPlace,
        ty: ArType,
        projection_types: &[ArType],
    ) -> AmirOperand {
        if place.projections.is_empty() {
            AmirOperand::Copy(place.local)
        } else {
            let mut current = AmirOperand::Copy(place.local);
            for (i, proj) in place.projections.iter().enumerate() {
                let step_ty = projection_types.get(i).cloned().unwrap_or_else(|| {
                    if i == place.projections.len() - 1 {
                        ty.clone()
                    } else {
                        ArType::Error
                    }
                });
                let temp = self.new_temp(step_ty);
                match proj {
                    AmirProjection::Field(name) => {
                        self.emit_assign_local(
                            temp,
                            AmirRvalue::FieldAccess {
                                base: current.clone(),
                                field: name.clone(),
                            },
                        );
                    }
                    AmirProjection::Index(idx_op) => {
                        self.emit_assign_local(
                            temp,
                            AmirRvalue::IndexAccess {
                                base: current.clone(),
                                index: idx_op.clone(),
                            },
                        );
                    }
                }
                current = AmirOperand::Copy(temp);
            }
            current
        }
    }

    fn lower_condition(
        &mut self,
        cond: &HirCondition,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        match cond {
            HirCondition::Expr(expr) => self.lower_expr(expr, None, symbols),
            HirCondition::Is { expr, .. } => {
                let expr_op = self.lower_expr(expr, None, symbols)?;
                let dest = self.new_temp(ArType::Primitive(Primitive::Bool));
                self.emit_assign_local(dest, AmirRvalue::Use(expr_op));
                Ok(AmirOperand::Copy(dest))
            }
        }
    }

    fn lower_expr(
        &mut self,
        expr: &HirExpr,
        target: Option<LocalId>,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        match &expr.kind {
            HirExprKind::Int(v) => {
                let op =
                    AmirOperand::Constant(self.intern_literal(AmirLiteralEntry::Int(v.clone())));
                if let Some(dest) = target {
                    self.emit_assign_local(dest, AmirRvalue::Use(op.clone()));
                }
                Ok(op)
            }
            HirExprKind::Float(v) => {
                let op =
                    AmirOperand::Constant(self.intern_literal(AmirLiteralEntry::Float(v.clone())));
                if let Some(dest) = target {
                    self.emit_assign_local(dest, AmirRvalue::Use(op.clone()));
                }
                Ok(op)
            }
            HirExprKind::Bool(v) => {
                let op = AmirOperand::Constant(AmirConstant::Bool(*v));
                if let Some(dest) = target {
                    self.emit_assign_local(dest, AmirRvalue::Use(op.clone()));
                }
                Ok(op)
            }
            HirExprKind::Str(v) => {
                let op =
                    AmirOperand::Constant(self.intern_literal(AmirLiteralEntry::Str(v.clone())));
                if let Some(dest) = target {
                    self.emit_assign_local(dest, AmirRvalue::Use(op.clone()));
                }
                Ok(op)
            }
            HirExprKind::Char(v) => {
                let op =
                    AmirOperand::Constant(self.intern_literal(AmirLiteralEntry::Char(v.clone())));
                if let Some(dest) = target {
                    self.emit_assign_local(dest, AmirRvalue::Use(op.clone()));
                }
                Ok(op)
            }
            HirExprKind::Nil => {
                let op = AmirOperand::Constant(AmirConstant::Nil);
                if let Some(dest) = target {
                    self.emit_assign_local(dest, AmirRvalue::Use(op.clone()));
                }
                Ok(op)
            }
            HirExprKind::Path { symbol } => {
                let op = if let Some(&local_id) = self.symbol_map.get(symbol) {
                    AmirOperand::Copy(local_id)
                } else {
                    let sym = symbols.get(*symbol);
                    match sym.kind {
                        SymbolKind::Func | SymbolKind::ExternFunc | SymbolKind::AssociatedFunc | SymbolKind::NamespaceMember => {
                            AmirOperand::FunctionRef(*symbol)
                        }
                        _ => AmirOperand::GlobalRef(*symbol),
                    }
                };
                if let Some(dest) = target {
                    self.emit_assign_local(dest, AmirRvalue::Use(op.clone()));
                }
                Ok(op)
            }
            HirExprKind::TypePath { member_symbol, .. } => {
                let op = if let Some(&local_id) = self.symbol_map.get(member_symbol) {
                    AmirOperand::Copy(local_id)
                } else {
                    AmirOperand::GlobalRef(*member_symbol)
                };
                if let Some(dest) = target {
                    self.emit_assign_local(dest, AmirRvalue::Use(op.clone()));
                }
                Ok(op)
            }
            HirExprKind::Generic { callee, .. } => self.lower_expr(callee, target, symbols),
            HirExprKind::Alloc { .. } => {
                return Err(amir_unsupported(expr.span, "`alloc` expression"));
            }
            HirExprKind::Binary { op, left, right } => {
                let l_op = self.lower_expr(left, None, symbols)?;
                let r_op = self.lower_expr(right, None, symbols)?;
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                self.emit_assign_local(
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
                let sub_op = self.lower_expr(sub_expr, None, symbols)?;
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                self.emit_assign_local(
                    dest,
                    AmirRvalue::Unary {
                        op: *op,
                        operand: sub_op,
                    },
                );
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::Field { base, field } => {
                let base_op = self.lower_expr(base, None, symbols)?;
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                self.emit_assign_local(
                    dest,
                    AmirRvalue::FieldAccess {
                        base: base_op,
                        field: field.clone(),
                    },
                );
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::Index { base, index } => {
                let base_op = self.lower_expr(base, None, symbols)?;
                let idx_op = self.lower_expr(index, None, symbols)?;
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                self.emit_assign_local(
                    dest,
                    AmirRvalue::IndexAccess {
                        base: base_op,
                        index: idx_op,
                    },
                );
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::Array { items } => {
                let mut item_ops = Vec::new();
                for item in items {
                    item_ops.push(self.lower_expr(item, None, symbols)?);
                }
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                self.emit_assign_local(dest, AmirRvalue::Array { items: item_ops });
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::Call { callee, args, .. } => {
                let callee_op = self.lower_expr(callee, None, symbols)?;
                let mut arg_ops = Vec::new();
                for arg in args {
                    arg_ops.push(self.lower_expr(arg, None, symbols)?);
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
                Ok(dest
                    .map(AmirOperand::Copy)
                    .unwrap_or(AmirOperand::Constant(AmirConstant::Nil)))
            }
            HirExprKind::StructLiteral {
                struct_symbol,
                fields,
            } => {
                let mut field_ops = Vec::new();
                for f in fields {
                    field_ops.push((f.name.clone(), self.lower_expr(&f.value, None, symbols)?));
                }
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                self.emit_assign_local(
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
                let bb_then = self.new_block();
                let bb_else = self.new_block();
                let bb_join = self.new_block();

                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));

                self.set_bool_branch(cond_op, bb_then, bb_else);

                // Then branch
                self.current_block = Some(bb_then);
                self.lower_block_as_expr(then_block, Some(dest), symbols)?;
                if self.current_block.is_some() {
                    self.set_terminator(AmirTerminator::Goto(bb_join));
                }

                // Else branch
                self.current_block = Some(bb_else);
                self.lower_block_as_expr(else_block, Some(dest), symbols)?;
                if self.current_block.is_some() {
                    self.set_terminator(AmirTerminator::Goto(bb_join));
                }

                // Join
                self.current_block = Some(bb_join);
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::Cast { expr: sub_expr, .. } => {
                let sub_op = self.lower_expr(sub_expr, None, symbols)?;
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                self.emit_assign_local(dest, AmirRvalue::Use(sub_op));
                Ok(AmirOperand::Copy(dest))
            }

            // --- Unsupported in AMIR v0.1: require CFG desugaring ---
            HirExprKind::Match { .. } => Err(amir_unsupported(expr.span, "match expression")),
            HirExprKind::Try { .. } => Err(amir_unsupported(expr.span, "`?` operator")),
            HirExprKind::SafeField { .. } => {
                Err(amir_unsupported(expr.span, "`?.` safe field access"))
            }
            HirExprKind::SafeIndex { .. } => {
                Err(amir_unsupported(expr.span, "`?[]` safe index access"))
            }
            HirExprKind::NullCoalesce { .. } => {
                Err(amir_unsupported(expr.span, "`??` null coalescing"))
            }
            HirExprKind::Catch { .. } => Err(amir_unsupported(expr.span, "`catch` handler")),
            HirExprKind::Lambda { .. } => Err(amir_unsupported(expr.span, "lambda/closure")),
            HirExprKind::AsyncBlock { .. } => Err(amir_unsupported(expr.span, "async block")),
            HirExprKind::UnsafeBlock { .. } => {
                Err(amir_unsupported(expr.span, "unsafe block expression"))
            }
        }
    }

    fn lower_stmt(&mut self, stmt: &HirStmt, symbols: &SymbolTable) -> Result<(), Diagnostic> {
        if self.current_block.is_none() {
            return Ok(());
        }

        match &stmt.kind {
            HirStmtKind::VarDecl { bindings, value } => {
                if bindings.len() == 1 {
                    let b = &bindings[0];
                    let local_id = self.new_local(b.ty.clone(), b.symbol);
                    self.lower_expr(value, Some(local_id), symbols)?;
                } else {
                    let val_op = self.lower_expr(value, None, symbols)?;
                    for (i, b) in bindings.iter().enumerate() {
                        let local_id = self.new_local(b.ty.clone(), b.symbol);
                        self.emit_assign_local(
                            local_id,
                            AmirRvalue::FieldAccess {
                                base: val_op.clone(),
                                field: format!("_{}", i),
                            },
                        );
                    }
                }
            }
            HirStmtKind::Set { places, op, value } => {
                let val_op = self.lower_expr(value, None, symbols)?;
                self.lower_set_places(places, op, &val_op, symbols)?;
            }
            HirStmtKind::Return { values } => {
                if values.is_empty() {
                    self.set_terminator(AmirTerminator::Return);
                } else if values.len() == 1 {
                    self.lower_expr(&values[0], Some(LocalId(0)), symbols)?;
                    self.set_terminator(AmirTerminator::Return);
                } else {
                    let mut ops = Vec::new();
                    for v in values {
                        ops.push(self.lower_expr(v, None, symbols)?);
                    }
                    self.emit_assign_local(LocalId(0), AmirRvalue::Tuple { items: ops });
                    self.set_terminator(AmirTerminator::Return);
                }
                self.current_block = None;
            }
            HirStmtKind::Break => {
                if let Some((_, exit_block)) = self.loop_stack.last() {
                    self.set_terminator(AmirTerminator::Goto(*exit_block));
                    self.current_block = None;
                }
            }
            HirStmtKind::Continue => {
                if let Some((cont_block, _)) = self.loop_stack.last() {
                    self.set_terminator(AmirTerminator::Goto(*cont_block));
                    self.current_block = None;
                }
            }
            HirStmtKind::Expr(expr) => {
                self.lower_expr(expr, None, symbols)?;
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
                self.lower_block(then_block, symbols)?;
                if self.current_block.is_some() {
                    self.set_terminator(AmirTerminator::Goto(bb_join));
                }

                // Else
                self.current_block = Some(bb_else);
                if let Some(eb) = else_block {
                    self.lower_block(eb, symbols)?;
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

                self.loop_stack.push((bb_cond, bb_exit));
                self.current_block = Some(bb_body);
                self.lower_block(body, symbols)?;
                if self.current_block.is_some() {
                    self.set_terminator(AmirTerminator::Goto(bb_cond));
                }
                self.loop_stack.pop();

                self.current_block = Some(bb_exit);
            }
            HirStmtKind::For { clause, body } => match clause {
                HirForClause::In { span, .. } => {
                    return Err(amir_unsupported(*span, "`for x in iterable` loop"));
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
                        self.lower_expr(c, None, symbols)?
                    } else {
                        AmirOperand::Constant(AmirConstant::Bool(true))
                    };
                    self.set_bool_branch(cond_op, bb_body, bb_exit);

                    self.loop_stack.push((bb_step, bb_exit));
                    self.current_block = Some(bb_body);
                    self.lower_block(body, symbols)?;
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
            HirStmtKind::Match { value, .. } => {
                return Err(amir_unsupported(value.span, "match statement"));
            }
            HirStmtKind::Unsafe(b) => {
                self.lower_block(b, symbols)?;
            }
            HirStmtKind::Defer(_) => {
                return Err(amir_unsupported(stmt.span, "`defer`"));
            }
            HirStmtKind::ErrDefer(_) => {
                return Err(amir_unsupported(stmt.span, "`errdefer`"));
            }
            HirStmtKind::Free(_) => {
                return Err(amir_unsupported(stmt.span, "`free` statement"));
            }
        }
        Ok(())
    }

    fn lower_simple_stmt(
        &mut self,
        stmt: &HirSimpleStmt,
        symbols: &SymbolTable,
    ) -> Result<(), Diagnostic> {
        if self.current_block.is_none() {
            return Ok(());
        }
        match stmt {
            HirSimpleStmt::VarDecl { bindings, value } => {
                if bindings.len() == 1 {
                    let b = &bindings[0];
                    let local_id = self.new_local(b.ty.clone(), b.symbol);
                    self.lower_expr(value, Some(local_id), symbols)?;
                } else {
                    let val_op = self.lower_expr(value, None, symbols)?;
                    for (i, b) in bindings.iter().enumerate() {
                        let local_id = self.new_local(b.ty.clone(), b.symbol);
                        self.emit_assign_local(
                            local_id,
                            AmirRvalue::FieldAccess {
                                base: val_op.clone(),
                                field: format!("_{}", i),
                            },
                        );
                    }
                }
            }
            HirSimpleStmt::Set { places, op, value } => {
                let val_op = self.lower_expr(value, None, symbols)?;
                self.lower_set_places(places, op, &val_op, symbols)?;
            }
            HirSimpleStmt::Expr(expr) => {
                self.lower_expr(expr, None, symbols)?;
            }
        }
        Ok(())
    }

    fn lower_set_places(
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
                        HirPlaceSuffix::Field { ty, .. }
                        | HirPlaceSuffix::Index { ty, .. } => ty.clone(),
                    })
                    .collect();
                let projections: Result<Vec<_>, Diagnostic> = place
                    .suffixes
                    .iter()
                    .map(|s| match s {
                        HirPlaceSuffix::Field { name, .. } => {
                            Ok(AmirProjection::Field(name.clone()))
                        }
                        HirPlaceSuffix::Index { expr, .. } => {
                            Ok(AmirProjection::Index(self.lower_expr(expr, None, symbols)?))
                        }
                    })
                    .collect();
                let amir_place = AmirPlace {
                    local: local_id,
                    projections: projections?.into(),
                };

                if *op == SetOp::Assign {
                    self.emit_assign_place(amir_place, AmirRvalue::Use(val_op.clone()));
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
                        self.load_place(&amir_place, place.ty.clone(), &projection_types);
                    let temp = self.new_temp(place.ty.clone());
                    self.emit_assign_local(
                        temp,
                        AmirRvalue::Binary {
                            op: bin_op,
                            left: old_val,
                            right: val_op.clone(),
                        },
                    );
                    self.emit_assign_place(
                        amir_place,
                        AmirRvalue::Use(AmirOperand::Copy(temp)),
                    );
                }
            }
        } else {
            for (i, place) in places.iter().enumerate() {
                if let Some(&local_id) = self.symbol_map.get(&place.root_symbol) {
                    let projections: Result<Vec<_>, Diagnostic> = place
                        .suffixes
                        .iter()
                        .map(|s| match s {
                            HirPlaceSuffix::Field { name, .. } => {
                                Ok(AmirProjection::Field(name.clone()))
                            }
                            HirPlaceSuffix::Index { expr, .. } => {
                                Ok(AmirProjection::Index(self.lower_expr(expr, None, symbols)?))
                            }
                        })
                        .collect();
                    let amir_place = AmirPlace {
                        local: local_id,
                        projections: projections?.into(),
                    };

                    let temp_ty = place.ty.clone();
                    let temp = self.new_temp(temp_ty);
                    self.emit_assign_local(
                        temp,
                        AmirRvalue::FieldAccess {
                            base: val_op.clone(),
                            field: format!("_{}", i),
                        },
                    );
                    self.emit_assign_place(
                        amir_place,
                        AmirRvalue::Use(AmirOperand::Copy(temp)),
                    );
                }
            }
        }
        Ok(())
    }

    fn lower_block(&mut self, block: &HirBlock, symbols: &SymbolTable) -> Result<(), Diagnostic> {
        for stmt in &block.statements {
            self.lower_stmt(stmt, symbols)?;
        }
        Ok(())
    }

    /// Lower a block as an expression: all statements are lowered, and if the
    /// last statement is an expression statement, its value is assigned to `target`.
    /// This ensures preceding statements (e.g. `y = 10`) are not skipped.
    fn lower_block_as_expr(
        &mut self,
        block: &HirBlock,
        target: Option<LocalId>,
        symbols: &SymbolTable,
    ) -> Result<(), Diagnostic> {
        if block.statements.is_empty() {
            if let Some(dest) = target {
                self.emit_assign_local(dest, AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Nil)));
            }
            return Ok(());
        }
        let last_idx = block.statements.len() - 1;
        for (i, stmt) in block.statements.iter().enumerate() {
            if i == last_idx {
                if let HirStmtKind::Expr(ref expr) = stmt.kind {
                    self.lower_expr(expr, target, symbols)?;
                } else {
                    self.lower_stmt(stmt, symbols)?;
                    if let Some(dest) = target {
                        self.emit_assign_local(dest, AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Nil)));
                    }
                }
            } else {
                self.lower_stmt(stmt, symbols)?;
            }
        }
        Ok(())
    }
}

fn lower_func(
    f: &HirFunc,
    body: &HirBlock,
    symbols: &SymbolTable,
    literal_pool: &mut AmirLiteralPool,
) -> Result<AmirFunc, Diagnostic> {
    let mut ctx = LowerCtx {
        locals: Vec::new(),
        blocks: Vec::new(),
        current_block: None,
        symbol_map: HashMap::new(),
        loop_stack: Vec::new(),
        literal_pool,
    };

    // Return register is _0
    ctx.locals.push(AmirLocal {
        id: LocalId(0),
        ty: f.return_type.clone(),
        symbol: None,
    });

    let mut params = Vec::new();
    for param in &f.params {
        let p_id = ctx.new_local(param.ty.clone(), param.symbol);
        params.push(p_id);
    }

    let bb0 = ctx.new_block();
    ctx.current_block = Some(bb0);

    ctx.lower_block(body, symbols)?;

    // If last block does not have a terminator, implicitly return
    if let Some(curr) = ctx.current_block {
        ctx.blocks[curr.as_usize()].terminator = AmirTerminator::Return;
    }

    compute_cfg_edges(&mut ctx.blocks);

    Ok(AmirFunc {
        symbol: f.symbol,
        return_type: f.return_type.clone(),
        params,
        locals: ctx.locals,
        blocks: ctx.blocks,
    })
}
