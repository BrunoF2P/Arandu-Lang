//! AMIR CFG invariant validation (CFG-1 … CFG-5 per `docs/arandu-amir-v0.1.md`).

use crate::SymbolTable;
use crate::amir::{AmirFunc, AmirProgram, AmirTerminator, BlockId, reachable_blocks_dense};
use crate::diagnostics::{DiagCode, Diagnostic};
use crate::types::TypeInterner;

/// Validate all functions in an AMIR program.
#[must_use]
pub fn validate_amir_program(
    program: &AmirProgram,
    symbols: &SymbolTable,
    interner: &TypeInterner,
) -> Vec<Diagnostic> {
    program
        .funcs
        .iter()
        .flat_map(|f| validate_amir_func(f, symbols, interner))
        .collect()
}

#[must_use]
pub fn validate_amir_func(
    func: &AmirFunc,
    symbols: &SymbolTable,
    interner: &TypeInterner,
) -> Vec<Diagnostic> {
    let span = symbols.get(func.symbol).span;
    let mut diags = Vec::new();

    if func.blocks.is_empty() {
        diags.push(Diagnostic::error(
            DiagCode::U001FeatureNotSupported,
            "function has no basic blocks (CFG-4)".to_string(),
            span,
        ));
        return diags;
    }

    for (i, block) in func.blocks.iter().enumerate() {
        if !is_valid_terminator(&block.terminator) {
            diags.push(Diagnostic::error(
                DiagCode::U001FeatureNotSupported,
                format!("bb{i}: invalid terminator (CFG-1)"),
                span,
            ));
        }

        for succ in terminator_targets(&block.terminator) {
            if succ.as_usize() >= func.blocks.len() {
                diags.push(Diagnostic::error(
                    DiagCode::U001FeatureNotSupported,
                    format!(
                        "bb{i}: terminator targets non-existent bb{} (CFG-3)",
                        succ.as_usize()
                    ),
                    span,
                ));
            }
        }
    }

    let reachable = reachable_blocks_dense(func);
    for (i, block) in func.blocks.iter().enumerate() {
        if i == 0 {
            continue;
        }
        if !reachable.contains(BlockId::from_usize(i))
            && !matches!(block.terminator, AmirTerminator::Unreachable)
        {
            diags.push(Diagnostic::error(
                DiagCode::U001FeatureNotSupported,
                format!("bb{i}: not reachable from bb0 (CFG-5)"),
                span,
            ));
        }
    }

    for (i, local) in func.locals.iter().enumerate() {
        if local.id.as_usize() != i {
            diags.push(Diagnostic::ice(
                DiagCode::ICEGEN002,
                format!(
                    "local at index {i} has mismatched LocalId s{} (expected s{i}) (TYP-2)",
                    local.id.as_usize()
                ),
                span,
            ));
        }
        if interner.is_error(local.ty) {
            diags.push(Diagnostic::ice(
                DiagCode::ICEGEN002,
                format!(
                    "local s{} has poison type Error (TYP-1)",
                    local.id.as_usize()
                ),
                span,
            ));
        }
    }

    for (i, temp) in func.temps.iter().enumerate() {
        if temp.id.as_usize() != i {
            diags.push(Diagnostic::ice(
                DiagCode::ICEGEN002,
                format!(
                    "temp at index {i} has mismatched TempId _{} (expected _{i}) (TYP-2)",
                    temp.id.as_usize()
                ),
                span,
            ));
        }
        if interner.is_error(temp.ty) {
            diags.push(Diagnostic::ice(
                DiagCode::ICEGEN002,
                format!("temp _{} has poison type Error (TYP-1)", temp.id.as_usize()),
                span,
            ));
        }
    }

    diags
}

fn is_valid_terminator(term: &AmirTerminator) -> bool {
    matches!(
        term,
        AmirTerminator::Return
            | AmirTerminator::Goto { .. }
            | AmirTerminator::Branch { .. }
            | AmirTerminator::SwitchInt { .. }
            | AmirTerminator::Suspend { .. }
            | AmirTerminator::Unreachable
    )
}

fn terminator_targets(term: &AmirTerminator) -> Vec<BlockId> {
    match term {
        AmirTerminator::Return | AmirTerminator::Unreachable => Vec::new(),
        AmirTerminator::Goto { target, .. } => vec![*target],
        AmirTerminator::Branch {
            if_true, if_false, ..
        } => vec![*if_true, *if_false],
        AmirTerminator::SwitchInt {
            targets, otherwise, ..
        } => {
            let mut v: Vec<BlockId> = targets.iter().map(|(_, b, _)| *b).collect();
            v.push(otherwise.0);
            v
        }
        AmirTerminator::Suspend { resume, .. } => vec![*resume],
    }
}
