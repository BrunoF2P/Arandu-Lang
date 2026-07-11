//! A3.2: OSSA checks at suspension frontiers (`await` / [`AmirTerminator::Suspend`]).
//!
//! A shared/mutable borrow (`&T` / `&mut T` temp) whose live range crosses a
//! suspend point is only safe if the referent lives in coroutine state that
//! outlasts the suspension. Stack frames of the creator may not exist when the
//! task resumes — so a live `Ref`/`RefMut` temp into resume is treated as an
//! escape of a borrowed value (same class as O010).
//!
//! This is the concrete checker the A3 design specified: a liveness query at the
//! await frontier (F2.2 / temp liveness), not a separate analysis engine.

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
            let span = temp.span;
            diags.push(
                Diagnostic::error(
                    DiagCode::O010EscapeOfBorrowedValue,
                    "borrow cannot cross an `await` suspension point \
                     (reference would outlive the stack frame / task state)",
                    span,
                )
                .with_label(
                    span,
                    "this reference is still live when the coroutine suspends",
                )
                .with_hint(
                    "copy the value before `await`, or wait for A3.4 pin-free \
                     LocalId refs in coroutine state",
                ),
            );
        }
    }

    diags
}
