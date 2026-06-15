use super::local::{LocalId, TempId};
use crate::SymbolId;
use crate::literal_pool::LiteralId;
use crate::ops::{BinaryOp, UnaryOp};
use smallvec::SmallVec;

#[derive(Debug, Clone)]
pub struct AmirPlace {
    pub local: LocalId,
    pub projections: SmallVec<[AmirProjection; 2]>,
}

#[derive(Debug, Clone)]
pub enum AmirProjection {
    Field(SymbolId),
    Index(AmirOperand),
}

#[derive(Debug, Clone)]
pub enum AmirRvalue {
    Use(AmirOperand),
    Binary {
        op: BinaryOp,
        left: AmirOperand,
        right: AmirOperand,
    },
    Unary {
        op: UnaryOp,
        operand: AmirOperand,
    },
    FieldAccess {
        base: AmirOperand,
        field: String,
    },
    StructLiteral {
        struct_symbol: SymbolId,
        fields: Vec<(String, AmirOperand)>,
    },
    IndexAccess {
        base: AmirOperand,
        index: AmirOperand,
    },
    Array {
        items: Vec<AmirOperand>,
    },
    Tuple {
        items: Vec<AmirOperand>,
    },
    Discriminant {
        value: AmirOperand,
    },
    EnumPayload {
        value: AmirOperand,
        variant: SymbolId,
        index: usize,
    },
    Len(AmirOperand),
    Alloc(AmirOperand),
    /// Load value from a stack-allocated local place (memory) into an SSA register.
    Load(AmirPlace),
    /// Create a shared borrow (reference) of a place.
    Borrow(AmirPlace),
    /// Create a mutable borrow (mutable reference) of a place.
    BorrowMut(AmirPlace),
}

#[derive(Debug, Clone)]
pub enum AmirOperand {
    Copy(TempId),
    Move(TempId),
    Constant(AmirConstant),
    FunctionRef(SymbolId),
    GlobalRef(SymbolId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmirConstant {
    Pool(LiteralId),
    Bool(bool),
    Nil,
}
