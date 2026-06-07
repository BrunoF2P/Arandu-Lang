use super::stmt::AmirTerminator;

use crate::DenseRange;
use crate::newtype_index;

newtype_index!(BlockId);

#[derive(Debug)]
pub struct AmirBasicBlock {
    pub id: BlockId,
    pub statements: DenseRange,
    pub terminator: AmirTerminator,
    pub successors: Vec<BlockId>,
    pub predecessors: Vec<BlockId>,
}
