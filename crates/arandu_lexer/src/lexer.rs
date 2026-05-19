mod ident;
mod punctuation;

use ident::{is_ident_continue, is_ident_start, keyword_kind};
use punctuation::{peek_kind_from, token_kind_from_prefix};

use crate::{LexError, LexErrorCode, Span, Token, TokenKind};

pub struct Lexer<'a> {
    source: &'a str,
    pos: usize,
    line: usize,
    col: usize,
    tokens: Vec<Token>,
    prev_significant: Option<TokenKind>,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            pos: 0,
            line: 1,
            col: 1,
            tokens: Vec::new(),
            prev_significant: None,
        }
    }

    pub fn lex(mut self) -> Result<Vec<Token>, LexError> {
        while !self.is_at_end() {
            if self.consume_space_or_newline()? {
                continue;
            }

            if self.starts_with("///") {
                self.lex_line_doc_comment();
                continue;
            }
            if self.starts_with("/**") {
                self.lex_block_doc_comment()?;
                continue;
            }
            if self.starts_with("//") {
                self.skip_line_comment();
                continue;
            }
            if self.starts_with("/*") {
                self.skip_block_comment()?;
                continue;
            }

            if self.starts_with("r\"\"\"") {
                self.lex_raw_multiline_string()?;
                continue;
            }
            if self.starts_with("r\"") {
                self.lex_raw_string()?;
                continue;
            }
            if self.starts_with("\"\"\"") {
                self.lex_multiline_string()?;
                continue;
            }
            if self.peek() == Some('"') {
                self.lex_string(false)?;
                continue;
            }
            if self.peek() == Some('\'') {
                self.lex_char()?;
                continue;
            }
            if self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.lex_number()?;
                continue;
            }
            if self.peek().is_some_and(is_ident_start) {
                self.lex_ident_or_keyword();
                continue;
            }

            self.lex_operator_or_punctuation()?;
        }

        self.insert_semicolon_if_needed();
        let span = self.current_span();
        self.push_token(TokenKind::Eof, "", span, false);
        Ok(self.tokens)
    }

    fn consume_space_or_newline(&mut self) -> Result<bool, LexError> {
        match self.peek() {
            Some(' ' | '\t') => {
                self.bump();
                Ok(true)
            }
            Some('\r') if self.peek_next() == Some('\n') => {
                let span = self.current_span();
                self.bump();
                self.bump();
                self.maybe_insert_semicolon_at(span);
                Ok(true)
            }
            Some('\n') => {
                let span = self.current_span();
                self.bump();
                self.maybe_insert_semicolon_at(span);
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn maybe_insert_semicolon_at(&mut self, span: Span) {
        let can_insert = self
            .prev_significant
            .as_ref()
            .is_some_and(TokenKind::can_end_statement);
        if !can_insert {
            return;
        }

        if self
            .peek_next_significant_kind()
            .is_some_and(|kind| kind.prevents_semicolon_before())
        {
            return;
        }

        self.push_token(TokenKind::Semicolon, "", span, true);
    }

    fn insert_semicolon_if_needed(&mut self) {
        let can_insert = self
            .prev_significant
            .as_ref()
            .is_some_and(TokenKind::can_end_statement);
        if can_insert {
            let span = self.current_span();
            self.push_token(TokenKind::Semicolon, "", span, true);
        }
    }

    fn lex_line_doc_comment(&mut self) {
        let start = self.mark();
        while !self.is_at_end() && !matches!(self.peek(), Some('\n' | '\r')) {
            self.bump();
        }
        let lexeme = self.slice_from(start.0).to_string();
        self.push_token(
            TokenKind::DocComment(lexeme.clone()),
            lexeme,
            self.span_from(start),
            false,
        );
    }

    fn lex_block_doc_comment(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        self.bump_n(3);
        while !self.is_at_end() && !self.starts_with("*/") {
            self.bump();
        }
        if self.is_at_end() {
            return Err(self.error_from(
                start,
                LexErrorCode::UnterminatedBlockComment,
                "unterminated doc block comment",
            ));
        }
        self.bump_n(2);
        let lexeme = self.slice_from(start.0).to_string();
        self.push_token(
            TokenKind::DocComment(lexeme.clone()),
            lexeme,
            self.span_from(start),
            false,
        );
        Ok(())
    }

    fn skip_line_comment(&mut self) {
        while !self.is_at_end() && !matches!(self.peek(), Some('\n' | '\r')) {
            self.bump();
        }
    }

    fn skip_block_comment(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        self.bump_n(2);
        while !self.is_at_end() && !self.starts_with("*/") {
            self.bump();
        }
        if self.is_at_end() {
            return Err(self.error_from(
                start,
                LexErrorCode::UnterminatedBlockComment,
                "unterminated block comment",
            ));
        }
        self.bump_n(2);
        Ok(())
    }

    fn lex_raw_string(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        self.bump_n(2);
        let content_start = self.pos;
        while !self.is_at_end() && self.peek() != Some('"') {
            self.bump();
        }
        if self.is_at_end() {
            return Err(self.error_from(
                start,
                LexErrorCode::UnterminatedRawString,
                "unterminated raw string literal",
            ));
        }
        let content = self.source[content_start..self.pos].to_string();
        self.bump();
        self.push_token(
            TokenKind::RawString(content),
            self.slice_from(start.0).to_string(),
            self.span_from(start),
            false,
        );
        Ok(())
    }

    fn lex_raw_multiline_string(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        self.bump_n(4);
        let content_start = self.pos;
        while !self.is_at_end() && !self.starts_with("\"\"\"") {
            self.bump();
        }
        if self.is_at_end() {
            return Err(self.error_from(
                start,
                LexErrorCode::UnterminatedRawString,
                "unterminated raw multiline string literal",
            ));
        }
        let content = self.source[content_start..self.pos].to_string();
        self.bump_n(3);
        self.push_token(
            TokenKind::RawString(content),
            self.slice_from(start.0).to_string(),
            self.span_from(start),
            false,
        );
        Ok(())
    }

    fn lex_string(&mut self, interpolation_mode: bool) -> Result<(), LexError> {
        let start = self.mark();
        self.bump();
        self.push_token(TokenKind::StringStart, "\"", self.span_from(start), false);
        self.lex_string_parts('"', interpolation_mode, LexErrorCode::UnterminatedString)?;
        let end = self.mark();
        self.bump();
        self.push_token(TokenKind::StringEnd, "\"", self.span_from(end), false);
        Ok(())
    }

    fn lex_multiline_string(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        self.bump_n(3);
        self.push_token(
            TokenKind::MultilineStringStart,
            "\"\"\"",
            self.span_from(start),
            false,
        );
        self.lex_multiline_string_parts()?;
        let end = self.mark();
        self.bump_n(3);
        self.push_token(
            TokenKind::MultilineStringEnd,
            "\"\"\"",
            self.span_from(end),
            false,
        );
        Ok(())
    }

    fn lex_string_parts(
        &mut self,
        terminator: char,
        interpolation_mode: bool,
        unterminated_code: LexErrorCode,
    ) -> Result<(), LexError> {
        let mut text_start = self.mark();
        while !self.is_at_end() && self.peek() != Some(terminator) {
            if !interpolation_mode && matches!(self.peek(), Some('\n' | '\r')) {
                return Err(self.error_from(
                    text_start,
                    unterminated_code,
                    "unterminated string literal",
                ));
            }
            if self.starts_with("${") {
                self.flush_text(text_start, self.pos);
                let start = self.mark();
                self.bump_n(2);
                self.push_token(TokenKind::InterpStart, "${", self.span_from(start), false);
                self.lex_interpolation()?;
                text_start = self.mark();
                continue;
            }
            if self.peek() == Some('\\') {
                self.flush_text(text_start, self.pos);
                self.lex_escape()?;
                text_start = self.mark();
                continue;
            }
            self.bump();
        }
        if self.is_at_end() {
            return Err(self.error_from(
                text_start,
                unterminated_code,
                "unterminated string literal",
            ));
        }
        self.flush_text(text_start, self.pos);
        Ok(())
    }

    fn lex_multiline_string_parts(&mut self) -> Result<(), LexError> {
        let mut text_start = self.mark();
        while !self.is_at_end() && !self.starts_with("\"\"\"") {
            if self.starts_with("${") {
                self.flush_text(text_start, self.pos);
                let start = self.mark();
                self.bump_n(2);
                self.push_token(TokenKind::InterpStart, "${", self.span_from(start), false);
                self.lex_interpolation()?;
                text_start = self.mark();
                continue;
            }
            if self.peek() == Some('\\') {
                self.flush_text(text_start, self.pos);
                self.lex_escape()?;
                text_start = self.mark();
                continue;
            }
            self.bump();
        }
        if self.is_at_end() {
            return Err(self.error_from(
                text_start,
                LexErrorCode::UnterminatedMultilineString,
                "unterminated multiline string literal",
            ));
        }
        self.flush_text(text_start, self.pos);
        Ok(())
    }

    fn lex_interpolation(&mut self) -> Result<(), LexError> {
        let mut brace_depth = 0usize;
        while !self.is_at_end() {
            if self.peek() == Some('}') {
                if brace_depth == 0 {
                    break;
                }
                brace_depth -= 1;
                self.lex_operator_or_punctuation()?;
                continue;
            }
            if self.consume_space_or_newline()? {
                continue;
            }
            if self.peek() == Some('"') {
                self.lex_string(true)?;
            } else if self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.lex_number()?;
            } else if self.peek().is_some_and(is_ident_start) {
                self.lex_ident_or_keyword();
            } else if self.starts_with("//") {
                self.skip_line_comment();
            } else if self.starts_with("/*") {
                self.skip_block_comment()?;
            } else {
                if self.peek() == Some('{') {
                    brace_depth += 1;
                }
                self.lex_operator_or_punctuation()?;
            }
        }
        if self.is_at_end() {
            return Err(LexError::new(
                LexErrorCode::UnclosedInterpolation,
                "expected `}` to close string interpolation",
                self.current_span(),
            ));
        }
        let end = self.mark();
        self.bump();
        self.push_token(TokenKind::InterpEnd, "}", self.span_from(end), false);
        Ok(())
    }

    fn lex_escape(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        self.bump();
        match self.peek() {
            Some('n' | 't' | 'r' | '0' | '\\' | '"' | '\'' | '$') => {
                self.bump();
            }
            Some('u') => {
                self.bump();
                if self.peek() != Some('{') {
                    return Err(self.error_from(
                        start,
                        LexErrorCode::InvalidUnicodeEscape,
                        "invalid unicode escape",
                    ));
                }
                self.bump();
                let mut digits = 0;
                while self.peek().is_some_and(|ch| ch.is_ascii_hexdigit()) {
                    digits += 1;
                    self.bump();
                }
                if digits == 0 || self.peek() != Some('}') {
                    return Err(self.error_from(
                        start,
                        LexErrorCode::InvalidUnicodeEscape,
                        "invalid unicode escape",
                    ));
                }
                self.bump();
            }
            _ => {
                return Err(self.error_from(
                    start,
                    LexErrorCode::InvalidEscape,
                    "invalid escape sequence",
                ));
            }
        }
        let lexeme = self.slice_from(start.0).to_string();
        self.push_token(
            TokenKind::StringEscape(lexeme.clone()),
            lexeme,
            self.span_from(start),
            false,
        );
        Ok(())
    }

    fn lex_char(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        self.bump();
        if self.peek() == Some('\'') {
            return Err(self.error_from(start, LexErrorCode::EmptyChar, "empty char literal"));
        }
        let content_start = self.pos;
        let mut count = 0;
        if self.peek() == Some('\\') {
            self.bump();
            match self.peek() {
                Some('n' | 't' | 'r' | '0' | '\\' | '"' | '\'' | '$') => {
                    self.bump();
                }
                Some('u') => {
                    self.bump();
                    if self.peek() != Some('{') {
                        return Err(self.error_from(
                            start,
                            LexErrorCode::InvalidUnicodeEscape,
                            "invalid unicode escape",
                        ));
                    }
                    self.bump();
                    let mut digits = 0;
                    while self.peek().is_some_and(|ch| ch.is_ascii_hexdigit()) {
                        digits += 1;
                        self.bump();
                    }
                    if digits == 0 || self.peek() != Some('}') {
                        return Err(self.error_from(
                            start,
                            LexErrorCode::InvalidUnicodeEscape,
                            "invalid unicode escape",
                        ));
                    }
                    self.bump();
                }
                _ => {
                    return Err(self.error_from(
                        start,
                        LexErrorCode::InvalidEscape,
                        "invalid escape sequence",
                    ));
                }
            }
            count = 1;
        } else {
            while !self.is_at_end() && self.peek() != Some('\'') {
                if matches!(self.peek(), Some('\n' | '\r')) {
                    return Err(self.error_from(
                        start,
                        LexErrorCode::UnterminatedChar,
                        "unterminated char literal",
                    ));
                }
                count += 1;
                self.bump();
            }
        }
        if self.is_at_end() || self.peek() != Some('\'') {
            return Err(self.error_from(
                start,
                LexErrorCode::UnterminatedChar,
                "unterminated char literal",
            ));
        }
        if count > 1 {
            return Err(self.error_from(
                start,
                LexErrorCode::CharTooLong,
                "char literal contains more than one codepoint",
            ));
        }
        let content = self.source[content_start..self.pos].to_string();
        self.bump();
        self.push_token(
            TokenKind::Char(content),
            self.slice_from(start.0).to_string(),
            self.span_from(start),
            false,
        );
        Ok(())
    }

    fn lex_number(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        if self.starts_with("0x") {
            return self.lex_radix_number(start, 16);
        }
        if self.starts_with("0b") {
            return self.lex_radix_number(start, 2);
        }
        if self.starts_with("0o") {
            return self.lex_radix_number(start, 8);
        }

        self.bump_digits_or_underscores();
        let mut is_float = false;
        if self.peek() == Some('.') && self.peek_next() != Some('.') {
            is_float = true;
            self.bump();
            if !self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                return Err(self.error_from(
                    start,
                    LexErrorCode::InvalidNumericLiteral,
                    "expected digit after decimal point",
                ));
            }
            self.bump_digits_or_underscores();
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            is_float = true;
            self.bump();
            if matches!(self.peek(), Some('+' | '-')) {
                self.bump();
            }
            if !self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                return Err(self.error_from(
                    start,
                    LexErrorCode::InvalidNumericLiteral,
                    "expected digit in float exponent",
                ));
            }
            self.bump_digits_or_underscores();
        }

        let lexeme = self.slice_from(start.0).to_string();
        if lexeme.ends_with('_') {
            return Err(self.error_from(
                start,
                LexErrorCode::InvalidNumericLiteral,
                "numeric literal cannot end with `_`",
            ));
        }
        if !is_float && lexeme.len() > 1 && lexeme.starts_with('0') {
            return Err(self.error_from(
                start,
                LexErrorCode::LeadingZero,
                "decimal literals cannot have leading zeroes",
            ));
        }

        let kind = if is_float {
            TokenKind::Float(lexeme.clone())
        } else {
            TokenKind::IntDec(lexeme.clone())
        };
        self.push_token(kind, lexeme, self.span_from(start), false);
        Ok(())
    }

    fn lex_radix_number(&mut self, start: Mark, radix: u8) -> Result<(), LexError> {
        self.bump_n(2);
        let digit_start = self.pos;
        while self
            .peek()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            let Some(ch) = self.peek() else {
                break;
            };
            let valid = match radix {
                2 => matches!(ch, '0' | '1' | '_'),
                8 => matches!(ch, '0'..='7' | '_'),
                16 => ch.is_ascii_hexdigit() || ch == '_',
                _ => {
                    return Err(self.error_from(
                        start,
                        LexErrorCode::InvalidNumericLiteral,
                        "unsupported numeric radix",
                    ));
                }
            };
            if !valid {
                let code = match radix {
                    2 => LexErrorCode::InvalidBinaryDigit,
                    8 => LexErrorCode::InvalidOctalDigit,
                    16 => LexErrorCode::InvalidHexDigit,
                    _ => LexErrorCode::InvalidNumericLiteral,
                };
                return Err(self.error_from(start, code, "invalid digit for numeric literal"));
            }
            self.bump();
        }
        if self.pos == digit_start {
            return Err(self.error_from(
                start,
                LexErrorCode::InvalidNumericLiteral,
                "expected digit after numeric prefix",
            ));
        }
        let lexeme = self.slice_from(start.0).to_string();
        if lexeme.ends_with('_') {
            return Err(self.error_from(
                start,
                LexErrorCode::InvalidNumericLiteral,
                "numeric literal cannot end with `_`",
            ));
        }
        let kind = match radix {
            2 => TokenKind::IntBin(lexeme.clone()),
            8 => TokenKind::IntOct(lexeme.clone()),
            16 => TokenKind::IntHex(lexeme.clone()),
            _ => {
                return Err(self.error_from(
                    start,
                    LexErrorCode::InvalidNumericLiteral,
                    "unsupported numeric radix",
                ));
            }
        };
        self.push_token(kind, lexeme, self.span_from(start), false);
        Ok(())
    }

    fn lex_ident_or_keyword(&mut self) {
        let start = self.mark();
        self.bump();
        while self.peek().is_some_and(is_ident_continue) {
            self.bump();
        }
        let lexeme = self.slice_from(start.0).to_string();
        let kind = keyword_kind(&lexeme).unwrap_or_else(|| {
            if lexeme.starts_with(|ch: char| ch.is_ascii_uppercase()) {
                TokenKind::IdentType(lexeme.clone())
            } else {
                TokenKind::IdentValue(lexeme.clone())
            }
        });
        self.push_token(kind, lexeme, self.span_from(start), false);
    }

    fn lex_operator_or_punctuation(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        let Some((kind, len)) = token_kind_from_prefix(&self.source[self.pos..]) else {
            if self.is_at_end() {
                return Ok(());
            }
            self.bump();
            return Err(self.error_from(start, LexErrorCode::InvalidChar, "invalid character"));
        };
        self.bump_n(len);
        self.push_token(
            kind,
            self.slice_from(start.0).to_string(),
            self.span_from(start),
            false,
        );
        Ok(())
    }

    fn flush_text(&mut self, start: Mark, end_pos: usize) {
        if end_pos <= start.0 {
            return;
        }
        let text = self.source[start.0..end_pos].to_string();
        let span = Span::new(start.0, end_pos, start.1, start.2, self.line, self.col);
        self.push_token(TokenKind::StringText(text.clone()), text, span, false);
    }

    fn peek_next_significant_kind(&self) -> Option<TokenKind> {
        let mut i = self.pos;
        loop {
            let rest = &self.source[i..];
            if rest.is_empty() {
                return Some(TokenKind::Eof);
            }
            if rest.starts_with(' ')
                || rest.starts_with('\t')
                || rest.starts_with('\n')
                || rest.starts_with("\r\n")
            {
                let Some(ch) = rest.chars().next() else {
                    return Some(TokenKind::Eof);
                };
                i += ch.len_utf8();
                continue;
            }
            if rest.starts_with("//") {
                if let Some(offset) = rest.find(['\n', '\r']) {
                    i += offset;
                    continue;
                }
                return Some(TokenKind::Eof);
            }
            if rest.starts_with("/*") {
                if let Some(offset) = rest.find("*/") {
                    i += offset + 2;
                    continue;
                }
                return Some(TokenKind::Eof);
            }
            return Some(peek_kind_from(rest));
        }
    }

    fn push_token(
        &mut self,
        kind: TokenKind,
        lexeme: impl Into<String>,
        span: Span,
        inserted: bool,
    ) {
        if !matches!(kind, TokenKind::DocComment(_)) {
            self.prev_significant = Some(kind.clone());
        }
        self.tokens.push(Token {
            kind,
            lexeme: lexeme.into(),
            span,
            inserted,
        });
    }

    fn current_span(&self) -> Span {
        Span::new(self.pos, self.pos, self.line, self.col, self.line, self.col)
    }

    fn span_from(&self, start: Mark) -> Span {
        Span::new(start.0, self.pos, start.1, start.2, self.line, self.col)
    }

    fn error_from(&self, start: Mark, code: LexErrorCode, message: impl Into<String>) -> LexError {
        LexError::new(code, message, self.span_from(start))
    }

    fn mark(&self) -> Mark {
        (self.pos, self.line, self.col)
    }

    fn slice_from(&self, start: usize) -> &str {
        &self.source[start..self.pos]
    }

    fn bump_digits_or_underscores(&mut self) {
        while self
            .peek()
            .is_some_and(|ch| ch.is_ascii_digit() || ch == '_')
        {
            self.bump();
        }
    }

    fn bump_n(&mut self, bytes: usize) {
        let target = self.pos + bytes;
        while self.pos < target {
            self.bump();
        }
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn peek(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }

    fn peek_next(&self) -> Option<char> {
        let mut chars = self.source[self.pos..].chars();
        chars.next()?;
        chars.next()
    }

    fn starts_with(&self, prefix: &str) -> bool {
        self.source[self.pos..].starts_with(prefix)
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.source.len()
    }
}

type Mark = (usize, usize, usize);
