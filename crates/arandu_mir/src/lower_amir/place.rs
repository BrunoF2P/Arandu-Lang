use super::LowerCtx;
use crate::SymbolTable;
use crate::amir::{AmirOperand, AmirRvalue, TempId};
use crate::diagnostics::Diagnostic;
use crate::hir::HirExprId;
use crate::passes::type_checker::types::ArType;

impl LowerCtx<'_> {
    pub(crate) fn resolve_field_index(&self, base_ty: &ArType, field: &str) -> usize {
        if let Ok(idx) = field.parse::<usize>() {
            return idx;
        }
        if field.starts_with('_')
            && let Ok(idx) = field[1..].parse::<usize>()
        {
            return idx;
        }
        let interner = &self.tc.type_info.type_interner;
        let struct_id = match base_ty {
            ArType::Nullable(inner) => {
                let inner_ty = interner.resolve(*inner);
                match inner_ty {
                    ArType::Named(id, _) => Some(*id),
                    ArType::Ptr(ptr_inner) => {
                        let ptr_inner_ty = interner.resolve(*ptr_inner);
                        match ptr_inner_ty {
                            ArType::Named(id, _) => Some(*id),
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
                    ArType::Named(id, _) => Some(*id),
                    _ => None,
                }
            }
            _ => None,
        };
        struct_id
            .and_then(|sid| self.tc.type_info.struct_field_indices.get(&sid))
            .and_then(|m| m.get(field).copied())
            .unwrap_or(0)
    }

    pub(crate) fn lower_field(
        &mut self,
        base: HirExprId,
        field: &str,
        expr_ty: ArType,
        target: Option<TempId>,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        let base_op = self.lower_expr(base, None, symbols)?;
        let dest = target.unwrap_or_else(|| self.new_temp(expr_ty));
        let base_expr = self.hir.pool.expr(base);
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

    pub(crate) fn lower_index(
        &mut self,
        base: HirExprId,
        index: HirExprId,
        expr_ty: ArType,
        target: Option<TempId>,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        let base_op = self.lower_expr(base, None, symbols)?;
        let idx_op = self.lower_expr(index, None, symbols)?;
        let dest = target.unwrap_or_else(|| self.new_temp(expr_ty));
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
