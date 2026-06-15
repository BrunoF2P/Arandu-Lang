//! CFG edge computation for AMIR basic blocks (C3).

use crate::amir::{AmirBasicBlock, AmirTerminator, BlockId};
use crate::index_vec::IndexVec;
use crate::DenseRange;

#[derive(Debug, Clone, Default)]
pub struct ControlFlowGraph {
    pub successors: Vec<BlockId>,
    pub successor_ranges: IndexVec<BlockId, DenseRange>,
    pub predecessors: Vec<BlockId>,
    pub predecessor_ranges: IndexVec<BlockId, DenseRange>,
}

pub fn compute_cfg_edges(blocks: &[AmirBasicBlock]) -> ControlFlowGraph {
    let num_blocks = blocks.len();
    let mut successors = Vec::new();
    let mut successor_ranges = IndexVec::from(vec![DenseRange::empty(); num_blocks]);

    for (i, block) in blocks.iter().enumerate() {
        let bid = BlockId::from_usize(i);
        let succs = terminator_successors(&block.terminator);

        let start = successors.len();
        for succ in succs {
            if succ.as_usize() < num_blocks {
                successors.push(succ);
            }
        }
        let len = successors.len() - start;
        successor_ranges[bid] = DenseRange::new(start, len);
    }

    let mut predecessors = Vec::new();
    let mut predecessor_ranges = IndexVec::from(vec![DenseRange::empty(); num_blocks]);

    for i in 0..num_blocks {
        let target_bid = BlockId::from_usize(i);
        let start = predecessors.len();

        for source_idx in 0..num_blocks {
            let source_bid = BlockId::from_usize(source_idx);
            let range = successor_ranges[source_bid];
            let source_succs = &successors[range.as_range()];
            if source_succs.contains(&target_bid) {
                predecessors.push(source_bid);
            }
        }

        let len = predecessors.len() - start;
        predecessor_ranges[target_bid] = DenseRange::new(start, len);
    }

    ControlFlowGraph {
        successors,
        successor_ranges,
        predecessors,
        predecessor_ranges,
    }
}

fn terminator_successors(term: &AmirTerminator) -> Vec<BlockId> {
    match term {
        AmirTerminator::Return | AmirTerminator::Unreachable => Vec::new(),
        AmirTerminator::Goto(b) => vec![*b],
        AmirTerminator::Branch {
            if_true, if_false, ..
        } => vec![*if_true, *if_false],
        AmirTerminator::SwitchInt {
            targets, otherwise, ..
        } => {
            let mut out: Vec<BlockId> = targets.iter().map(|(_, b)| *b).collect();
            out.push(*otherwise);
            out
        }
    }
}
