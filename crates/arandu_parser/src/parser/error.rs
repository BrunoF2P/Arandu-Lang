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
    pub fn format_for_cli(&self, filepath: &str) -> String {
        let diag = arandu_diagnostics::Diagnostic::from(self.clone());
        diag.format_for_cli(filepath)
    }
}

impl From<ParseError> for arandu_diagnostics::Diagnostic {
    fn from(err: ParseError) -> Self {
        let code_str = match err.code {
            ParseErrorCode::Lex => "L001",
            ParseErrorCode::ExpectedToken => "P001",
            ParseErrorCode::ExpectedTopLevelDecl => "P002",
            ParseErrorCode::ExpectedExpression => "P003",
            ParseErrorCode::ExpectedType => "P004",
            ParseErrorCode::ExpectedPlace => "P005",
        };
        let msg = if err.expected.is_empty() {
            format!("{} (found {})", err.message, err.found)
        } else {
            format!(
                "{} (expected {}, found {})",
                err.message,
                err.expected.join(" or "),
                err.found
            )
        };
        arandu_diagnostics::Diagnostic::error(code_str, msg, err.span)
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format_for_cli(""))
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
