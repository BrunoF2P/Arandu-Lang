//! A3.2: OSSA checks at suspension frontiers (`await` / [`AmirTerminator::Suspend`]).
//!
//! Absolute (`Borrow`) refs whose live range crosses a suspend point would dangle
//! if the task state moves. A3.4 rewrites eligible borrows to pin-free
//! [`crate::amir::AmirRvalue::RelativeBorrow`] first; **this** pass only rejects
//! remaining absolute `Ref`/`RefMut` temps still live into a resume block (O010).

use crate::DiagCode;
use crate::SymbolTable;
use crate::amir::{AmirFunc, AmirTerminator, TempId};
use crate::diagnostics::Diagnostic;
use crate::liveness::analyze_temp_liveness;
use crate::types::{ArType, TypeInterner};

/// Reject borrows whose temp is live into a resume block after `Suspend`.
pub fn check_borrow_across_suspend(
    func: &AmirFunc,
    _symbols: &SymbolTable,
    interner: &TypeInterner,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let has_suspend = func
        .blocks
        .iter()
        .any(|b| matches!(b.terminator, AmirTerminator::Suspend { .. }));
    if !has_suspend {
        return diags;
    }

    let temp_live = analyze_temp_liveness(func);

    for block in &func.blocks {
        let AmirTerminator::Suspend { resume, .. } = &block.terminator else {
            continue;
        };
        let live_into_resume = temp_live.live_in(*resume);
        for t in 0..func.temps.len() {
            let tid = TempId::from_usize(t);
            if !live_into_resume.contains(tid) {
                continue;
            }
            let Some(temp) = func.temps.get(t) else {
                continue;
            };
            let ty = interner.resolve(temp.ty);
            if !matches!(ty, ArType::Ref(_) | ArType::RefMut(_)) {
                continue;
            }
            // RelativeBorrow temps still have Ref type but hold a LocalId index —
            // safe across suspend. Detect by scanning defining assign.
            if temp_is_relative_borrow(func, tid) {
                continue;
            }
            let span = temp.span;
            diags.push(
                Diagnostic::error(
                    DiagCode::O010EscapeOfBorrowedValue,
                    "borrow cannot cross an `await` suspension point \
                     (absolute reference would outlive the stack frame / task state)",
                    span,
                )
                .with_label(
                    span,
                    "this absolute reference is still live when the coroutine suspends",
                )
                .with_hint(
                    "copy the value before `await`, or use a local-only borrow that \
                     A3.4 can rewrite to a pin-free LocalId relative ref",
                ),
            );
        }
    }

    diags
}

fn temp_is_relative_borrow(func: &AmirFunc, tid: TempId) -> bool {
    use crate::amir::{AmirRvalue, AmirStmt};
    for block in &func.blocks {
        for stmt in func.block_stmts(block.id) {
            if let AmirStmt::Assign {
                lhs,
                rhs: AmirRvalue::RelativeBorrow { .. },
            } = stmt
            {
                if *lhs == tid {
                    return true;
                }
            }
        }
    }
    false
}
