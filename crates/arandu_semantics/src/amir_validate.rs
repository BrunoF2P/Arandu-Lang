//! AMIR CFG invariant validation (CFG-1 … CFG-5 per `docs/arandu-amir-v0.1.md`).

use crate::amir::{AmirFunc, AmirProgram, AmirTerminator, BlockId};
use crate::diagnostics::{DiagCode, Diagnostic};
use crate::SymbolTable;

/// Validate all functions in an AMIR program.
#[must_use]
pub fn validate_amir_program(program: &AmirProgram, symbols: &SymbolTable) -> Vec<Diagnostic> {
    program
        .funcs
        .iter()
        .flat_map(|f| validate_amir_func(f, symbols))
        .collect()
}

#[must_use]
pub fn validate_amir_func(func: &AmirFunc, symbols: &SymbolTable) -> Vec<Diagnostic> {
    let span = symbols.get(func.symbol).span;
    let mut diags = Vec::new();

    if func.blocks.is_empty() {
        diags.push(Diagnostic::error(
            DiagCode::L002AmirUnsupportedFeature,
            "function has no basic blocks (CFG-4)".to_string(),
            span,
        ));
        return diags;
    }

    for (i, block) in func.blocks.iter().enumerate() {
        if !is_valid_terminator(&block.terminator) {
            diags.push(Diagnostic::error(
                DiagCode::L002AmirUnsupportedFeature,
                format!("bb{i}: invalid terminator (CFG-1)"),
                span,
            ));
        }

        for succ in terminator_targets(&block.terminator) {
            if succ.as_usize() >= func.blocks.len() {
                diags.push(Diagnostic::error(
                    DiagCode::L002AmirUnsupportedFeature,
                    format!(
                        "bb{i}: terminator targets non-existent bb{} (CFG-3)",
                        succ.as_usize()
                    ),
                    span,
                ));
            }
        }
    }

    let reachable = reachable_blocks(func);
    for (i, block) in func.blocks.iter().enumerate() {
        if i == 0 {
            continue;
        }
        if !reachable.contains(&i)
            && !matches!(block.terminator, AmirTerminator::Unreachable)
        {
            diags.push(Diagnostic::error(
                DiagCode::L002AmirUnsupportedFeature,
                format!("bb{i}: not reachable from bb0 (CFG-5)"),
                span,
            ));
        }
    }

    for local in &func.locals {
        if local.ty.is_error() {
            diags.push(Diagnostic::error(
                DiagCode::L002AmirUnsupportedFeature,
                format!(
                    "local s{} has poison type Error (TYP-1)",
                    local.id.as_usize()
                ),
                span,
            ));
        }
    }

    for temp in &func.temps {
        if temp.ty.is_error() {
            diags.push(Diagnostic::error(
                DiagCode::L002AmirUnsupportedFeature,
                format!(
                    "temp _{} has poison type Error (TYP-1)",
                    temp.id.as_usize()
                ),
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
            | AmirTerminator::Goto(_)
            | AmirTerminator::Branch { .. }
            | AmirTerminator::SwitchInt { .. }
            | AmirTerminator::Unreachable
    )
}

fn terminator_targets(term: &AmirTerminator) -> Vec<BlockId> {
    match term {
        AmirTerminator::Return | AmirTerminator::Unreachable => Vec::new(),
        AmirTerminator::Goto(b) => vec![*b],
        AmirTerminator::Branch {
            if_true, if_false, ..
        } => vec![*if_true, *if_false],
        AmirTerminator::SwitchInt {
            targets, otherwise, ..
        } => {
            let mut v: Vec<BlockId> = targets.iter().map(|(_, b)| *b).collect();
            v.push(*otherwise);
            v
        }
    }
}

fn reachable_blocks(func: &AmirFunc) -> std::collections::HashSet<usize> {
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![0usize];
    while let Some(i) = stack.pop() {
        if !seen.insert(i) || i >= func.blocks.len() {
            continue;
        }
        for succ in terminator_targets(&func.blocks[i].terminator) {
            stack.push(succ.as_usize());
        }
    }
    seen
}
