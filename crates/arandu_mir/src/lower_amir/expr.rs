use super::{LowerCtx, amir_unsupported};
use crate::amir::{AmirConstant, AmirOperand, AmirRvalue, AmirStmt, AmirTerminator, TempId};
use crate::diagnostics::{DiagCode, Diagnostic};
use crate::literal_pool::AmirLiteralEntry;
use crate::ops::BinaryOp;
use crate::passes::type_checker::types::{ArType, Primitive, is_option_type, result_ok_err};
use crate::{SymbolKind, SymbolTable};

use crate::hir::{HirExpr, HirExprId, HirExprKind, ResultCtorVariant};

fn resolve_method_target(
    callee: &HirExpr,
    pool: &crate::hir::HirPool,
    symbols: &SymbolTable,
    interner: &crate::types::TypeInterner,
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
            let inner_ty = interner.resolve(*inner);
            match inner_ty {
                ArType::Named(id, _) => Some(id),
                ArType::Ptr(ptr_inner) => {
                    let ptr_inner_ty = interner.resolve(ptr_inner);
                    match ptr_inner_ty {
                        ArType::Named(id, _) => Some(id),
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        ArType::Named(id, _) => Some(*id),
        ArType::Ptr(inner) => {
            let inner_ty = interner.resolve(*inner);
            match inner_ty {
                ArType::Named(id, _) => Some(id),
                _ => None,
            }
        }
        _ => None,
    }?;

    let struct_name = symbols.get(struct_id).name.clone();
    symbols.lookup_associated_member(&struct_name, field)
}

impl LowerCtx<'_> {
    /// If `src_ty` is formatable and not already `str`, emit `ToStr` into a
    /// fresh `str` temp. Identity for `str`; other types left unchanged (typeck
    /// should already have rejected non-formatables at str sites).
    pub(crate) fn maybe_to_str(
        &mut self,
        op: AmirOperand,
        src_ty: &ArType,
    ) -> Result<AmirOperand, Diagnostic> {
        if matches!(src_ty, ArType::Primitive(Primitive::Str)) || src_ty.is_error() {
            return Ok(op);
        }
        if !src_ty.is_to_str_v01() {
            return Ok(op);
        }
        let src_ty_id = self.intern_ty(src_ty.clone());
        let dest = self.new_temp(ArType::Primitive(Primitive::Str));
        self.emit_assign_temp(
            dest,
            AmirRvalue::ToStr {
                value: op,
                src_ty: src_ty_id,
            },
        );
        Ok(AmirOperand::Copy(dest))
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
        if result_ok_err(&self.func_return_type, &self.tc.type_info.type_interner).is_none() {
            return false;
        }
        match values.len() {
            0 => false,
            1 => self.expr_is_result_err(self.hir.pool.expr(values[0])),
            _ => false,
        }
    }

    pub(crate) fn lower_result_ok_field(&mut self, base: AmirOperand, dest: TempId) {
        self.emit_assign_temp(dest, AmirRvalue::FieldAccess { base, field: 1 });
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
        let ok_err = result_ok_err(&inner.ty, &self.tc.type_info.type_interner);
        let (_, err_ty) = match ok_err {
            Some(tup) => tup,
            None => {
                if matches!(inner.ty, ArType::Error) {
                    // Tipo Error = diagnóstico já emitido antes (type checker já rejeitou).
                    // NÃO é bug interno — bail silenciosamente com um valor poison,
                    // sem duplicar erro nem gerar ICE.
                    let dest = target.unwrap_or_else(|| self.new_temp(ArType::Error));
                    return Ok(AmirOperand::Copy(dest));
                }
                return Err(Diagnostic::ice(
                    DiagCode::ICEGEN001,
                    "try_result: tipo não-Error mas result_ok_err falhou",
                    inner.span,
                ));
            }
        };
        let base = self.lower_expr(inner_id, None, symbols)?;
        if self.current_block.is_none() {
            let dest = target.unwrap_or_else(|| self.new_temp(expr_ty));
            return Ok(AmirOperand::Copy(dest));
        }

        let err_tmp = self.new_temp(err_ty.clone());
        self.lower_result_err_field(base, err_ty, err_tmp);

        let tag_tmp = self.new_temp(ArType::Primitive(Primitive::Int));
        self.emit_assign_temp(
            tag_tmp,
            AmirRvalue::Discriminant {
                value: base,
            },
        );

        let one_lit = self.intern_literal(AmirLiteralEntry::Int("1".to_string()));
        let cond_tmp = self.new_temp(ArType::Primitive(Primitive::Bool));
        self.emit_assign_temp(
            cond_tmp,
            AmirRvalue::Binary {
                op: BinaryOp::Equal,
                left: AmirOperand::Copy(tag_tmp),
                right: AmirOperand::Constant(one_lit),
            },
        );

        let bb_return_err = self.new_block();
        let bb_continue = self.new_block();

        self.set_bool_branch(AmirOperand::Copy(cond_tmp), bb_return_err, bb_continue);
        self.seal_block(bb_return_err);
        self.seal_block(bb_continue);

        self.current_block = Some(bb_return_err);
        self.exit_all_defer_frames(true, symbols)?;
        let err_ctor_tmp = self.new_temp(self.func_return_type.clone());
        self.emit_assign_temp(
            err_ctor_tmp,
            AmirRvalue::EnumConstruct {
                variant_tag: 1,
                payload: Some(AmirOperand::Copy(err_tmp)),
            },
        );
        self.emit_assign_temp(TempId(0), AmirRvalue::Use(AmirOperand::Copy(err_ctor_tmp)));
        self.set_terminator(AmirTerminator::Return);

        self.current_block = None;

        self.current_block = Some(bb_continue);
        let dest = target.unwrap_or_else(|| self.new_temp(expr_ty));
        self.lower_result_ok_field(base, dest);
        Ok(AmirOperand::Copy(dest))
    }

    /// Lower `expr catch handler` — like `?` but recover with handler instead of return.
    pub(crate) fn lower_catch(
        &mut self,
        inner_id: HirExprId,
        handler: &crate::hir::HirCatchHandler,
        target: Option<TempId>,
        expr_ty: ArType,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        use crate::hir::HirCatchHandler;

        let inner = self.hir.pool.expr(inner_id);
        let ok_err = result_ok_err(&inner.ty, &self.tc.type_info.type_interner);
        let (ok_ty, err_ty) = match ok_err {
            Some(tup) => tup,
            None => {
                if matches!(inner.ty, ArType::Error) {
                    let dest = target.unwrap_or_else(|| self.new_temp(ArType::Error));
                    return Ok(AmirOperand::Copy(dest));
                }
                return Err(Diagnostic::ice(
                    DiagCode::ICEGEN001,
                    "catch: expected Result type",
                    inner.span,
                ));
            }
        };
        let _ = ok_ty;

        let base = self.lower_expr(inner_id, None, symbols)?;
        if self.current_block.is_none() {
            let dest = target.unwrap_or_else(|| self.new_temp(expr_ty));
            return Ok(AmirOperand::Copy(dest));
        }

        let dest = target.unwrap_or_else(|| self.new_temp(expr_ty.clone()));

        let tag_tmp = self.new_temp(ArType::Primitive(Primitive::Int));
        self.emit_assign_temp(
            tag_tmp,
            AmirRvalue::Discriminant {
                value: base,
            },
        );

        let one_lit = self.intern_literal(AmirLiteralEntry::Int("1".to_string()));
        let is_err = self.new_temp(ArType::Primitive(Primitive::Bool));
        self.emit_assign_temp(
            is_err,
            AmirRvalue::Binary {
                op: BinaryOp::Equal,
                left: AmirOperand::Copy(tag_tmp),
                right: AmirOperand::Constant(one_lit),
            },
        );

        let bb_err = self.new_block();
        let bb_ok = self.new_block();
        let bb_join = self.new_block();

        self.set_bool_branch(AmirOperand::Copy(is_err), bb_err, bb_ok);
        self.seal_block(bb_err);
        self.seal_block(bb_ok);

        // Err arm: evaluate handler (optionally bind error payload).
        self.current_block = Some(bb_err);
        if let HirCatchHandler::Block {
            error_symbol: Some(err_sym),
            ..
        } = handler
        {
            let err_tmp = self.new_temp(err_ty.clone());
            self.lower_result_err_field(base, err_ty.clone(), err_tmp);
            let err_local = self.new_local(err_ty.clone(), *err_sym, inner.span);
            let consumed = self.consume_operand(AmirOperand::Copy(err_tmp))?;
            self.write_variable_source(err_local, consumed)?;
        }
        match handler {
            HirCatchHandler::Expr(h_expr) => {
                self.lower_expr(*h_expr, Some(dest), symbols)?;
            }
            HirCatchHandler::Block { block, .. } => {
                self.lower_block_as_expr(*block, Some(dest), symbols)?;
            }
        }
        if self.current_block.is_some() {
            self.emit_goto(bb_join);
        }

        // Ok arm: unwrap payload.
        self.current_block = Some(bb_ok);
        self.lower_result_ok_field(base, dest);
        self.emit_goto(bb_join);

        self.seal_block(bb_join);
        self.current_block = Some(bb_join);
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
                left: base,
                right: AmirOperand::Constant(AmirConstant::Nil),
            },
        );

        let bb_return_nil = self.new_block();
        let bb_continue = self.new_block();

        self.set_bool_branch(AmirOperand::Copy(cond_tmp), bb_return_nil, bb_continue);
        self.seal_block(bb_return_nil);
        self.seal_block(bb_continue);

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
        self.current_span = expr.span;
        match &expr.kind {
            HirExprKind::Int(v) => {
                let op =
                    AmirOperand::Constant(self.intern_literal(AmirLiteralEntry::Int(v.clone())));
                if let Some(dest) = target {
                    self.emit_assign_temp(dest, AmirRvalue::Use(op));
                }
                Ok(op)
            }
            HirExprKind::Float(v) => {
                let op =
                    AmirOperand::Constant(self.intern_literal(AmirLiteralEntry::Float(v.clone())));
                if let Some(dest) = target {
                    self.emit_assign_temp(dest, AmirRvalue::Use(op));
                }
                Ok(op)
            }
            HirExprKind::Bool(v) => {
                let op = AmirOperand::Constant(AmirConstant::Bool(*v));
                if let Some(dest) = target {
                    self.emit_assign_temp(dest, AmirRvalue::Use(op));
                }
                Ok(op)
            }
            HirExprKind::Str(v) => {
                let op =
                    AmirOperand::Constant(self.intern_literal(AmirLiteralEntry::Str(v.clone())));
                if let Some(dest) = target {
                    self.emit_assign_temp(dest, AmirRvalue::Use(op));
                }
                Ok(op)
            }
            HirExprKind::StringInterp { parts } => {
                let mut part_ops = Vec::with_capacity(parts.len());
                for part in parts {
                    let op = match part {
                        arandu_middle::hir::HirStringPart::Text(t) => AmirOperand::Constant(
                            self.intern_literal(AmirLiteralEntry::Str(t.clone())),
                        ),
                        arandu_middle::hir::HirStringPart::Expr(e) => {
                            let part_expr = self.hir.pool.expr(*e);
                            let part_op = self.lower_expr(*e, None, symbols)?;
                            self.maybe_to_str(part_op, &part_expr.ty)?
                        }
                    };
                    part_ops.push(op);
                }
                let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                self.emit_assign_temp(dest, AmirRvalue::StringInterp { parts: part_ops });
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::ToStr { value } => {
                let value_expr = self.hir.pool.expr(*value);
                let op = self.lower_expr(*value, None, symbols)?;
                // Always materialize ToStr (even for `str` identity is a no-op in maybe_to_str).
                let str_op = self.maybe_to_str(op, &value_expr.ty)?;
                if let Some(dest) = target {
                    self.emit_assign_temp(dest, AmirRvalue::Use(str_op.clone()));
                    Ok(AmirOperand::Copy(dest))
                } else {
                    Ok(str_op)
                }
            }
            HirExprKind::Char(v) => {
                let op =
                    AmirOperand::Constant(self.intern_literal(AmirLiteralEntry::Char(v.clone())));
                if let Some(dest) = target {
                    self.emit_assign_temp(dest, AmirRvalue::Use(op));
                }
                Ok(op)
            }
            HirExprKind::Nil => {
                let op = if is_option_type(&expr.ty) {
                    let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                    self.emit_assign_temp(
                        dest,
                        AmirRvalue::EnumConstruct {
                            variant_tag: 0,
                            payload: None,
                        },
                    );
                    AmirOperand::Copy(dest)
                } else {
                    AmirOperand::Constant(AmirConstant::Nil)
                };
                if let (Some(dest), false) = (target, is_option_type(&expr.ty)) {
                    self.emit_assign_temp(dest, AmirRvalue::Use(op));
                }
                Ok(op)
            }
            HirExprKind::Path { symbol } => {
                // Derive the parent enum SymbolId from the expression's resolved type.
                // The type checker always resolves an enum-variant expression to
                // ArType::Named(enum_sym, []), so we can use that as a filter anchor
                // instead of doing a global name-based scan — which would silently
                // pick the wrong discriminant when two enums share a variant name.
                let enum_sym_from_ty = match &expr.ty {
                    ArType::Named(id, _) => Some(*id),
                    _ => None,
                };
                let op: AmirOperand = if let Some(&local_id) = self.symbol_map.get(symbol) {
                    Ok::<AmirOperand, Diagnostic>(self.read_variable_source(local_id)?)
                } else if let Some(&tag) =
                    self.tc.type_info.enum_variant_tags.get(symbol).or_else(|| {
                        // Fallback: find the canonical variant SymbolId whose parent enum
                        // matches the type we already know this expression has, then look up
                        // its tag. No string comparison needed — anchored by SymbolId.
                        let enum_id = enum_sym_from_ty?;
                        self.tc
                            .type_info
                            .enum_variants
                            .iter()
                            .find(|&(v_sym, (parent_sym, _))| {
                                *parent_sym == enum_id
                                    && self.tc.type_info.enum_variant_tags.contains_key(v_sym)
                                    && {
                                        // Name must match (bare suffix of the lookup symbol vs
                                        // bare suffix of the registered variant symbol).
                                        let lookup_bare = symbols
                                            .get(*symbol)
                                            .name
                                            .rsplit('.')
                                            .next()
                                            .unwrap_or("");
                                        let reg_bare = symbols
                                            .get(*v_sym)
                                            .name
                                            .rsplit('.')
                                            .next()
                                            .unwrap_or("");
                                        lookup_bare == reg_bare
                                    }
                            })
                            .and_then(|(v_sym, _)| self.tc.type_info.enum_variant_tags.get(v_sym))
                    })
                {
                    let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                    self.emit_assign_temp(
                        dest,
                        AmirRvalue::EnumConstruct {
                            variant_tag: tag,
                            payload: None,
                        },
                    );
                    Ok(AmirOperand::Copy(dest))
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
                    let already_assigned = self.tc.type_info.enum_variant_tags.contains_key(symbol)
                        || enum_sym_from_ty.is_some_and(|enum_id| {
                            let lookup_bare =
                                symbols.get(*symbol).name.rsplit('.').next().unwrap_or("");
                            self.tc
                                .type_info
                                .enum_variants
                                .iter()
                                .any(|(v_sym, (parent, _))| {
                                    *parent == enum_id
                                        && symbols.get(*v_sym).name.rsplit('.').next().unwrap_or("")
                                            == lookup_bare
                                        && self.tc.type_info.enum_variant_tags.contains_key(v_sym)
                                })
                        });
                    if !already_assigned {
                        let rhs = self.consume_operand(op)?;
                        self.emit_assign_temp(dest, AmirRvalue::Use(rhs));
                    }
                }
                Ok(op)
            }
            HirExprKind::TypePath {
                type_symbol,
                member_symbol,
            } => {
                let op: AmirOperand = if let Some(&local_id) = self.symbol_map.get(member_symbol) {
                    Ok::<AmirOperand, Diagnostic>(self.read_variable_source(local_id)?)
                } else if let Some(&tag) = self
                    .tc
                    .type_info
                    .enum_variant_tags
                    .get(member_symbol)
                    .or_else(|| {
                        // Filter by the enum type that the parser already resolved (type_symbol).
                        // This eliminates cross-enum collisions for identically-named variants.
                        let lookup_bare = symbols
                            .get(*member_symbol)
                            .name
                            .rsplit('.')
                            .next()
                            .unwrap_or("");
                        self.tc
                            .type_info
                            .enum_variants
                            .iter()
                            .find(|&(v_sym, (parent_sym, _))| {
                                *parent_sym == *type_symbol
                                    && symbols.get(*v_sym).name.rsplit('.').next().unwrap_or("")
                                        == lookup_bare
                                    && self.tc.type_info.enum_variant_tags.contains_key(v_sym)
                            })
                            .and_then(|(v_sym, _)| self.tc.type_info.enum_variant_tags.get(v_sym))
                    })
                {
                    let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                    self.emit_assign_temp(
                        dest,
                        AmirRvalue::EnumConstruct {
                            variant_tag: tag,
                            payload: None,
                        },
                    );
                    Ok(AmirOperand::Copy(dest))
                } else {
                    Ok(AmirOperand::GlobalRef(*member_symbol))
                }?;
                if let Some(dest) = target {
                    let lookup_bare = symbols
                        .get(*member_symbol)
                        .name
                        .rsplit('.')
                        .next()
                        .unwrap_or("");
                    let already_assigned =
                        self.tc
                            .type_info
                            .enum_variant_tags
                            .contains_key(member_symbol)
                            || self.tc.type_info.enum_variants.iter().any(
                                |(v_sym, (parent, _))| {
                                    *parent == *type_symbol
                                        && symbols.get(*v_sym).name.rsplit('.').next().unwrap_or("")
                                            == lookup_bare
                                        && self.tc.type_info.enum_variant_tags.contains_key(v_sym)
                                },
                            );
                    if !already_assigned {
                        let rhs = self.consume_operand(op)?;
                        self.emit_assign_temp(dest, AmirRvalue::Use(rhs));
                    }
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
                self.lower_binary(*op, *left, *right, expr.ty.clone(), target, symbols)
            }
            HirExprKind::Unary { op, expr: sub_expr } => {
                self.lower_unary(*op, *sub_expr, expr.ty.clone(), target, symbols)
            }
            HirExprKind::Field { base, field } => {
                self.lower_field(*base, field, expr.ty.clone(), target, symbols)
            }
            HirExprKind::Index { base, index } => {
                self.lower_index(*base, *index, expr.ty.clone(), target, symbols)
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

                let mut is_enum_ctor = None;
                match &callee_expr.kind {
                    HirExprKind::Path { symbol } => {
                        let enum_sym_from_ty = match &callee_expr.ty {
                            ArType::Named(id, _) => Some(*id),
                            ArType::Func(_, ret) => {
                                match self.tc.type_info.type_interner.resolve(*ret) {
                                    ArType::Named(id, _) => Some(id),
                                    _ => None,
                                }
                            }
                            _ => None,
                        };
                        if let Some(&tag) =
                            self.tc.type_info.enum_variant_tags.get(symbol).or_else(|| {
                                let enum_id = enum_sym_from_ty?;
                                self.tc
                                    .type_info
                                    .enum_variants
                                    .iter()
                                    .find(|&(v_sym, (parent_sym, _))| {
                                        *parent_sym == enum_id
                                            && self
                                                .tc
                                                .type_info
                                                .enum_variant_tags
                                                .contains_key(v_sym)
                                            && {
                                                let lookup_bare = symbols
                                                    .get(*symbol)
                                                    .name
                                                    .rsplit('.')
                                                    .next()
                                                    .unwrap_or("");
                                                let reg_bare = symbols
                                                    .get(*v_sym)
                                                    .name
                                                    .rsplit('.')
                                                    .next()
                                                    .unwrap_or("");
                                                lookup_bare == reg_bare
                                            }
                                    })
                                    .and_then(|(v_sym, _)| {
                                        self.tc.type_info.enum_variant_tags.get(v_sym)
                                    })
                            })
                        {
                            is_enum_ctor = Some(tag);
                        }
                    }
                    HirExprKind::TypePath {
                        type_symbol,
                        member_symbol,
                    } => {
                        if let Some(&tag) = self
                            .tc
                            .type_info
                            .enum_variant_tags
                            .get(member_symbol)
                            .or_else(|| {
                                let lookup_bare = symbols
                                    .get(*member_symbol)
                                    .name
                                    .rsplit('.')
                                    .next()
                                    .unwrap_or("");
                                self.tc
                                    .type_info
                                    .enum_variants
                                    .iter()
                                    .find(|&(v_sym, (parent_sym, _))| {
                                        *parent_sym == *type_symbol
                                            && symbols
                                                .get(*v_sym)
                                                .name
                                                .rsplit('.')
                                                .next()
                                                .unwrap_or("")
                                                == lookup_bare
                                            && self
                                                .tc
                                                .type_info
                                                .enum_variant_tags
                                                .contains_key(v_sym)
                                    })
                                    .and_then(|(v_sym, _)| {
                                        self.tc.type_info.enum_variant_tags.get(v_sym)
                                    })
                            })
                        {
                            is_enum_ctor = Some(tag);
                        }
                    }
                    _ => {}
                }

                if let Some(tag) = is_enum_ctor {
                    let args_slice = self.hir.pool.expr_list(*args);
                    let payload_op = match args_slice.len() {
                        0 => None,
                        1 => Some(self.lower_expr(args_slice[0], None, symbols)?),
                        _ => {
                            let mut item_ops = Vec::with_capacity(args_slice.len());
                            for &arg in args_slice {
                                item_ops.push(self.lower_expr(arg, None, symbols)?);
                            }
                            let param_tys = match &callee_expr.ty {
                                ArType::Func(params, _) => params.clone(),
                                _ => vec![],
                            };
                            let tuple_ty = ArType::Tuple(param_tys);
                            let dest_tuple = self.new_temp(tuple_ty);
                            self.emit_assign_temp(
                                dest_tuple,
                                AmirRvalue::Tuple { items: item_ops },
                            );
                            Some(AmirOperand::Copy(dest_tuple))
                        }
                    };
                    let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
                    self.emit_assign_temp(
                        dest,
                        AmirRvalue::EnumConstruct {
                            variant_tag: tag,
                            payload: payload_op,
                        },
                    );
                    return Ok(AmirOperand::Copy(dest));
                }

                let method_target = resolve_method_target(
                    callee_expr,
                    &self.hir.pool,
                    symbols,
                    &self.tc.type_info.type_interner,
                );
                let callee_op = if let Some(method_symbol) = method_target {
                    AmirOperand::FunctionRef(method_symbol)
                } else {
                    self.lower_expr(*callee, None, symbols)?
                };
                let args_slice = self.hir.pool.expr_list(*args);
                // Method calls: HIR already includes the receiver as arg 0 when
                // typeck rewrites `obj.m(a)` → Call(Field(m), [obj, a]). Only inject
                // `base` when args are short of the formal arity (legacy / incomplete HIR).
                let formal_params: Vec<ArType> = match &callee_expr.ty {
                    ArType::Func(params, _) => {
                        params.iter().map(|&id| self.resolve_ty(id)).collect()
                    }
                    _ => Vec::new(),
                };
                let mut arg_ops = Vec::with_capacity(args_slice.len() + 1);
                let inject_receiver = method_target.is_some()
                    && !formal_params.is_empty()
                    && args_slice.len() < formal_params.len();
                if inject_receiver
                    && let HirExprKind::Field { base, .. } | HirExprKind::SafeField { base, .. } =
                        &callee_expr.kind
                {
                    let base_op = self.lower_expr(*base, None, symbols)?;
                    arg_ops.push(base_op);
                }
                let arg_param_offset = if inject_receiver { 1 } else { 0 };
                for (i, &arg) in args_slice.iter().enumerate() {
                    let arg_expr = self.hir.pool.expr(arg);
                    let arg_op = self.lower_expr(arg, None, symbols)?;
                    let arg_op = self.consume_operand(arg_op)?;
                    let arg_op = if let Some(param_ty) = formal_params.get(i + arg_param_offset) {
                        if matches!(param_ty, ArType::Primitive(Primitive::Str)) {
                            self.maybe_to_str(arg_op, &arg_expr.ty)?
                        } else {
                            arg_op
                        }
                    } else {
                        arg_op
                    };
                    arg_ops.push(arg_op);
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
                self.seal_block(bb_then);
                self.seal_block(bb_else);

                // Then branch
                self.current_block = Some(bb_then);
                self.lower_block_as_expr(*then_block, Some(dest), symbols)?;
                if self.current_block.is_some() {
                    self.emit_goto(bb_join);
                }

                // Else branch
                self.current_block = Some(bb_else);
                self.lower_block_as_expr(*else_block, Some(dest), symbols)?;
                if self.current_block.is_some() {
                    self.emit_goto(bb_join);
                }

                // Join
                self.seal_block(bb_join);
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
                            AmirRvalue::EnumConstruct {
                                variant_tag: 0,
                                payload: Some(val_op),
                            },
                        );
                    }
                    ResultCtorVariant::Err => {
                        self.emit_assign_temp(
                            dest,
                            AmirRvalue::EnumConstruct {
                                variant_tag: 1,
                                payload: Some(val_op),
                            },
                        );
                    }
                    ResultCtorVariant::Some => {
                        self.emit_assign_temp(
                            dest,
                            AmirRvalue::EnumConstruct {
                                variant_tag: 1,
                                payload: Some(val_op),
                            },
                        );
                    }
                }
                Ok(AmirOperand::Copy(dest))
            }

            HirExprKind::Try { expr: inner } => {
                let inner_expr = self.hir.pool.expr(*inner);
                if result_ok_err(&inner_expr.ty, &self.tc.type_info.type_interner).is_some() {
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
                        left: base_op,
                        right: AmirOperand::Constant(AmirConstant::Nil),
                    },
                );

                let bb_null = self.new_block();
                let bb_access = self.new_block();
                let bb_join = self.new_block();

                self.set_bool_branch(AmirOperand::Copy(cond_tmp), bb_null, bb_access);
                self.seal_block(bb_null);
                self.seal_block(bb_access);

                self.current_block = Some(bb_null);
                self.emit_assign_temp(
                    dest,
                    AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Nil)),
                );
                self.emit_goto(bb_join);

                self.current_block = Some(bb_access);
                let base_expr = self.hir.pool.expr(*base);
                // Materialize base into a typed temp so FieldAccess never takes a
                // bare Constant(Nil) operand (pretty-print `nil.0` / ZST layout).
                // Nullable bases are pointer handles; field load goes through that ptr.
                let base_tmp = self.new_temp(base_expr.ty.clone());
                self.emit_assign_temp(base_tmp, AmirRvalue::Use(base_op));
                let field_idx = self.resolve_field_index(&base_expr.ty, field);
                self.emit_assign_temp(
                    dest,
                    AmirRvalue::FieldAccess {
                        base: AmirOperand::Copy(base_tmp),
                        field: field_idx,
                    },
                );
                self.emit_goto(bb_join);

                self.seal_block(bb_join);
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
                        left: base_op,
                        right: AmirOperand::Constant(AmirConstant::Nil),
                    },
                );

                let bb_null = self.new_block();
                let bb_access = self.new_block();
                let bb_join = self.new_block();

                self.set_bool_branch(AmirOperand::Copy(cond_tmp), bb_null, bb_access);
                self.seal_block(bb_null);
                self.seal_block(bb_access);

                self.current_block = Some(bb_null);
                self.emit_assign_temp(
                    dest,
                    AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Nil)),
                );
                self.emit_goto(bb_join);

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
                    self.emit_goto(bb_join);
                }

                self.seal_block(bb_join);
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
                        left: left_op,
                        right: AmirOperand::Constant(AmirConstant::Nil),
                    },
                );

                let bb_left = self.new_block();
                let bb_right = self.new_block();
                let bb_join = self.new_block();

                self.set_bool_branch(AmirOperand::Copy(cond_tmp), bb_left, bb_right);
                self.seal_block(bb_left);
                self.seal_block(bb_right);

                self.current_block = Some(bb_left);
                self.emit_assign_temp(dest, AmirRvalue::Use(left_op));
                self.emit_goto(bb_join);

                self.current_block = Some(bb_right);
                self.lower_expr(*right, Some(dest), symbols)?;
                if self.current_block.is_some() {
                    self.emit_goto(bb_join);
                }

                self.seal_block(bb_join);
                self.current_block = Some(bb_join);
                Ok(AmirOperand::Copy(dest))
            }
            HirExprKind::Catch { expr: inner, handler } => {
                self.lower_catch(*inner, handler, target, expr.ty.clone(), symbols)
            }
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
