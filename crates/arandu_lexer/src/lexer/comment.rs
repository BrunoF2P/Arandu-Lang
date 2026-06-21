use super::Lexer;
use crate::{LexError, LexErrorCode, TokenKind};

impl<'a> Lexer<'a> {
    pub(super) fn lex_line_doc_comment(&mut self) {
        let start = self.mark();
        while !self.is_at_end() && !matches!(self.peek(), Some('\n' | '\r')) {
            self.bump();
        }
        self.push_token(TokenKind::DocComment, self.span_from(start), false);
    }

    pub(super) fn lex_block_doc_comment(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        self.bump_ascii(3); // /**
        let mut depth = 1;
        while !self.is_at_end() && depth > 0 {
            if self.starts_with("/*") {
                self.bump_ascii(2);
                depth += 1;
            } else if self.starts_with("*/") {
                self.bump_ascii(2);
                depth -= 1;
            } else {
                self.bump();
            }
        }
        if depth > 0 {
            return Err(self.error_from(
                start,
                LexErrorCode::UnterminatedBlockComment,
                "unterminated doc block comment",
            ));
        }
        self.push_token(TokenKind::DocComment, self.span_from(start), false);
        Ok(())
    }

    pub(super) fn skip_line_comment(&mut self) {
        while !self.is_at_end() && !matches!(self.peek(), Some('\n' | '\r')) {
            self.bump();
        }
    }

    pub(super) fn skip_block_comment(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        self.bump_ascii(2); // /*
        let mut depth = 1;
        while !self.is_at_end() && depth > 0 {
            if self.starts_with("/*") {
                self.bump_ascii(2);
                depth += 1;
            } else if self.starts_with("*/") {
                self.bump_ascii(2);
                depth -= 1;
            } else {
                self.bump();
            }
        }
        if depth > 0 {
            return Err(self.error_from(
                start,
                LexErrorCode::UnterminatedBlockComment,
                "unterminated block comment",
            ));
        }
        Ok(())
    }
}
