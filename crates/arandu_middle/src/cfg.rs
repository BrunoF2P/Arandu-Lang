//! CFG edge computation for AMIR basic blocks (C3).

use crate::DenseRange;
use crate::amir::{AmirBasicBlock, AmirTerminator, BlockId};
use crate::index_vec::IndexVec;

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

fn terminator_successors(term: &AmirTerminator) -> smallvec::SmallVec<[BlockId; 2]> {
    match term {
        AmirTerminator::Return | AmirTerminator::Unreachable => smallvec::SmallVec::new(),
        AmirTerminator::Goto(b) => {
            let mut s = smallvec::SmallVec::new();
            s.push(*b);
            s
        }
        AmirTerminator::Branch {
            if_true, if_false, ..
        } => {
            let mut s = smallvec::SmallVec::new();
            s.push(*if_true);
            s.push(*if_false);
            s
        }
        AmirTerminator::SwitchInt {
            targets, otherwise, ..
        } => {
            let mut out = smallvec::SmallVec::new();
            for (_, b) in targets {
                out.push(*b);
            }
            out.push(*otherwise);
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amir::{AmirConstant, AmirOperand, AmirTerminator};

    fn block(id: usize, terminator: AmirTerminator) -> AmirBasicBlock {
        AmirBasicBlock {
            id: BlockId::from_usize(id),
            statements: DenseRange::empty(),
            terminator,
        }
    }

    #[test]
    fn cfg_empty_no_blocks() {
        let cfg = compute_cfg_edges(&[]);
        assert!(cfg.successors.is_empty());
        assert!(cfg.predecessors.is_empty());
    }

    #[test]
    fn cfg_single_return_block() {
        let blocks = vec![block(0, AmirTerminator::Return)];
        let cfg = compute_cfg_edges(&blocks);
        assert_eq!(cfg.successor_ranges[BlockId::from_usize(0)].len, 0);
        assert_eq!(cfg.predecessor_ranges[BlockId::from_usize(0)].len, 0);
    }

    #[test]
    fn cfg_two_blocks_with_goto() {
        let blocks = vec![
            block(0, AmirTerminator::Goto(BlockId::from_usize(1))),
            block(1, AmirTerminator::Return),
        ];
        let cfg = compute_cfg_edges(&blocks);
        assert_eq!(
            cfg.successors[cfg.successor_ranges[BlockId::from_usize(0)].as_range()],
            vec![BlockId::from_usize(1)]
        );
        assert_eq!(
            cfg.predecessors[cfg.predecessor_ranges[BlockId::from_usize(1)].as_range()],
            vec![BlockId::from_usize(0)]
        );
    }

    #[test]
    fn cfg_branch_has_two_successors() {
        let cond = AmirOperand::Constant(AmirConstant::Bool(true));
        let blocks = vec![
            block(
                0,
                AmirTerminator::Branch {
                    condition: cond,
                    if_true: BlockId::from_usize(1),
                    if_false: BlockId::from_usize(2),
                },
            ),
            block(1, AmirTerminator::Return),
            block(2, AmirTerminator::Return),
        ];
        let cfg = compute_cfg_edges(&blocks);
        let b0_succs: Vec<BlockId> =
            cfg.successors[cfg.successor_ranges[BlockId::from_usize(0)].as_range()].to_vec();
        assert_eq!(b0_succs.len(), 2);
        assert!(b0_succs.contains(&BlockId::from_usize(1)));
        assert!(b0_succs.contains(&BlockId::from_usize(2)));
        for target in &[BlockId::from_usize(1), BlockId::from_usize(2)] {
            let preds: Vec<BlockId> =
                cfg.predecessors[cfg.predecessor_ranges[*target].as_range()].to_vec();
            assert_eq!(preds, vec![BlockId::from_usize(0)]);
        }
    }

    #[test]
    fn cfg_switch_int_multiple_targets() {
        let disc = AmirOperand::Constant(AmirConstant::Bool(false));
        let targets = vec![
            (1i128, BlockId::from_usize(1)),
            (2i128, BlockId::from_usize(2)),
        ];
        let blocks = vec![
            block(
                0,
                AmirTerminator::SwitchInt {
                    discriminant: disc,
                    targets,
                    otherwise: BlockId::from_usize(3),
                },
            ),
            block(1, AmirTerminator::Return),
            block(2, AmirTerminator::Return),
            block(3, AmirTerminator::Return),
        ];
        let cfg = compute_cfg_edges(&blocks);
        let b0_succs: Vec<BlockId> =
            cfg.successors[cfg.successor_ranges[BlockId::from_usize(0)].as_range()].to_vec();
        assert_eq!(b0_succs.len(), 3);
        assert!(b0_succs.contains(&BlockId::from_usize(1)));
        assert!(b0_succs.contains(&BlockId::from_usize(2)));
        assert!(b0_succs.contains(&BlockId::from_usize(3)));
    }

    #[test]
    fn cfg_unreachable_has_no_successors() {
        let blocks = vec![
            block(0, AmirTerminator::Unreachable),
            block(1, AmirTerminator::Return),
        ];
        let cfg = compute_cfg_edges(&blocks);
        assert_eq!(cfg.successor_ranges[BlockId::from_usize(0)].len, 0);
        assert_eq!(cfg.predecessor_ranges[BlockId::from_usize(0)].len, 0);
        assert_eq!(cfg.predecessor_ranges[BlockId::from_usize(1)].len, 0);
    }

    #[test]
    fn cfg_out_of_bounds_target_is_skipped() {
        let blocks = vec![block(0, AmirTerminator::Goto(BlockId::from_usize(5)))];
        let cfg = compute_cfg_edges(&blocks);
        assert_eq!(cfg.successor_ranges[BlockId::from_usize(0)].len, 0);
    }

    #[test]
    fn cfg_diamond_merge() {
        let cond = AmirOperand::Constant(AmirConstant::Bool(true));
        let blocks = vec![
            block(
                0,
                AmirTerminator::Branch {
                    condition: cond,
                    if_true: BlockId::from_usize(1),
                    if_false: BlockId::from_usize(2),
                },
            ),
            block(1, AmirTerminator::Goto(BlockId::from_usize(3))),
            block(2, AmirTerminator::Goto(BlockId::from_usize(3))),
            block(3, AmirTerminator::Return),
        ];
        let cfg = compute_cfg_edges(&blocks);
        let b3_preds: Vec<BlockId> =
            cfg.predecessors[cfg.predecessor_ranges[BlockId::from_usize(3)].as_range()].to_vec();
        assert_eq!(b3_preds.len(), 2);
        assert!(b3_preds.contains(&BlockId::from_usize(1)));
        assert!(b3_preds.contains(&BlockId::from_usize(2)));
    }
}
