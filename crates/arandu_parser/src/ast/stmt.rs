use super::{Expr, TypeExpr};
use arandu_lexer::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub span: Span,
    pub statements: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    VarDecl {
        span: Span,
        bindings: Vec<BindingItem>,
        value: Expr,
    },
    Set {
        span: Span,
        places: Vec<Place>,
        op: SetOp,
        value: Expr,
    },
    Return {
        span: Span,
        values: Vec<Expr>,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    Free {
        span: Span,
        expr: Expr,
    },
    Expr {
        span: Span,
        expr: Box<Expr>,
    },
    If {
        span: Span,
        condition: Condition,
        then_block: Block,
        else_block: Option<Block>,
    },
    For {
        span: Span,
        clause: Box<ForClause>,
        body: Block,
    },
    While {
        span: Span,
        condition: Condition,
        body: Block,
    },
    Match {
        span: Span,
        expr: Expr,
    },
    Defer {
        span: Span,
        body: DeferBody,
    },
    ErrDefer {
        span: Span,
        body: DeferBody,
    },
    Unsafe {
        span: Span,
        block: Block,
    },
    Error(Span),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    Expr {
        span: Span,
        expr: Box<Expr>,
    },
    Is {
        span: Span,
        expr: Box<Expr>,
        pattern: Box<super::Pattern>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ForClause {
    In {
        span: Span,
        bindings: Vec<ForBinding>,
        iterable: Box<Expr>,
    },
    CStyle {
        span: Span,
        init: Option<Box<SimpleStmt>>,
        condition: Option<Box<Expr>>,
        step: Option<Box<SimpleStmt>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForBinding {
    pub span: Span,
    pub mutable: bool,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SimpleStmt {
    VarDecl {
        span: Span,
        bindings: Vec<BindingItem>,
        value: Expr,
    },
    Set {
        span: Span,
        places: Vec<Place>,
        op: SetOp,
        value: Expr,
    },
    Expr {
        span: Span,
        expr: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeferBody {
    Expr { span: Span, expr: Box<Expr> },
    Block { span: Span, block: Block },
}

#[derive(Debug, Clone, PartialEq)]
pub struct BindingItem {
    pub span: Span,
    pub mutable: bool,
    pub name: String,
    pub ty: Option<TypeExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub span: Span,
    pub root: String,
    pub suffixes: Vec<PlaceSuffix>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaceSuffix {
    Field { span: Span, name: String },
    Index { span: Span, expr: Box<Expr> },
}

#[derive(Debug, Clone, PartialEq)]
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
