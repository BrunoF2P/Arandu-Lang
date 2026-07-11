use super::LowerCtx;
use crate::SymbolTable;
use crate::amir::{AmirOperand, AmirRvalue, TempId};
use crate::diagnostics::Diagnostic;
use crate::hir::HirExprId;
use crate::ops::{BinaryOp, UnaryOp};

impl LowerCtx<'_> {
    pub(crate) fn lower_binary(
        &mut self,
        op: BinaryOp,
        left: HirExprId,
        right: HirExprId,
        expr_ty: crate::types::TypeId,
        target: Option<TempId>,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        let l_op = self.lower_expr(left, None, symbols)?;
        let r_op = self.lower_expr(right, None, symbols)?;
        let dest = target.unwrap_or_else(|| self.new_temp_id(expr_ty));
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
        expr_ty: crate::types::TypeId,
        target: Option<TempId>,
        symbols: &SymbolTable,
    ) -> Result<AmirOperand, Diagnostic> {
        // F2.0: `&`/`&mut` lower to place borrows; `*` on a ref loads through the pointer.
        match op {
            UnaryOp::Ref | UnaryOp::RefMut => {
                let place = self.lower_expr_to_place(sub_expr, symbols)?;
                // F2.0: address-taken *stack* scalars need a stack home (`is_memory`).
                // BC.4a: a place that goes through `Deref` already has a materialised
                // pointer in the local's SSA value — do NOT force a stack slot for it
                // (stack_addr of the pointer cell ≠ the pointer itself).
                let through_ptr = place
                    .projections
                    .iter()
                    .any(|p| matches!(p, crate::amir::AmirProjection::Deref));
                if place.projections.is_empty() && !through_ptr {
                    let idx = place.local.as_usize();
                    if idx < self.locals.len() {
                        self.locals[idx].is_memory = true;
                    }
                }
                let dest = target.unwrap_or_else(|| self.new_temp_id(expr_ty));
                let rv = if matches!(op, UnaryOp::RefMut) {
                    AmirRvalue::BorrowMut(place)
                } else {
                    AmirRvalue::Borrow(place)
                };
                self.emit_assign_temp(dest, rv);
                Ok(AmirOperand::Copy(dest))
            }
            UnaryOp::Deref => {
                // `*p` where p is `&T` / `&mut T` / local holding a ref: load pointee.
                // If sub is a place of the referent (`*&x` after fold would be x), use Load.
                // Otherwise treat the operand as a pointer value and load through it via
                // FieldAccess-free Load of a temporary place when possible.
                if let Ok(place) = self.lower_expr_to_place(sub_expr, symbols) {
                    // Local of type Ref/RefMut still needs one indirection — Load the place
                    // yields the reference bits; for stack locals of Ref, that *is* the
                    // pointer. Backend maps Load of Ref-typed local as "use pointer value"
                    // and for Borrow result we already have a pointer temp.
                    //
                    // Gold path: `*p` with p: &T → emit Load after reinterpreting.
                    // Use Unary Deref for pointer-valued operands so backends can load.
                    let sub_op = self.read_variable_source(place.local)?;
                    let dest = target.unwrap_or_else(|| self.new_temp_id(expr_ty));
                    if place.projections.is_empty() {
                        self.emit_assign_temp(
                            dest,
                            AmirRvalue::Unary {
                                op: UnaryOp::Deref,
                                operand: sub_op,
                            },
                        );
                    } else {
                        self.emit_assign_temp(dest, AmirRvalue::Load(place));
                    }
                    Ok(AmirOperand::Copy(dest))
                } else {
                    let sub_op = self.lower_expr(sub_expr, None, symbols)?;
                    let dest = target.unwrap_or_else(|| self.new_temp_id(expr_ty));
                    self.emit_assign_temp(
                        dest,
                        AmirRvalue::Unary {
                            op: UnaryOp::Deref,
                            operand: sub_op,
                        },
                    );
                    Ok(AmirOperand::Copy(dest))
                }
            }
            _ => {
                let sub_op = self.lower_expr(sub_expr, None, symbols)?;
                let dest = target.unwrap_or_else(|| self.new_temp_id(expr_ty));
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
    }
}
