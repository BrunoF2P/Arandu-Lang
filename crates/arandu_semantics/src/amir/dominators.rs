use super::block::BlockId;
use super::program::AmirFunc;

pub struct Dominators {
    // Maps each BlockId (by its index) to its immediate dominator (idom).
    // The entry block dominates itself, so idoms[entry] = Some(entry).
    // Unreachable blocks will have None as their idom.
    idoms: Vec<Option<BlockId>>,
}

impl Dominators {
    #[must_use]
    pub fn new(func: &AmirFunc) -> Self {
        let n = func.blocks.len();
        let mut idoms = vec![None; n];

        if n == 0 {
            return Self { idoms };
        }

        let entry = BlockId::from_usize(0);
        idoms[entry.as_usize()] = Some(entry);

        // Step 1: Compute post-order of reachable blocks via DFS
        let mut visited = vec![false; n];
        let mut post_order = Vec::with_capacity(n);
        post_order_walk(entry, func, &mut visited, &mut post_order);

        // Map BlockId to its index in the post_order vector
        let mut post_order_indices = vec![0; n];
        for (idx, &block_id) in post_order.iter().enumerate() {
            post_order_indices[block_id.as_usize()] = idx;
        }

        // Reverse post-order for iteration (excluding entry)
        let mut rpo: Vec<BlockId> = post_order.into_iter().rev().collect();
        if let Some(pos) = rpo.iter().position(|&b| b == entry) {
            rpo.remove(pos);
        }

        // Step 2: Iterate until convergence (Cooper-Harvey-Kennedy)
        let mut changed = true;
        while changed {
            changed = false;
            for &b in &rpo {
                let block = &func.blocks[b.as_usize()];

                // Find the first predecessor that has its dominator already set
                let mut processed_pred = None;
                for &p in &block.predecessors {
                    if idoms[p.as_usize()].is_some() {
                        processed_pred = Some(p);
                        break;
                    }
                }

                if let Some(mut new_idom) = processed_pred {
                    for &p in &block.predecessors {
                        if p != new_idom && idoms[p.as_usize()].is_some() {
                            new_idom = intersect(p, new_idom, &post_order_indices, &idoms);
                        }
                    }

                    let b_idx = b.as_usize();
                    if idoms[b_idx] != Some(new_idom) {
                        idoms[b_idx] = Some(new_idom);
                        changed = true;
                    }
                }
            }
        }

        Self { idoms }
    }

    /// Returns the immediate dominator of the given block, if it is reachable.
    #[must_use]
    pub fn immediate_dominator(&self, block: BlockId) -> Option<BlockId> {
        self.idoms.get(block.as_usize()).copied().flatten()
    }

    /// Returns true if block `a` dominates block `b`.
    #[must_use]
    pub fn dominates(&self, a: BlockId, b: BlockId) -> bool {
        if a == b {
            return true;
        }
        let mut curr = b;
        while let Some(idom) = self.immediate_dominator(curr) {
            if idom == curr {
                break;
            }
            if idom == a {
                return true;
            }
            curr = idom;
        }
        false
    }
}

fn post_order_walk(
    block_id: BlockId,
    func: &AmirFunc,
    visited: &mut Vec<bool>,
    post_order: &mut Vec<BlockId>,
) {
    let idx = block_id.as_usize();
    if visited[idx] {
        return;
    }
    visited[idx] = true;

    if let Some(block) = func.blocks.get(idx) {
        for &succ in &block.successors {
            post_order_walk(succ, func, visited, post_order);
        }
    }
    post_order.push(block_id);
}

fn intersect(
    mut b1: BlockId,
    mut b2: BlockId,
    post_order_indices: &[usize],
    idoms: &[Option<BlockId>],
) -> BlockId {
    while b1 != b2 {
        while post_order_indices[b1.as_usize()] < post_order_indices[b2.as_usize()] {
            b1 = idoms[b1.as_usize()].expect("dominator not set in intersection");
        }
        while post_order_indices[b2.as_usize()] < post_order_indices[b1.as_usize()] {
            b2 = idoms[b2.as_usize()].expect("dominator not set in intersection");
        }
    }
    b1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SymbolId;
    use crate::amir::block::AmirBasicBlock;
    use crate::amir::stmt::AmirTerminator;
    use crate::passes::type_checker::types::ArType;

    fn make_block(id: usize, predecessors: &[usize], successors: &[usize]) -> AmirBasicBlock {
        AmirBasicBlock {
            id: BlockId::from_usize(id),
            statements: Vec::new(),
            terminator: AmirTerminator::Return,
            successors: successors.iter().map(|&x| BlockId::from_usize(x)).collect(),
            predecessors: predecessors
                .iter()
                .map(|&x| BlockId::from_usize(x))
                .collect(),
        }
    }

    fn make_func(blocks: Vec<AmirBasicBlock>) -> AmirFunc {
        AmirFunc {
            symbol: SymbolId(0),
            return_type: ArType::Void,
            receiver: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks,
        }
    }

    #[test]
    fn test_simple_diamond() {
        // 0 -> 1, 2
        // 1 -> 3
        // 2 -> 3
        // 3 -> 4
        let blocks = vec![
            make_block(0, &[], &[1, 2]),
            make_block(1, &[0], &[3]),
            make_block(2, &[0], &[3]),
            make_block(3, &[1, 2], &[4]),
            make_block(4, &[3], &[]),
        ];
        let func = make_func(blocks);
        let doms = Dominators::new(&func);

        assert_eq!(
            doms.immediate_dominator(BlockId::from_usize(0)),
            Some(BlockId::from_usize(0))
        );
        assert_eq!(
            doms.immediate_dominator(BlockId::from_usize(1)),
            Some(BlockId::from_usize(0))
        );
        assert_eq!(
            doms.immediate_dominator(BlockId::from_usize(2)),
            Some(BlockId::from_usize(0))
        );
        assert_eq!(
            doms.immediate_dominator(BlockId::from_usize(3)),
            Some(BlockId::from_usize(0))
        );
        assert_eq!(
            doms.immediate_dominator(BlockId::from_usize(4)),
            Some(BlockId::from_usize(3))
        );

        assert!(doms.dominates(BlockId::from_usize(0), BlockId::from_usize(3)));
        assert!(!doms.dominates(BlockId::from_usize(1), BlockId::from_usize(3)));
        assert!(!doms.dominates(BlockId::from_usize(2), BlockId::from_usize(3)));
        assert!(doms.dominates(BlockId::from_usize(3), BlockId::from_usize(4)));
        assert!(doms.dominates(BlockId::from_usize(0), BlockId::from_usize(4)));
    }

    #[test]
    fn test_loop_cfg() {
        // 0 -> 1
        // 1 -> 2, 5
        // 2 -> 3
        // 3 -> 4
        // 4 -> 1
        // 5 -> 6
        let blocks = vec![
            make_block(0, &[], &[1]),
            make_block(1, &[0, 4], &[2, 5]),
            make_block(2, &[1], &[3]),
            make_block(3, &[2], &[4]),
            make_block(4, &[3], &[1]),
            make_block(5, &[1], &[6]),
            make_block(6, &[5], &[]),
        ];
        let func = make_func(blocks);
        let doms = Dominators::new(&func);

        assert_eq!(
            doms.immediate_dominator(BlockId::from_usize(0)),
            Some(BlockId::from_usize(0))
        );
        assert_eq!(
            doms.immediate_dominator(BlockId::from_usize(1)),
            Some(BlockId::from_usize(0))
        );
        assert_eq!(
            doms.immediate_dominator(BlockId::from_usize(2)),
            Some(BlockId::from_usize(1))
        );
        assert_eq!(
            doms.immediate_dominator(BlockId::from_usize(3)),
            Some(BlockId::from_usize(2))
        );
        assert_eq!(
            doms.immediate_dominator(BlockId::from_usize(4)),
            Some(BlockId::from_usize(3))
        );
        assert_eq!(
            doms.immediate_dominator(BlockId::from_usize(5)),
            Some(BlockId::from_usize(1))
        );
        assert_eq!(
            doms.immediate_dominator(BlockId::from_usize(6)),
            Some(BlockId::from_usize(5))
        );
    }

    #[test]
    fn test_unreachable_block() {
        // 0 -> 1
        // 2 -> 3 (unreachable)
        let blocks = vec![
            make_block(0, &[], &[1]),
            make_block(1, &[0], &[]),
            make_block(2, &[], &[3]),
            make_block(3, &[2], &[]),
        ];
        let func = make_func(blocks);
        let doms = Dominators::new(&func);

        assert_eq!(
            doms.immediate_dominator(BlockId::from_usize(0)),
            Some(BlockId::from_usize(0))
        );
        assert_eq!(
            doms.immediate_dominator(BlockId::from_usize(1)),
            Some(BlockId::from_usize(0))
        );
        assert_eq!(doms.immediate_dominator(BlockId::from_usize(2)), None);
        assert_eq!(doms.immediate_dominator(BlockId::from_usize(3)), None);
    }
}
