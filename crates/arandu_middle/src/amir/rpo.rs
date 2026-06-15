use super::{AmirFunc, BlockId};

#[must_use]
pub fn reverse_post_order(func: &AmirFunc) -> Vec<BlockId> {
    let n = func.blocks.len();
    if n == 0 {
        return Vec::new();
    }

    let entry = BlockId::from_usize(0);
    let mut visited = vec![false; n];
    let mut post_order = Vec::with_capacity(n);
    post_order_walk(entry, func, &mut visited, &mut post_order);
    post_order.into_iter().rev().collect()
}

fn post_order_walk(
    block_id: BlockId,
    func: &AmirFunc,
    visited: &mut [bool],
    post_order: &mut Vec<BlockId>,
) {
    let idx = block_id.as_usize();
    if idx >= visited.len() || visited[idx] {
        return;
    }
    visited[idx] = true;

    for &succ in func.successors(block_id) {
        post_order_walk(succ, func, visited, post_order);
    }
    post_order.push(block_id);
}
