use super::{Block, Condition, MatchArm, TypeExpr, TypeName};
use arandu_lexer::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Path {
        span: Span,
        path: Vec<String>,
    },
    TypePath {
        span: Span,
        type_name: TypeName,
        member: String,
    },
    Generic {
        span: Span,
        callee: Box<Expr>,
        args: Vec<TypeExpr>,
    },
    Field {
        span: Span,
        base: Box<Expr>,
        field: String,
    },
    SafeField {
        span: Span,
        base: Box<Expr>,
        field: String,
    },
    Index {
        span: Span,
        base: Box<Expr>,
        index: Box<Expr>,
    },
    SafeIndex {
        span: Span,
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Try {
        span: Span,
        expr: Box<Expr>,
    },
    Call {
        span: Span,
        callee: Box<Expr>,
        args: Vec<Expr>,
        trailing_block: Option<Block>,
    },
    StructLiteral {
        span: Span,
        ty: TypeExpr,
        fields: Vec<FieldInit>,
    },
    Array {
        span: Span,
        items: Vec<Expr>,
    },
    Lambda {
        span: Span,
        params: Vec<LambdaParam>,
        body: LambdaBody,
    },
    Alloc {
        span: Span,
        expr: Box<Expr>,
    },
    AsyncBlock {
        span: Span,
        block: Block,
    },
    UnsafeBlock {
        span: Span,
        block: Block,
    },
    If {
        span: Span,
        condition: Box<Condition>,
        then_block: Block,
        else_block: Block,
    },
    Match {
        span: Span,
        value: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    Catch {
        span: Span,
        expr: Box<Expr>,
        handler: CatchHandler,
    },
    NullCoalesce {
        span: Span,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Cast {
        span: Span,
        expr: Box<Expr>,
        ty: TypeExpr,
    },
    Error(Span),
    Group {
        span: Span,
        expr: Box<Expr>,
    },
    Unary {
        span: Span,
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        span: Span,
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Int {
        span: Span,
        value: String,
    },
    Float {
        span: Span,
        value: String,
    },
    Bool {
        span: Span,
        value: bool,
    },
    Char {
        span: Span,
        value: String,
    },
    InterpolatedString {
        span: Span,
        parts: Vec<StringPart>,
    },
    Nil {
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldInit {
    pub span: Span,
    pub name: String,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LambdaParam {
    pub span: Span,
    pub name: String,
    pub ty: Option<TypeExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LambdaBody {
    Expr { span: Span, expr: Box<Expr> },
    Block { span: Span, block: Block },
}

#[derive(Debug, Clone, PartialEq)]
pub enum CatchHandler {
    Expr {
        span: Span,
        expr: Box<Expr>,
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
    Expr { span: Span, expr: Box<Expr> },
}

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

impl Expr {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Expr::Path { span, .. }
            | Expr::TypePath { span, .. }
            | Expr::Generic { span, .. }
            | Expr::Field { span, .. }
            | Expr::SafeField { span, .. }
            | Expr::Index { span, .. }
            | Expr::SafeIndex { span, .. }
            | Expr::Try { span, .. }
            | Expr::Call { span, .. }
            | Expr::StructLiteral { span, .. }
            | Expr::Array { span, .. }
            | Expr::Lambda { span, .. }
            | Expr::Alloc { span, .. }
            | Expr::AsyncBlock { span, .. }
            | Expr::UnsafeBlock { span, .. }
            | Expr::If { span, .. }
            | Expr::Match { span, .. }
            | Expr::Catch { span, .. }
            | Expr::NullCoalesce { span, .. }
            | Expr::Cast { span, .. }
            | Expr::Group { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Int { span, .. }
            | Expr::Float { span, .. }
            | Expr::Bool { span, .. }
            | Expr::Char { span, .. }
            | Expr::InterpolatedString { span, .. }
            | Expr::Nil { span }
            | Expr::Error(span) => *span,
        }
    }
}
