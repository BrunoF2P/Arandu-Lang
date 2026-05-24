mod error;
mod lexer;
mod token;

pub use error::{LexError, LexErrorCode};
pub use lexer::Lexer;
pub use token::{Span, Token, TokenKind};

/// Lexes source, stopping at the first error.
///
/// # Errors
///
/// Returns the first [`LexError`] if the source contains invalid tokens.
pub fn lex<'a>(source: &'a str) -> Result<Lexed<'a>, LexError> {
    Lexer::new(source).lex()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lexed<'a> {
    pub source: &'a str,
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<LexError>,
}

#[must_use]
pub fn lex_recovering<'a>(source: &'a str) -> Lexed<'a> {
    Lexer::new(source).lex_recovering()
}

/// Lexes source and returns a newline-separated token dump.
///
/// # Errors
///
/// Returns the first [`LexError`] if the source contains invalid tokens.
pub fn lex_to_string(source: &str) -> Result<String, LexError> {
    let lexed = lex(source)?;
    let mut out = String::new();
    for (i, token) in lexed.tokens.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&token.dump(lexed.source));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_keywords_and_primitive_types() {
        let dump = lex_to_string("func main() int { return 1 }").unwrap();
        assert!(dump.contains("KW_FUNC"));
        assert!(dump.contains("TYPE_INT"));
        assert!(dump.contains("KW_RETURN"));
    }

    #[test]
    fn reports_unterminated_string() {
        let err = lex("\"open").unwrap_err();
        assert_eq!(err.code, LexErrorCode::UnterminatedString);
    }

    #[test]
    fn reports_empty_char() {
        let err = lex("''").unwrap_err();
        assert_eq!(err.code, LexErrorCode::EmptyChar);
    }

    #[test]
    fn reports_invalid_escape() {
        let err = lex("\"\\q\"").unwrap_err();
        assert_eq!(err.code, LexErrorCode::InvalidEscape);
    }

    #[test]
    fn reports_unterminated_block_comment() {
        let err = lex("/* open").unwrap_err();
        assert_eq!(err.code, LexErrorCode::UnterminatedBlockComment);
    }

    #[test]
    fn reports_invalid_binary_digit() {
        let err = lex("0b102").unwrap_err();
        assert_eq!(err.code, LexErrorCode::InvalidBinaryDigit);
    }

    #[test]
    fn reports_invalid_unicode_escape_in_char() {
        let err = lex("'\\u{}'").unwrap_err();
        assert_eq!(err.code, LexErrorCode::InvalidUnicodeEscape);
    }
}
