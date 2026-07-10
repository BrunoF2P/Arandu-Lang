use super::LowerCtx;
use crate::SymbolTable;
use crate::amir::{AmirOperand, AmirRvalue, TempId};
use crate::diagnostics::Diagnostic;
use crate::hir::HirExprId;
use crate::ops::{BinaryOp, UnaryOp};
use crate::passes::type_checker::types::ArType;

impl LowerCtx<'_> {
    pub(crate) fn lower_binary(
        &mut self,
        op: BinaryOp,
        left: HirExprId,
        right: HirExprId,
        expr_ty: &ArType,
        target: Option<TempId>,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        let l_op = self.lower_expr(left, None, symbols)?;
        let r_op = self.lower_expr(right, None, symbols)?;
        let dest = target.unwrap_or_else(|| self.new_temp_ref(expr_ty));
        self.emit_assign_temp(
            dest,
            AmirRvalue::Binary {
                op,
                left: l_op,
                right: r_op,
            },
        );
        Ok(AmirOperand::Copy(dest))
    }

    pub(crate) fn lower_unary(
        &mut self,
        op: UnaryOp,
        sub_expr: HirExprId,
        expr_ty: &ArType,
        target: Option<TempId>,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        let sub_op = self.lower_expr(sub_expr, None, symbols)?;
        let dest = target.unwrap_or_else(|| self.new_temp_ref(expr_ty));
        self.emit_assign_temp(
            dest,
            AmirRvalue::Unary {
                op,
                operand: sub_op,
            },
        );
        Ok(AmirOperand::Copy(dest))
    }
}
