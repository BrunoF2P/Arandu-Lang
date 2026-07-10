use super::LowerCtx;
use crate::SymbolId;
use crate::SymbolTable;
use crate::amir::{AmirOperand, AmirPlace, AmirProjection, AmirRvalue, TempId};
use crate::diagnostics::Diagnostic;
use crate::hir::{HirExprId, HirExprKind};
use crate::passes::type_checker::types::ArType;

impl LowerCtx<'_> {
    /// Lower an expression to an [`AmirPlace`] for Borrow/Load (F2.0).
    ///
    /// Gold path: locals and field chains. Other forms get a clear diagnostic.
    pub(crate) fn lower_expr_to_place(
        &mut self,
        expr_id: HirExprId,
        symbols: &SymbolTable,
    ) -> Result<AmirPlace, Diagnostic> {
        let expr = self.hir.pool.expr(expr_id);
        match &expr.kind {
            HirExprKind::Path { symbol } => {
                if let Some(&local_id) = self.symbol_map.get(symbol) {
                    return Ok(AmirPlace {
                        local: local_id,
                        projections: smallvec::smallvec![],
                    });
                }
                Err(self.move_diag(format!(
                    "cannot take address of non-local `{}`",
                    symbols.get(*symbol).name
                )))
            }
            HirExprKind::Field { base, field } | HirExprKind::SafeField { base, field } => {
                let mut place = self.lower_expr_to_place(*base, symbols)?;
                let base_ty = self.resolve_ty(self.hir.pool.expr(*base).ty);
                let field_sym = self
                    .resolve_field_symbol(&base_ty, field.as_str())
                    .ok_or_else(|| {
                        self.move_diag(format!(
                            "cannot borrow field `{}`: symbol not resolved",
                            field
                        ))
                    })?;
                place.projections.push(AmirProjection::Field(field_sym));
                Ok(place)
            }
            _ => Err(self.move_diag("F2.0: can only borrow locals and field paths (`&x`, `&x.f`)")),
        }
    }

    /// Struct symbol for a base type (unwraps Ptr/Ref/RefMut/Nullable).
    fn struct_id_of_base(&self, base_ty: &ArType) -> Option<SymbolId> {
        let interner = &self.tc.type_info.type_interner;
        match base_ty {
            ArType::Named(id, _) => Some(*id),
            ArType::Nullable(inner)
            | ArType::Ptr(inner)
            | ArType::Ref(inner)
            | ArType::RefMut(inner) => {
                let inner_ty = interner.resolve(*inner);
                match inner_ty {
                    ArType::Named(id, _) => Some(id),
                    ArType::Ptr(ptr_inner) | ArType::Ref(ptr_inner) | ArType::RefMut(ptr_inner) => {
                        match interner.resolve(ptr_inner) {
                            ArType::Named(id, _) => Some(id),
                            _ => None,
                        }
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    pub(crate) fn resolve_field_symbol(&self, base_ty: &ArType, field: &str) -> Option<SymbolId> {
        let sid = self.struct_id_of_base(base_ty)?;
        self.tc
            .type_info
            .struct_field_symbols
            .get(&sid)
            .and_then(|m| m.get(field).copied())
    }

    pub(crate) fn resolve_field_index(&self, base_ty: &ArType, field: &str) -> usize {
        if let Ok(idx) = field.parse::<usize>() {
            return idx;
        }
        if field.starts_with('_')
            && let Ok(idx) = field[1..].parse::<usize>()
        {
            return idx;
        }
        self.struct_id_of_base(base_ty)
            .and_then(|sid| self.tc.type_info.struct_field_indices.get(&sid))
            .and_then(|m| m.get(field).copied())
            .unwrap_or(0)
    }

    pub(crate) fn lower_field(
        &mut self,
        base: HirExprId,
        field: &str,
        expr_ty: crate::types::TypeId,
        target: Option<TempId>,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        let base_op = self.lower_expr(base, None, symbols)?;
        let dest = target.unwrap_or_else(|| self.new_temp_id(expr_ty));
        let base_expr = self.hir.pool.expr(base);
        let base_ty = self.resolve_ty(base_expr.ty);
        let field_idx = self.resolve_field_index(&base_ty, field);
        self.emit_assign_temp(
            dest,
            AmirRvalue::FieldAccess {
                base: base_op,
                field: field_idx,
            },
        );
        Ok(AmirOperand::Copy(dest))
    }

    pub(crate) fn lower_index(
        &mut self,
        base: HirExprId,
        index: HirExprId,
        expr_ty: crate::types::TypeId,
        target: Option<TempId>,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        let base_op = self.lower_expr(base, None, symbols)?;
        let idx_op = self.lower_expr(index, None, symbols)?;
        let dest = target.unwrap_or_else(|| self.new_temp_id(expr_ty));
        self.emit_assign_temp(
            dest,
            AmirRvalue::IndexAccess {
                base: base_op,
                index: idx_op,
            },
        );
        Ok(AmirOperand::Copy(dest))
    }
}
