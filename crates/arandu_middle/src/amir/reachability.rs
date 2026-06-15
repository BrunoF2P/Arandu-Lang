use super::{AmirFunc, AmirTerminator, BlockId};
use crate::BitSet;

#[must_use]
pub fn reachable_blocks_dense(func: &AmirFunc) -> BitSet<BlockId> {
    let mut reachable = BitSet::with_capacity(func.blocks.len());
    if func.blocks.is_empty() {
        return reachable;
    }

    let mut stack = vec![BlockId::from_usize(0)];
    while let Some(block_id) = stack.pop() {
        let index = block_id.as_usize();
        if index >= func.blocks.len() || !reachable.insert(block_id) {
            continue;
        }

        for successor in terminator_targets(&func.blocks[index].terminator) {
            stack.push(successor);
        }
    }

    reachable
}

pub fn terminator_targets(term: &AmirTerminator) -> Vec<BlockId> {
    match term {
        AmirTerminator::Return | AmirTerminator::Unreachable => Vec::new(),
        AmirTerminator::Goto(block) => vec![*block],
        AmirTerminator::Branch {
            if_true, if_false, ..
        } => vec![*if_true, *if_false],
        AmirTerminator::SwitchInt {
            targets, otherwise, ..
        } => {
            let mut blocks: Vec<BlockId> = targets.iter().map(|(_, block)| *block).collect();
            blocks.push(*otherwise);
            blocks
        }
    }
}
