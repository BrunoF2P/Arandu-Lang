//! Stable operator enums shared by HIR and AMIR (decoupled from the parser AST).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
    Await,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BinaryOp {
    Or,
    And,
    Equal,
    NotEqual,
    Lt,
    Gt,
    LtEqual,
    GtEqual,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitOr,
    BitXor,
    BitAnd,
    ShiftLeft,
    ShiftRight,
    NullCoalesce,
    RangeExclusive,
    RangeInclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SetOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    BitAndAssign,
    BitOrAssign,
    BitXorAssign,
    ShiftLeftAssign,
    ShiftRightAssign,
}

impl From<arandu_parser::UnaryOp> for UnaryOp {
    fn from(op: arandu_parser::UnaryOp) -> Self {
        match op {
            arandu_parser::UnaryOp::Neg => Self::Neg,
            arandu_parser::UnaryOp::Not => Self::Not,
            arandu_parser::UnaryOp::BitNot => Self::BitNot,
            arandu_parser::UnaryOp::Await => Self::Await,
        }
    }
}

impl From<arandu_parser::BinaryOp> for BinaryOp {
    fn from(op: arandu_parser::BinaryOp) -> Self {
        match op {
            arandu_parser::BinaryOp::Or => Self::Or,
            arandu_parser::BinaryOp::And => Self::And,
            arandu_parser::BinaryOp::Equal => Self::Equal,
            arandu_parser::BinaryOp::NotEqual => Self::NotEqual,
            arandu_parser::BinaryOp::Lt => Self::Lt,
            arandu_parser::BinaryOp::Gt => Self::Gt,
            arandu_parser::BinaryOp::LtEqual => Self::LtEqual,
            arandu_parser::BinaryOp::GtEqual => Self::GtEqual,
            arandu_parser::BinaryOp::Add => Self::Add,
            arandu_parser::BinaryOp::Sub => Self::Sub,
            arandu_parser::BinaryOp::Mul => Self::Mul,
            arandu_parser::BinaryOp::Div => Self::Div,
            arandu_parser::BinaryOp::Mod => Self::Mod,
            arandu_parser::BinaryOp::BitOr => Self::BitOr,
            arandu_parser::BinaryOp::BitXor => Self::BitXor,
            arandu_parser::BinaryOp::BitAnd => Self::BitAnd,
            arandu_parser::BinaryOp::ShiftLeft => Self::ShiftLeft,
            arandu_parser::BinaryOp::ShiftRight => Self::ShiftRight,
            arandu_parser::BinaryOp::NullCoalesce => Self::NullCoalesce,
            arandu_parser::BinaryOp::RangeExclusive => Self::RangeExclusive,
            arandu_parser::BinaryOp::RangeInclusive => Self::RangeInclusive,
        }
    }
}

impl From<arandu_parser::SetOp> for SetOp {
    fn from(op: arandu_parser::SetOp) -> Self {
        match op {
            arandu_parser::SetOp::Assign => Self::Assign,
            arandu_parser::SetOp::AddAssign => Self::AddAssign,
            arandu_parser::SetOp::SubAssign => Self::SubAssign,
            arandu_parser::SetOp::MulAssign => Self::MulAssign,
            arandu_parser::SetOp::DivAssign => Self::DivAssign,
            arandu_parser::SetOp::ModAssign => Self::ModAssign,
            arandu_parser::SetOp::BitAndAssign => Self::BitAndAssign,
            arandu_parser::SetOp::BitOrAssign => Self::BitOrAssign,
            arandu_parser::SetOp::BitXorAssign => Self::BitXorAssign,
            arandu_parser::SetOp::ShiftLeftAssign => Self::ShiftLeftAssign,
            arandu_parser::SetOp::ShiftRightAssign => Self::ShiftRightAssign,
        }
    }
}
