use std::fmt;

use arandu_lexer::{Span, Token};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub code: ParseErrorCode,
    pub message: Box<str>,
    pub span: Span,
    pub found: Box<str>,
    pub expected: &'static [&'static str],
}

impl ParseError {
    pub(super) fn new(code: ParseErrorCode, message: impl Into<String>, token: &Token) -> Self {
        Self {
            code,
            message: message.into().into_boxed_str(),
            span: token.span,
            found: token.kind.to_string().into_boxed_str(),
            expected: &[],
        }
    }

    pub(super) fn expected(
        code: ParseErrorCode,
        message: impl Into<String>,
        token: &Token,
        expected: &'static [&'static str],
    ) -> Self {
        Self {
            code,
            message: message.into().into_boxed_str(),
            span: token.span,
            found: token.kind.to_string().into_boxed_str(),
            expected,
        }
    }

    pub(super) fn from_lex(err: arandu_lexer::LexError) -> Self {
        Self {
            code: ParseErrorCode::Lex,
            message: err.message.into_boxed_str(),
            span: err.span,
            found: format!("{:?}", err.code).into_boxed_str(),
            expected: &[],
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.expected.is_empty() {
            return write!(
                f,
                "{:?}: {} (found {})",
                self.code, self.message, self.found
            );
        }
        write!(
            f,
            "{:?}: {} (expected {}, found {})",
            self.code,
            self.message,
            self.expected.join(" or "),
            self.found
        )
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorCode {
    Lex,
    ExpectedToken,
    ExpectedTopLevelDecl,
    ExpectedExpression,
    ExpectedType,
    ExpectedPlace,
}
