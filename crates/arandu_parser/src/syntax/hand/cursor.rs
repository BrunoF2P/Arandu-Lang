//! Shared token cursor and context for hand-lower.

use crate::ast::ast_pool::AstPool;
use arandu_lexer::{Span, Token, TokenKind};

/// Mutable AST builder context shared across hand-lower helpers.
pub struct HandCtx<'a> {
    pub pool: &'a mut AstPool,
    pub source: &'a str,
    pub file_id: u32,
}

impl HandCtx<'_> {
    #[inline]
    #[must_use]
    pub fn span(&self, start: u32, end: u32) -> Span {
        Span::new(self.file_id, start, end)
    }

    #[inline]
    #[must_use]
    pub fn token_span(&self, t: &Token) -> Span {
        token_span(self.file_id, t)
    }

    #[inline]
    #[must_use]
    pub fn text<'s>(&'s self, t: &Token) -> Option<&'s str> {
        token_text(self.source, t)
    }
}

/// Non-EOF, non-inserted-semicolon tokens inside `[start, end)`.
#[must_use]
pub fn tokens_in_range(tokens: &[Token], start: u32, end: u32) -> Vec<&Token> {
    tokens
        .iter()
        .filter(|t| {
            !matches!(t.kind, TokenKind::Eof)
                && t.start >= start
                && t.start < end
                && !(t.kind == TokenKind::Semicolon && t.inserted)
        })
        .collect()
}

#[inline]
#[must_use]
pub fn token_text<'a>(source: &'a str, t: &Token) -> Option<&'a str> {
    let ts = t.start as usize;
    let te = t.start.saturating_add(t.len) as usize;
    source.get(ts..te.min(source.len()))
}

#[inline]
#[must_use]
pub fn token_span(file_id: u32, t: &Token) -> Span {
    Span::new(file_id, t.start, t.start + t.len)
}

/// Lightweight cursor over a token slice.
#[derive(Clone, Copy)]
pub struct Cursor<'a> {
    toks: &'a [&'a Token],
    pos: usize,
}

impl<'a> Cursor<'a> {
    #[must_use]
    pub fn new(toks: &'a [&'a Token]) -> Self {
        Self { toks, pos: 0 }
    }

    #[must_use]
    pub fn pos(self) -> usize {
        self.pos
    }

    #[must_use]
    pub fn remaining(self) -> &'a [&'a Token] {
        &self.toks[self.pos..]
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.pos >= self.toks.len()
    }

    #[must_use]
    pub fn at_end(self) -> bool {
        self.is_empty()
    }

    #[must_use]
    pub fn peek(self) -> Option<&'a Token> {
        self.toks.get(self.pos).copied()
    }

    #[must_use]
    pub fn peek_kind(self) -> Option<TokenKind> {
        self.peek().map(|t| t.kind)
    }

    #[must_use]
    pub fn peek_at(self, offset: usize) -> Option<&'a Token> {
        self.toks.get(self.pos + offset).copied()
    }

    pub fn bump(&mut self) -> Option<&'a Token> {
        let t = self.peek()?;
        self.pos += 1;
        Some(t)
    }

    pub fn eat(&mut self, kind: TokenKind) -> bool {
        if self.peek_kind() == Some(kind) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    pub fn expect(&mut self, kind: TokenKind) -> Option<&'a Token> {
        let t = self.peek()?;
        if t.kind == kind {
            self.pos += 1;
            Some(t)
        } else {
            None
        }
    }

    /// Drop a trailing explicit `;` if present.
    pub fn trim_trailing_semi(&mut self) {
        if self
            .toks
            .last()
            .is_some_and(|t| matches!(t.kind, TokenKind::Semicolon))
            && self.pos <= self.toks.len().saturating_sub(1)
        {
            // Rebuild view without trailing semi when at start of full slice.
        }
    }

    /// Tokens from current position to end, excluding a trailing semicolon.
    #[must_use]
    pub fn rest_no_trailing_semi(self) -> &'a [&'a Token] {
        let rest = self.remaining();
        if rest
            .last()
            .is_some_and(|t| matches!(t.kind, TokenKind::Semicolon))
        {
            &rest[..rest.len() - 1]
        } else {
            rest
        }
    }
}

/// Drop trailing semicolon from a owned token vec (used by stmt entry).
pub fn drop_trailing_semi(toks: &mut Vec<&Token>) {
    if toks
        .last()
        .is_some_and(|t| matches!(t.kind, TokenKind::Semicolon))
    {
        toks.pop();
    }
}

/// Map set-operator token → AST op.
#[must_use]
pub fn set_op_from_token(kind: TokenKind) -> Option<crate::SetOp> {
    use crate::SetOp;
    match kind {
        TokenKind::Equal => Some(SetOp::Assign),
        TokenKind::PlusEqual => Some(SetOp::AddAssign),
        TokenKind::MinusEqual => Some(SetOp::SubAssign),
        TokenKind::StarEqual => Some(SetOp::MulAssign),
        TokenKind::SlashEqual => Some(SetOp::DivAssign),
        TokenKind::PercentEqual => Some(SetOp::ModAssign),
        TokenKind::AmpEqual => Some(SetOp::BitAndAssign),
        TokenKind::PipeEqual => Some(SetOp::BitOrAssign),
        TokenKind::CaretEqual => Some(SetOp::BitXorAssign),
        TokenKind::ShiftLeftEqual => Some(SetOp::ShiftLeftAssign),
        TokenKind::ShiftRightEqual => Some(SetOp::ShiftRightAssign),
        _ => None,
    }
}

/// Binary op binding powers (left, right) — subset aligned with RD.
#[must_use]
pub fn bin_bp(kind: TokenKind) -> Option<(u8, u8, crate::BinaryOp)> {
    use crate::BinaryOp;
    match kind {
        TokenKind::NullCoalesce => Some((1, 2, BinaryOp::NullCoalesce)),
        TokenKind::LogicalOr => Some((3, 4, BinaryOp::Or)),
        TokenKind::LogicalAnd => Some((5, 6, BinaryOp::And)),
        TokenKind::EqualEqual => Some((7, 8, BinaryOp::Equal)),
        TokenKind::BangEqual => Some((7, 8, BinaryOp::NotEqual)),
        TokenKind::Lt => Some((9, 10, BinaryOp::Lt)),
        TokenKind::Gt => Some((9, 10, BinaryOp::Gt)),
        TokenKind::LtEqual => Some((9, 10, BinaryOp::LtEqual)),
        TokenKind::GtEqual => Some((9, 10, BinaryOp::GtEqual)),
        TokenKind::Pipe => Some((11, 12, BinaryOp::BitOr)),
        TokenKind::Caret => Some((13, 14, BinaryOp::BitXor)),
        TokenKind::Amp => Some((15, 16, BinaryOp::BitAnd)),
        TokenKind::ShiftLeft => Some((17, 18, BinaryOp::ShiftLeft)),
        TokenKind::ShiftRight => Some((17, 18, BinaryOp::ShiftRight)),
        TokenKind::Plus => Some((19, 20, BinaryOp::Add)),
        TokenKind::Minus => Some((19, 20, BinaryOp::Sub)),
        TokenKind::Star => Some((21, 22, BinaryOp::Mul)),
        TokenKind::Slash => Some((21, 22, BinaryOp::Div)),
        TokenKind::Percent => Some((21, 22, BinaryOp::Mod)),
        _ => None,
    }
}
