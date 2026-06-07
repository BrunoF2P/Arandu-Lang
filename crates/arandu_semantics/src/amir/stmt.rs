use super::block::BlockId;
use super::local::TempId;
use super::value::{AmirOperand, AmirPlace, AmirRvalue};
use crate::index_vec::IndexVec;
use crate::newtype_index;
use smallvec::SmallVec;

newtype_index!(InstrId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmirStmtKind {
    Assign,
    Store,
    Call,
    Free,
    StorageLive,
    StorageDead,
    Destroy,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AmirStmt {
    /// Binds an SSA register to an RValue.
    Assign {
        lhs: TempId,
        rhs: AmirRvalue,
    },
    /// Stores an operand into a memory location (local stack slot or projection).
    Store {
        lhs: AmirPlace,
        rhs: AmirOperand,
    },
    Call {
        lhs: Option<TempId>,
        callee: AmirOperand,
        args: SmallVec<[AmirOperand; 4]>,
    },
    Free(AmirOperand),
    /// Declares that a stack slot's lifetime is active.
    StorageLive(super::local::LocalId),
    /// Declares that a stack slot's lifetime has ended.
    StorageDead(super::local::LocalId),
    /// Explicitly drop/destroy a value at a place.
    Destroy(AmirPlace),
}

impl AmirStmt {
    #[must_use]
    pub const fn kind(&self) -> AmirStmtKind {
        match self {
            Self::Assign { .. } => AmirStmtKind::Assign,
            Self::Store { .. } => AmirStmtKind::Store,
            Self::Call { .. } => AmirStmtKind::Call,
            Self::Free(_) => AmirStmtKind::Free,
            Self::StorageLive(_) => AmirStmtKind::StorageLive,
            Self::StorageDead(_) => AmirStmtKind::StorageDead,
            Self::Destroy(_) => AmirStmtKind::Destroy,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct AmirStmtTable {
    pub kinds: IndexVec<InstrId, AmirStmtKind>,
    pub payloads: IndexVec<InstrId, AmirStmt>,
}

impl AmirStmtTable {
    #[must_use]
    pub fn new() -> Self {
        Self {
            kinds: IndexVec::new(),
            payloads: IndexVec::new(),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.payloads.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.payloads.is_empty()
    }

    pub fn push(&mut self, stmt: AmirStmt) -> InstrId {
        let kind = stmt.kind();
        let id = self.payloads.push(stmt);
        let kind_id = self.kinds.push(kind);
        debug_assert_eq!(id, kind_id);
        id
    }

    #[must_use]
    pub fn get(&self, id: InstrId) -> Option<&AmirStmt> {
        self.payloads.get(id)
    }

    pub fn get_mut(&mut self, id: InstrId) -> Option<&mut AmirStmt> {
        self.payloads.get_mut(id)
    }

    pub fn iter_ids(&self) -> impl Iterator<Item = InstrId> + '_ {
        self.payloads.ids()
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AmirTerminator {
    Return,
    Goto(BlockId),
    /// Boolean conditional branch: if `condition` is true, jump to `if_true`, else `if_false`.
    Branch {
        condition: AmirOperand,
        if_true: BlockId,
        if_false: BlockId,
    },
    /// Integer discriminant switch (e.g. enum tags, `switch` on int).
    SwitchInt {
        discriminant: AmirOperand,
        targets: Vec<(i128, BlockId)>,
        otherwise: BlockId,
    },
    Unreachable,
}
