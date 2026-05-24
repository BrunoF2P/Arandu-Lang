use super::block::BlockId;
use super::local::TempId;
use super::value::{AmirOperand, AmirPlace, AmirRvalue};
use smallvec::SmallVec;

#[derive(Debug)]
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

#[derive(Debug)]
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
