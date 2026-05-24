use super::{Block, Expr, TypeName};
use arandu_lexer::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub span: Span,
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: MatchArmBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchArmBody {
    Expr { span: Span, expr: Box<Expr> },
    Block { span: Span, block: Block },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard {
        span: Span,
    },
    Bind {
        span: Span,
        name: String,
    },
    Literal {
        span: Span,
        expr: Box<Expr>,
    },
    Enum {
        span: Span,
        type_name: TypeName,
        variant: String,
        payload: Vec<Pattern>,
    },
    TypeTuple {
        span: Span,
        name: String,
        payload: Vec<Pattern>,
    },
    Struct {
        span: Span,
        type_name: TypeName,
        fields: Vec<FieldPattern>,
    },
    Tuple {
        span: Span,
        items: Vec<Pattern>,
    },
    Range {
        span: Span,
        start: Box<Expr>,
        inclusive: bool,
        end: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldPattern {
    pub span: Span,
    pub name: String,
    pub pattern: Option<Pattern>,
}

impl Pattern {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Pattern::Wildcard { span }
            | Pattern::Bind { span, .. }
            | Pattern::Literal { span, .. }
            | Pattern::Enum { span, .. }
            | Pattern::TypeTuple { span, .. }
            | Pattern::Struct { span, .. }
            | Pattern::Tuple { span, .. }
            | Pattern::Range { span, .. } => *span,
        }
    }
}
