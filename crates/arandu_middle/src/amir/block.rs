use super::stmt::AmirTerminator;

use crate::DenseRange;
use crate::newtype_index;

newtype_index!(BlockId);

#[derive(Debug)]
pub struct AmirBasicBlock {
    pub id: BlockId,
    pub statements: DenseRange,
    pub terminator: AmirTerminator,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_id_from_usize() {
        let id = BlockId::from_usize(5);
        assert_eq!(id.as_usize(), 5);
    }

    #[test]
    fn basic_block_construction() {
        let b = AmirBasicBlock {
            id: BlockId::from_usize(1),
            statements: DenseRange::empty(),
            terminator: AmirTerminator::Return,
        };
        assert_eq!(b.id.as_usize(), 1);
        assert!(b.statements.is_empty());
    }

    #[test]
    fn unreachable_terminator() {
        let b = AmirBasicBlock {
            id: BlockId::from_usize(2),
            statements: DenseRange::empty(),
            terminator: AmirTerminator::Unreachable,
        };
        assert!(b.statements.is_empty());
    }
}
