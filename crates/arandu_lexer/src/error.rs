use std::fmt;

use crate::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub code: LexErrorCode,
    pub message: String,
    pub span: Span,
}

impl LexError {
    pub fn new(code: LexErrorCode, message: impl Into<String>, span: Span) -> Self {
        Self {
            code,
            message: message.into(),
            span,
        }
    }
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?}: {} at {}:{}",
            self.code, self.message, self.span.start_line, self.span.start_col
        )
    }
}

impl std::error::Error for LexError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexErrorCode {
    InvalidChar,
    UnterminatedString,
    UnterminatedMultilineString,
    UnterminatedRawString,
    UnterminatedChar,
    EmptyChar,
    CharTooLong,
    InvalidEscape,
    InvalidUnicodeEscape,
    UnterminatedBlockComment,
    InvalidNumericLiteral,
    InvalidBinaryDigit,
    InvalidOctalDigit,
    InvalidHexDigit,
    LeadingZero,
    UnclosedInterpolation,
}
