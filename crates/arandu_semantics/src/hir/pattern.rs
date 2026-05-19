//! HIR pattern types (B7): stable IR decoupled from parser `Pattern`.

use super::HirExpr;
use crate::SymbolId;
use arandu_lexer::Span;

#[derive(Debug)]
#[non_exhaustive]
pub enum HirPattern {
    Wildcard {
        span: Span,
    },
    Bind {
        span: Span,
        name: String,
    },
    Literal {
        span: Span,
        expr: Box<HirExpr>,
    },
    Enum {
        span: Span,
        type_symbol: SymbolId,
        variant: String,
        variant_symbol: Option<SymbolId>,
        payload: Vec<HirPattern>,
    },
    TypeTuple {
        span: Span,
        name: String,
        payload: Vec<HirPattern>,
    },
    Struct {
        span: Span,
        struct_symbol: SymbolId,
        fields: Vec<HirFieldPattern>,
    },
    Tuple {
        span: Span,
        items: Vec<HirPattern>,
    },
    Range {
        span: Span,
        start: Box<HirExpr>,
        inclusive: bool,
        end: Box<HirExpr>,
    },
}

#[derive(Debug)]
pub struct HirFieldPattern {
    pub span: Span,
    pub name: String,
    pub pattern: Option<Box<HirPattern>>,
}

impl HirPattern {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Wildcard { span }
            | Self::Bind { span, .. }
            | Self::Literal { span, .. }
            | Self::Enum { span, .. }
            | Self::TypeTuple { span, .. }
            | Self::Struct { span, .. }
            | Self::Tuple { span, .. }
            | Self::Range { span, .. } => *span,
        }
    }
}
