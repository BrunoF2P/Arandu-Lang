use super::LowerCtx;
use crate::amir::{AmirConstant, AmirOperand, AmirPlace, AmirRvalue};
use crate::diagnostics::{DiagCode, Diagnostic};
use crate::hir::{HirCondition, HirDecl, HirPattern};
use crate::literal_pool::AmirLiteralEntry;
use crate::ops::BinaryOp;
use crate::passes::type_checker::types::{ArType, Primitive};
use crate::{SymbolId, SymbolTable};
use arandu_lexer::Span;

pub(crate) struct EnumPatternInput<'a> {
    scrutinee: AmirOperand,
    span: Span,
    type_symbol: SymbolId,
    variant: &'a str,
    variant_symbol: Option<SymbolId>,
    payload: &'a [HirPattern],
    symbols: &'a SymbolTable,
}
impl LowerCtx<'_> {
    pub(crate) fn lower_enum_pattern(
        &mut self,
        input: EnumPatternInput<'_>,
    ) -> Result<AmirOperand, Diagnostic> {
        let EnumPatternInput {
            scrutinee,
            span,
            type_symbol,
            variant,
            variant_symbol,
            payload,
            symbols,
        } = input;
        let enum_name = &symbols.get(type_symbol).name;
        let variant_symbol_id =
            variant_symbol.or_else(|| symbols.lookup_associated_member(enum_name, variant));

        let Some(variant_symbol_id) = variant_symbol_id else {
            return Err(Diagnostic::error(
                DiagCode::T018UndefinedField,
                format!("variant '{variant}' is not defined on enum '{enum_name}'"),
                span,
            ));
        };

        let mut variant_tag = None;
        let mut found_variant_symbol = None;
        for &decl_id in &self.hir.decls {
            let decl = self.hir.pool.decl(decl_id);
            if let HirDecl::Enum(hir_enum) = decl
                && hir_enum.symbol == type_symbol
            {
                for (index, v) in self
                    .hir
                    .pool
                    .enum_variants_list(hir_enum.variants)
                    .iter()
                    .enumerate()
                {
                    if symbols.get(v.symbol).name == variant {
                        variant_tag = Some(index);
                        found_variant_symbol = Some(v.symbol);
                        break;
                    }
                }
                break;
            }
        }

        let Some(tag_value) = variant_tag else {
            return Err(Diagnostic::error(
                DiagCode::T018UndefinedField,
                format!("variant '{variant}' tag not found on enum '{enum_name}'"),
                span,
            ));
        };

        let tmp_tag = self.new_temp(ArType::Primitive(Primitive::Int));
        self.emit_assign_temp(
            tmp_tag,
            AmirRvalue::Discriminant {
                value: scrutinee.clone(),
            },
        );

        let tag_op = AmirOperand::Constant(
            self.intern_literal(AmirLiteralEntry::Int(tag_value.to_string())),
        );
        let tag_matches = self.new_temp(ArType::Primitive(Primitive::Bool));
        self.emit_assign_temp(
            tag_matches,
            AmirRvalue::Binary {
                op: BinaryOp::Equal,
                left: AmirOperand::Copy(tmp_tag),
                right: tag_op,
            },
        );

        if payload.is_empty() {
            Ok(AmirOperand::Copy(tag_matches))
        } else {
            let variant_symbol_actual = found_variant_symbol.unwrap_or(variant_symbol_id);
            let shape_opt = self.tc.type_info.enum_variants.get(&variant_symbol_actual);
            let Some((_, crate::passes::type_checker::EnumPayloadShape::Tuple(tys))) = shape_opt
            else {
                return Err(Diagnostic::error(
                    DiagCode::T012WrongArgCount,
                    format!(
                        "enum variant '{}' expects 0 payload items, found {}",
                        variant,
                        payload.len()
                    ),
                    span,
                ));
            };

            if tys.len() != payload.len() {
                return Err(Diagnostic::error(
                    DiagCode::T012WrongArgCount,
                    format!(
                        "enum variant '{}' expects {} payload items, found {}",
                        variant,
                        tys.len(),
                        payload.len()
                    ),
                    span,
                ));
            }

            let mut current_matches = AmirOperand::Copy(tag_matches);

            for (i, pat) in payload.iter().enumerate() {
                let tmp_payload = self.new_temp(tys[i].clone());
                self.emit_assign_temp(
                    tmp_payload,
                    AmirRvalue::EnumPayload {
                        value: scrutinee.clone(),
                        variant: variant_symbol_actual,
                        index: i,
                    },
                );

                let item_matches =
                    self.lower_pattern_match(AmirOperand::Copy(tmp_payload), pat, symbols)?;

                let and_dest = self.new_temp(ArType::Primitive(Primitive::Bool));
                self.emit_assign_temp(
                    and_dest,
                    AmirRvalue::Binary {
                        op: BinaryOp::And,
                        left: current_matches,
                        right: item_matches,
                    },
                );
                current_matches = AmirOperand::Copy(and_dest);
            }

            Ok(current_matches)
        }
    }

    pub(crate) fn lower_condition(
        &mut self,
        cond: &HirCondition,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        match cond {
            HirCondition::Expr(expr_id) => self.lower_expr(*expr_id, None, symbols),
            HirCondition::Is { expr, pattern } => {
                let scrutinee = self.lower_expr(*expr, None, symbols)?;
                let pat = self.hir.pool.pattern(*pattern).clone();
                self.lower_pattern_match(scrutinee, &pat, symbols)
            }
        }
    }

    pub(crate) fn lower_pattern_match(
        &mut self,
        scrutinee: AmirOperand,
        pattern: &HirPattern,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        match pattern {
            HirPattern::Wildcard { .. } => Ok(AmirOperand::Constant(AmirConstant::Bool(true))),
            HirPattern::Bind { symbol, .. } => {
                let ty = self
                    .tc
                    .type_info
                    .decl_type(*symbol)
                    .cloned()
                    .unwrap_or(ArType::Error);
                let local_id = self.new_local(ty, *symbol, pattern.span());
                self.emit_store_place(
                    AmirPlace {
                        local: local_id,
                        projections: smallvec::SmallVec::new(),
                    },
                    scrutinee,
                )?;
                Ok(AmirOperand::Constant(AmirConstant::Bool(true)))
            }
            HirPattern::Literal {
                expr: lit_expr_id, ..
            } => {
                let lit_op = self.lower_expr(*lit_expr_id, None, symbols)?;
                let dest = self.new_temp(ArType::Primitive(Primitive::Bool));
                self.emit_assign_temp(
                    dest,
                    AmirRvalue::Binary {
                        op: BinaryOp::Equal,
                        left: scrutinee,
                        right: lit_op,
                    },
                );
                Ok(AmirOperand::Copy(dest))
            }
            HirPattern::Enum {
                span,
                type_symbol,
                variant,
                variant_symbol,
                payload,
            } => {
                let payload_patterns: Vec<HirPattern> = self
                    .hir
                    .pool
                    .pattern_list(*payload)
                    .iter()
                    .map(|&pid| self.hir.pool.pattern(pid).clone())
                    .collect();
                self.lower_enum_pattern(EnumPatternInput {
                    scrutinee,
                    span: *span,
                    type_symbol: *type_symbol,
                    variant: variant.as_str(),
                    variant_symbol: *variant_symbol,
                    payload: &payload_patterns,
                    symbols,
                })
            }
            HirPattern::Struct {
                struct_symbol,
                fields,
                ..
            } => {
                let fields_map = self.tc.type_info.struct_fields.get(struct_symbol);
                let mut current_matches = AmirOperand::Constant(AmirConstant::Bool(true));

                let field_ids: Vec<_> = self.hir.pool.field_pattern_list(*fields).to_vec();
                for &fid in &field_ids {
                    let field = self.hir.pool.field_pattern(fid).clone();
                    let field_ty = fields_map
                        .and_then(|m| m.get(&field.name).cloned())
                        .unwrap_or(ArType::Error);

                    let tmp_field = self.new_temp(field_ty.clone());
                    let field_idx = self
                        .tc
                        .type_info
                        .struct_field_indices
                        .get(struct_symbol)
                        .and_then(|m| m.get(&field.name).copied())
                        .unwrap_or(0);
                    self.emit_assign_temp(
                        tmp_field,
                        AmirRvalue::FieldAccess {
                            base: scrutinee.clone(),
                            field: field_idx,
                        },
                    );

                    let item_matches = if let Some(pat_id) = field.pattern {
                        let pat = self.hir.pool.pattern(pat_id).clone();
                        self.lower_pattern_match(AmirOperand::Copy(tmp_field), &pat, symbols)?
                    } else {
                        let key = crate::NodeKey::from(field.span);
                        let Some(symbol_id) = self.tc.resolved.definitions.get(&key).copied()
                        else {
                            return Err(Diagnostic::error(
                                DiagCode::T018UndefinedField,
                                format!(
                                    "field '{}' symbol not found during struct lowering",
                                    field.name
                                ),
                                field.span,
                            ));
                        };
                        let local_id = self.new_local(field_ty, symbol_id, field.span);
                        self.emit_store_place(
                            AmirPlace {
                                local: local_id,
                                projections: smallvec::SmallVec::new(),
                            },
                            AmirOperand::Copy(tmp_field),
                        )?;
                        AmirOperand::Constant(AmirConstant::Bool(true))
                    };

                    let and_dest = self.new_temp(ArType::Primitive(Primitive::Bool));
                    self.emit_assign_temp(
                        and_dest,
                        AmirRvalue::Binary {
                            op: BinaryOp::And,
                            left: current_matches,
                            right: item_matches,
                        },
                    );
                    current_matches = AmirOperand::Copy(and_dest);
                }

                Ok(current_matches)
            }
            HirPattern::Tuple { items, .. } => {
                let scrutinee_ty = self.operand_type(&scrutinee);
                let pat_ids: Vec<_> = self.hir.pool.pattern_list(*items).to_vec();
                let item_tys = if let ArType::Tuple(tys) = scrutinee_ty {
                    tys.iter()
                        .map(|&tid| {
                            arandu_middle::types::type_interner::with_resolved_type(tid, |t| {
                                t.clone()
                            })
                        })
                        .collect()
                } else {
                    vec![ArType::Error; pat_ids.len()]
                };

                let mut current_matches = AmirOperand::Constant(AmirConstant::Bool(true));
                for (i, &pid) in pat_ids.iter().enumerate() {
                    let pat = self.hir.pool.pattern(pid).clone();
                    let item_ty = item_tys.get(i).cloned().unwrap_or(ArType::Error);
                    let tmp_item = self.new_temp(item_ty.clone());
                    self.emit_assign_temp(
                        tmp_item,
                        AmirRvalue::FieldAccess {
                            base: scrutinee.clone(),
                            field: i,
                        },
                    );

                    let item_matches =
                        self.lower_pattern_match(AmirOperand::Copy(tmp_item), &pat, symbols)?;

                    let and_dest = self.new_temp(ArType::Primitive(Primitive::Bool));
                    self.emit_assign_temp(
                        and_dest,
                        AmirRvalue::Binary {
                            op: BinaryOp::And,
                            left: current_matches,
                            right: item_matches,
                        },
                    );
                    current_matches = AmirOperand::Copy(and_dest);
                }
                Ok(current_matches)
            }
            HirPattern::TypeTuple {
                span,
                name,
                payload,
            } => {
                let scrutinee_ty = self.operand_type(&scrutinee);
                let ArType::Named(type_symbol, _) = scrutinee_ty else {
                    return Err(Diagnostic::error(
                        DiagCode::T002IncompatibleAssignment,
                        "cannot match type tuple pattern against non-enum type".to_string(),
                        *span,
                    ));
                };

                let payload_patterns: Vec<HirPattern> = self
                    .hir
                    .pool
                    .pattern_list(*payload)
                    .iter()
                    .map(|&pid| self.hir.pool.pattern(pid).clone())
                    .collect();
                self.lower_enum_pattern(EnumPatternInput {
                    scrutinee,
                    span: *span,
                    type_symbol,
                    variant: name.as_str(),
                    variant_symbol: None,
                    payload: &payload_patterns,
                    symbols,
                })
            }
            HirPattern::Range {
                span: _,
                start,
                inclusive,
                end,
            } => {
                let start_op = self.lower_expr(*start, None, symbols)?;
                let end_op = self.lower_expr(*end, None, symbols)?;

                let ge_dest = self.new_temp(ArType::Primitive(Primitive::Bool));
                self.emit_assign_temp(
                    ge_dest,
                    AmirRvalue::Binary {
                        op: BinaryOp::GtEqual,
                        left: scrutinee.clone(),
                        right: start_op,
                    },
                );

                let limit_op = if *inclusive {
                    BinaryOp::LtEqual
                } else {
                    BinaryOp::Lt
                };
                let limit_dest = self.new_temp(ArType::Primitive(Primitive::Bool));
                self.emit_assign_temp(
                    limit_dest,
                    AmirRvalue::Binary {
                        op: limit_op,
                        left: scrutinee.clone(),
                        right: end_op,
                    },
                );

                let range_dest = self.new_temp(ArType::Primitive(Primitive::Bool));
                self.emit_assign_temp(
                    range_dest,
                    AmirRvalue::Binary {
                        op: BinaryOp::And,
                        left: AmirOperand::Copy(ge_dest),
                        right: AmirOperand::Copy(limit_dest),
                    },
                );
                Ok(AmirOperand::Copy(range_dest))
            }
        }
    }
}
