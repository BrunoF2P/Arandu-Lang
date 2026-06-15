//! HIR pattern types (B7): stable IR decoupled from parser `Pattern`.

use crate::SymbolId;
use arandu_lexer::Span;

#[derive(Debug, Clone)]
pub enum HirPattern {
    Wildcard {
        span: Span,
    },
    Bind {
        span: Span,
        name: String,
        symbol: SymbolId,
    },
    Literal {
        span: Span,
        expr: super::pool::HirExprId,
    },
    Enum {
        span: Span,
        type_symbol: SymbolId,
        variant: String,
        variant_symbol: Option<SymbolId>,
        payload: super::pool::IndexRange,
    },
    TypeTuple {
        span: Span,
        name: String,
        payload: super::pool::IndexRange,
    },
    Struct {
        span: Span,
        struct_symbol: SymbolId,
        fields: super::pool::IndexRange,
    },
    Tuple {
        span: Span,
        items: super::pool::IndexRange,
    },
    Range {
        span: Span,
        start: super::pool::HirExprId,
        inclusive: bool,
        end: super::pool::HirExprId,
    },
}

#[derive(Debug, Clone)]
pub struct HirFieldPattern {
    pub span: Span,
    pub name: String,
    pub pattern: Option<super::pool::HirPatternId>,
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
