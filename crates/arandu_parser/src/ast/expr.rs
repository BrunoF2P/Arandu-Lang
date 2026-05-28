use super::{Block, TypeExpr};
use arandu_lexer::Span;

pub use super::ast_pool::ExprId as Expr;
pub use super::ast_pool::ExprId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
    Await,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq)]
pub struct FieldInit {
    pub span: Span,
    pub name: String,
    pub value: ExprId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LambdaParam {
    pub span: Span,
    pub name: String,
    pub ty: Option<TypeExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LambdaBody {
    Expr { span: Span, expr: ExprId },
    Block { span: Span, block: Block },
}

#[derive(Debug, Clone, PartialEq)]
pub enum CatchHandler {
    Expr {
        span: Span,
        expr: ExprId,
    },
    Block {
        span: Span,
        error: String,
        block: Block,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    Text { span: Span, text: String },
    Expr { span: Span, expr: ExprId },
}
