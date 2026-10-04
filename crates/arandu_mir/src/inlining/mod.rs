//! Interprocedural leaf-function inlining pass on AMIR.

pub mod cost;
pub mod splice;

use crate::SymbolId;
use crate::amir::{AmirOperand, AmirProgram, AmirStmt, BlockId, InstrId, TempId};
use cost::{INLINE_LEAF_BUDGET, evaluate_leaf_inlining};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use splice::splice_call;

/// Maximum number of inlines into a single caller function to avoid code-bloat.
pub const MAX_INLINES_PER_CALLER: usize = 32;

/// Reconsider wrappers that become small leaves after their helpers inline.
/// The call-site budget remains cumulative across rounds, not per round.
const MAX_INLINING_ROUNDS: usize = 8;

struct InlinableCall {
    bid: BlockId,
    instr_id: InstrId,
    lhs: Option<TempId>,
    args: SmallVec<[AmirOperand; 4]>,
    callee_sym: SymbolId,
}

/// Optimizes `program` by inlining eligible small leaf functions into their callers.
///
/// Returns the total number of call sites inlined across the program.
pub fn inline_leaf_functions(program: &mut AmirProgram) -> usize {
    let mut total_inlined = 0usize;
    let mut inlines_per_caller = vec![0usize; program.funcs.len()];
    for _ in 0..MAX_INLINING_ROUNDS {
        // 1. Identify all eligible leaf functions and map their SymbolId to index in `program.funcs`
        let mut eligible = FxHashMap::default();
        for (idx, func) in program.funcs.iter().enumerate() {
            if let Some(cost) = evaluate_leaf_inlining(func, INLINE_LEAF_BUDGET) {
                eligible.insert(func.symbol, (idx, cost));
            }
        }

        if eligible.is_empty() {
            break;
        }

        let before_round = total_inlined;

        // 2. For each function in program, inline eligible calls
        for (caller_idx, inline_count) in inlines_per_caller.iter_mut().enumerate() {
            let caller_sym = program.funcs[caller_idx].symbol;
            let mut inlines_in_caller = *inline_count;

            loop {
                if inlines_in_caller >= MAX_INLINES_PER_CALLER {
                    break;
                }

                // Find the next inlinable call site in caller
                let caller = &program.funcs[caller_idx];
                let mut target_call: Option<InlinableCall> = None;

                'search: for (bid_usize, block) in caller.blocks.iter().enumerate() {
                    let bid = BlockId::from_usize(bid_usize);
                    for instr_id in block.statements.iter_ids::<InstrId>() {
                        if let Some(AmirStmt::Call {
                            lhs,
                            callee: AmirOperand::FunctionRef(callee_sym),
                            args,
                            ..
                        }) = caller.try_stmt(instr_id)
                            && *callee_sym != caller_sym
                            && eligible.contains_key(callee_sym)
                        {
                            target_call = Some(InlinableCall {
                                bid,
                                instr_id,
                                lhs: *lhs,
                                args: args.clone(),
                                callee_sym: *callee_sym,
                            });
                            break 'search;
                        }
                    }
                }

                let Some(call) = target_call else {
                    break;
                };

                let Some(&(callee_idx, _)) = eligible.get(&call.callee_sym) else {
                    break;
                };
                // Clone the callee template so caller can be mutated in place
                let callee = program.funcs[callee_idx].clone();

                let caller_mut = &mut program.funcs[caller_idx];
                if splice_call(
                    caller_mut,
                    call.bid,
                    call.instr_id,
                    call.lhs,
                    &call.args,
                    &callee,
                ) {
                    inlines_in_caller += 1;
                    total_inlined += 1;
                } else {
                    // Splicing failed; don't loop endlessly on the same call
                    break;
                }
            }
            *inline_count = inlines_in_caller;
        }
        if total_inlined == before_round {
            break;
        }
    }

    total_inlined
}

#[cfg(test)]
mod tests;
