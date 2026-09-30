use std::fmt;

use arandu_base::source_registry::SourceRegistry;
use arandu_base::span::Span;
use arandu_lexer::Token;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub code: ParseErrorCode,
    pub message: Box<str>,
    pub span: Span,
    pub found: Box<str>,
    pub expected: &'static [&'static str],
}

impl ParseError {
    #[cold]
    #[inline(never)]
    #[tracing::instrument(
        level = "trace",
        target = "arandu_parser",
        skip(source, token, message)
    )]
    pub(super) fn new(
        code: ParseErrorCode,
        message: impl Into<String>,
        token: &Token,
        file_id: u32,
        source: &str,
    ) -> Self {
        Self {
            code,
            message: message.into().into_boxed_str(),
            span: token.span(file_id),
            found: token
                .kind
                .user_facing_display(token, source)
                .into_boxed_str(),
            expected: &[],
        }
    }

    #[cold]
    #[inline(never)]
    pub(super) fn expected(
        code: ParseErrorCode,
        message: impl Into<String>,
        token: &Token,
        file_id: u32,
        source: &str,
        expected: &'static [&'static str],
    ) -> Self {
        Self {
            code,
            message: message.into().into_boxed_str(),
            span: token.span(file_id),
            found: token
                .kind
                .user_facing_display(token, source)
                .into_boxed_str(),
            expected,
        }
    }

    pub(crate) fn from_lex(err: arandu_lexer::LexError, file_id: u32) -> Self {
        let mut span = err.span;
        span.file_id = file_id;
        Self {
            code: ParseErrorCode::Lex,
            message: Box::from(err.message),
            span,
            found: format!("{:?}", err.code).into_boxed_str(),
            expected: &[],
        }
    }
    #[must_use]
    pub fn format_for_cli(&self, registry: &SourceRegistry) -> String {
        let diag = arandu_diagnostics::Diagnostic::from(self.clone());
        diag.format_for_cli(registry)
    }
}

impl From<ParseError> for arandu_diagnostics::Diagnostic {
    fn from(err: ParseError) -> Self {
        let diag_code = match err.code {
            ParseErrorCode::Lex => match &*err.found {
                "BidiTrojanSource" => arandu_diagnostics::DiagCode::LX004BidiTrojanSource,
                "InvalidChar" => arandu_diagnostics::DiagCode::LX002InvalidUnicodeChar,
                "UnterminatedString"
                | "UnterminatedMultilineString"
                | "UnterminatedRawString"
                | "UnterminatedChar"
                | "UnterminatedBlockComment"
                | "UnclosedInterpolation" => arandu_diagnostics::DiagCode::LX001UnterminatedString,
                "InvalidNumericLiteral"
                | "InvalidBinaryDigit"
                | "InvalidOctalDigit"
                | "InvalidHexDigit"
                | "LeadingZero" => arandu_diagnostics::DiagCode::LX003InvalidNumericLiteral,
                _ => arandu_diagnostics::DiagCode::LX002InvalidUnicodeChar,
            },
            ParseErrorCode::ExpectedToken => arandu_diagnostics::DiagCode::P001UnexpectedToken,
            ParseErrorCode::ExpectedTopLevelDecl => {
                arandu_diagnostics::DiagCode::P001UnexpectedToken
            }
            ParseErrorCode::ExpectedExpression => {
                arandu_diagnostics::DiagCode::P005ExpectedExpression
            }
            ParseErrorCode::ExpectedType => arandu_diagnostics::DiagCode::P001UnexpectedToken,
            ParseErrorCode::ExpectedPlace => arandu_diagnostics::DiagCode::P001UnexpectedToken,
            ParseErrorCode::InvalidResultReturn => {
                arandu_diagnostics::DiagCode::P001UnexpectedToken
            }
        };
        let msg = user_facing_message(&err);
        let primary_label =
            if err.code != ParseErrorCode::Lex && err.code != ParseErrorCode::InvalidResultReturn {
                let found = if err.found.as_ref() == "end of file" {
                    "end of file".to_string()
                } else {
                    format!("`{}`", err.found)
                };
                Some(format!("unexpected {found}"))
            } else {
                None
            };
        arandu_diagnostics::Diagnostic::error(diag_code, msg, err.span)
            .with_primary_label_opt(primary_label)
    }
}

fn user_facing_message(err: &ParseError) -> String {
    if err.code == ParseErrorCode::Lex || err.code == ParseErrorCode::InvalidResultReturn {
        return err.message.to_string();
    }

    if err.code == ParseErrorCode::ExpectedTopLevelDecl
        && err.message.as_ref() != "expected top-level declaration"
    {
        return err.message.to_string();
    }

    let expectation = match err.code {
        ParseErrorCode::ExpectedTopLevelDecl => "a declaration".to_string(),
        ParseErrorCode::ExpectedExpression if err.message.as_ref() == "expected expression" => {
            "an expression".to_string()
        }
        ParseErrorCode::ExpectedExpression
            if err.message.as_ref() == "expected type-qualified expression or struct literal" =>
        {
            "an expression or struct literal".to_string()
        }
        ParseErrorCode::ExpectedExpression => {
            err.message.trim_start_matches("expected ").to_string()
        }
        ParseErrorCode::ExpectedType if err.message.as_ref() == "expected type" => {
            "a type".to_string()
        }
        ParseErrorCode::ExpectedType => err.message.trim_start_matches("expected ").to_string(),
        ParseErrorCode::ExpectedPlace => {
            "a variable, field, or indexed value on the left side of an assignment".to_string()
        }
        ParseErrorCode::ExpectedToken => match err.message.as_ref() {
            "expected identifier" => "a name".to_string(),
            "expected value identifier" => "a variable or function name".to_string(),
            "expected type identifier" => "a type name".to_string(),
            "expected member name" => "a member name".to_string(),
            "expected module path segment" => "a module name".to_string(),
            "expected static ABI string" => "a string literal naming the ABI".to_string(),
            "expected assignment operator" => "an assignment operator".to_string(),
            "expected array size" => "an array size".to_string(),
            "expected string part" => "a string segment".to_string(),
            "expected string content" => "string content".to_string(),
            "expected import identifier" => "an imported name".to_string(),
            message => {
                let phrase = message.trim_start_matches("expected ");
                if err.expected.len() == 1 && err.expected[0] == phrase {
                    format!("`{phrase}`")
                } else {
                    phrase.to_string()
                }
            }
        },
        ParseErrorCode::Lex | ParseErrorCode::InvalidResultReturn => err.message.to_string(),
    };
    let found = if err.found.as_ref() == "end of file" {
        "the end of the file".to_string()
    } else {
        format!("`{}`", err.found)
    };
    format!("Expected {expectation} here, but found {found}.")
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let registry = SourceRegistry::default();
        f.write_str(&self.format_for_cli(&registry))
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
    /// Tuple error-return syntax; use `Result<T, E>` instead.
    InvalidResultReturn,
}
