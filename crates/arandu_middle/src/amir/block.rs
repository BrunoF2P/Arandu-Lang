use super::local::{LocalId, TempId};
use super::stmt::AmirTerminator;
use crate::types::ArType;

use crate::DenseRange;
use crate::newtype_index;

newtype_index!(BlockId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockParam {
    pub id: TempId,
    pub local: LocalId,
    pub ty: ArType,
    pub from: Option<String>,
    pub moved: bool,
}

#[derive(Debug, Clone)]
pub struct AmirBasicBlock {
    pub id: BlockId,
    pub params: Vec<BlockParam>,
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
            params: Vec::new(),
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
            params: Vec::new(),
            statements: DenseRange::empty(),
            terminator: AmirTerminator::Unreachable,
        };
        assert!(b.statements.is_empty());
    }
}
