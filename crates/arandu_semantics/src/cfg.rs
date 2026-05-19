//! CFG edge computation for AMIR basic blocks (C3).

use crate::amir::{AmirBasicBlock, AmirTerminator, BlockId};

pub fn compute_cfg_edges(blocks: &mut [AmirBasicBlock]) {
    let len = blocks.len();
    for block in blocks.iter_mut() {
        block.successors.clear();
        block.predecessors.clear();
    }

    for i in 0..len {
        let succs = terminator_successors(&blocks[i].terminator);
        for succ in succs {
            let s = succ.as_usize();
            if s < len {
                blocks[i].successors.push(succ);
                blocks[s].predecessors.push(BlockId::from_usize(i));
            }
        }
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
