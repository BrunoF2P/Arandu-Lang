use crate::ident::{is_ident_continue, is_ident_start, is_digit, keyword_kind};
use crate::punctuation::{peek_kind_from, token_kind_from_prefix};
use crate::simd::SimdBackendKind;

use crate::{LexError, LexErrorCode, Span, Token, TokenKind};

pub struct Lexer<'a> {
    source: &'a str,
    pos: usize,
    line: usize,
    col: usize,
    tokens: Vec<Token>,
    prev_significant: Option<TokenKind>,
    diagnostics: Vec<LexError>,
    backend: SimdBackendKind,
}

#[derive(Clone, Copy)]
struct Mark {
    pos: usize,
    line: usize,
    col: usize,
}

#[derive(Clone, Copy)]
enum Radix {
    Bin,
    Oct,
    Hex,
}

impl Radix {
    fn is_valid_digit(self, ch: char) -> bool {
        match self {
            Radix::Bin => matches!(ch, '0' | '1' | '_'),
            Radix::Oct => matches!(ch, '0'..='7' | '_'),
            Radix::Hex => ch.is_ascii_hexdigit() || ch == '_',
        }
    }

    fn token_kind(self) -> TokenKind {
        match self {
            Radix::Bin => TokenKind::IntBin,
            Radix::Oct => TokenKind::IntOct,
            Radix::Hex => TokenKind::IntHex,
        }
    }

    fn invalid_digit_code(self) -> LexErrorCode {
        match self {
            Radix::Bin => LexErrorCode::InvalidBinaryDigit,
            Radix::Oct => LexErrorCode::InvalidOctalDigit,
            Radix::Hex => LexErrorCode::InvalidHexDigit,
        }
    }
}

impl<'a> Lexer<'a> {
    #[must_use]
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            pos: 0,
            line: 1,
            col: 1,
            tokens: Vec::with_capacity(source.len() / 4),
            prev_significant: None,
            diagnostics: Vec::new(),
            backend: SimdBackendKind::detect(),
        }
    }

    /// Lexes the full source, returning tokens or the first lexical error.
    ///
    /// # Errors
    ///
    /// Returns the first [`LexError`] if the source contains invalid tokens.
    pub fn lex(self) -> Result<crate::Lexed<'a>, LexError> {
        let lexed = self.lex_recovering();
        if let Some(err) = lexed.diagnostics.first().copied() {
            Err(err)
        } else {
            Ok(lexed)
        }
    }

    #[must_use]
    pub fn lex_recovering(mut self) -> crate::Lexed<'a> {
        while !self.is_at_end() {
            let start_pos = self.pos;
            let result = self.lex_next_token();

            if let Err(err) = result {
                let code = err.code;
                let span = err.span;
                self.diagnostics.push(err);
                self.push_token(TokenKind::Error(code), span, false);
                if self.pos == start_pos {
                    self.bump();
                }
            }
        }

        self.insert_semicolon_if_needed();
        let span = self.current_span();
        self.push_token(TokenKind::Eof, span, false);

        crate::Lexed {
            source: self.source,
            tokens: self.tokens,
            diagnostics: self.diagnostics,
        }
    }

    fn lex_next_token(&mut self) -> Result<(), LexError> {
        let prev_pos = self.pos;
        self.skip_whitespace();
        if self.pos > prev_pos {
            return Ok(());
        }

        let bytes = self.source.as_bytes();
        let remaining = bytes.len() - self.pos;
        if remaining == 0 {
            return Ok(());
        }

        let first = bytes[self.pos];

        match first {
            b'/' => {
                if remaining >= 3 && bytes[self.pos + 1] == b'/' && bytes[self.pos + 2] == b'/' {
                    self.lex_line_doc_comment();
                    return Ok(());
                } else if remaining >= 3
                    && bytes[self.pos + 1] == b'*'
                    && bytes[self.pos + 2] == b'*'
                {
                    return self.lex_block_doc_comment();
                } else if remaining >= 2 && bytes[self.pos + 1] == b'/' {
                    self.skip_line_comment();
                    return Ok(());
                } else if remaining >= 2 && bytes[self.pos + 1] == b'*' {
                    return self.skip_block_comment();
                }
            }
            b'r' => {
                if remaining >= 4 && &bytes[self.pos..self.pos + 4] == b"r\"\"\"" {
                    return self.lex_raw_multiline_string();
                } else if remaining >= 2 && bytes[self.pos + 1] == b'"' {
                    return self.lex_raw_string();
                }
            }
            b'"' => {
                if remaining >= 3 && bytes[self.pos + 1] == b'"' && bytes[self.pos + 2] == b'"' {
                    return self.lex_multiline_string();
                }
                return self.lex_string(false);
            }
            b'\'' => {
                return self.lex_char();
            }
            b'0'..=b'9' => {
                return self.lex_number();
            }
            _ => {}
        }

        if first < 128 && is_ident_start(first as char)
            || first >= 128 && self.peek().is_some_and(is_ident_start)
        {
            self.lex_ident_or_keyword();
            return Ok(());
        }

        self.lex_operator_or_punctuation()
    }

    fn skip_whitespace(&mut self) {
        let (newlines, skipped, last_nl) = match self.backend {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            SimdBackendKind::Avx2 => unsafe { crate::simd::avx2::skip_whitespace(&self.source.as_bytes()[self.pos..]) },
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            SimdBackendKind::Sse2 => unsafe { crate::simd::sse2::skip_whitespace(&self.source.as_bytes()[self.pos..]) },
            #[cfg(target_arch = "aarch64")]
            SimdBackendKind::Neon => unsafe { crate::simd::neon::skip_whitespace(&self.source.as_bytes()[self.pos..]) },
            _ => crate::simd::scalar::skip_whitespace(&self.source.as_bytes()[self.pos..]),
        };

        if skipped == 0 {
            return;
        }

        if newlines > 0 {
            let mut first_nl_offset = self.source.as_bytes()[self.pos..self.pos + skipped]
                .iter()
                .position(|&b| b == b'\n')
                .unwrap_or(0);

            if first_nl_offset > 0 && self.source.as_bytes()[self.pos + first_nl_offset - 1] == b'\r' {
                first_nl_offset -= 1;
            }

            let first_nl_span = Span::new(
                self.pos + first_nl_offset,
                self.pos + first_nl_offset,
                self.line,
                self.col + first_nl_offset,
                self.line,
                self.col + first_nl_offset,
            );

            self.pos += skipped;
            self.line += newlines;
            if let Some(last_nl_idx) = last_nl {
                self.col = skipped - last_nl_idx;
            } else {
                self.col += skipped;
            }

            self.maybe_insert_semicolon_at(first_nl_span);
        } else {
            self.pos += skipped;
            self.col += skipped;
        }
    }

    fn maybe_insert_semicolon_at(&mut self, span: Span) {
        let can_insert = self
            .prev_significant
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

        self.push_token(TokenKind::Semicolon, span, true);
    }

    fn insert_semicolon_if_needed(&mut self) {
        let can_insert = self
            .prev_significant
            .is_some_and(TokenKind::can_end_statement);
        if can_insert {
            let span = self.current_span();
            self.push_token(TokenKind::Semicolon, span, true);
        }
    }

    fn lex_line_doc_comment(&mut self) {
        let start = self.mark();
        while !self.is_at_end() && !matches!(self.peek(), Some('\n' | '\r')) {
            self.bump();
        }
        self.push_token(TokenKind::DocComment, self.span_from(start), false);
    }

    fn lex_block_doc_comment(&mut self) -> Result<(), LexError> {
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

    fn skip_line_comment(&mut self) {
        while !self.is_at_end() && !matches!(self.peek(), Some('\n' | '\r')) {
            self.bump();
        }
    }

    fn skip_block_comment(&mut self) -> Result<(), LexError> {
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

    fn lex_raw_string(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        self.bump_ascii(2);
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
        self.bump();
        self.push_token(TokenKind::RawString, self.span_from(start), false);
        Ok(())
    }

    fn lex_raw_multiline_string(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        self.bump_ascii(4);
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
        self.bump_ascii(3);
        self.push_token(TokenKind::RawString, self.span_from(start), false);
        Ok(())
    }

    fn lex_string(&mut self, interpolation_mode: bool) -> Result<(), LexError> {
        let start = self.mark();
        self.bump();
        self.push_token(TokenKind::StringStart, self.span_from(start), false);
        self.lex_string_body(
            StringTerminator::Quote,
            interpolation_mode,
            LexErrorCode::UnterminatedString,
        )?;
        let end = self.mark();
        self.bump();
        self.push_token(TokenKind::StringEnd, self.span_from(end), false);
        Ok(())
    }

    fn lex_multiline_string(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        self.bump_ascii(3);
        self.push_token(
            TokenKind::MultilineStringStart,
            self.span_from(start),
            false,
        );
        self.lex_string_body(
            StringTerminator::TripleQuote,
            true,
            LexErrorCode::UnterminatedMultilineString,
        )?;
        let end = self.mark();
        self.bump_ascii(3);
        self.push_token(TokenKind::MultilineStringEnd, self.span_from(end), false);
        Ok(())
    }

    fn lex_string_body(
        &mut self,
        terminator: StringTerminator,
        allow_newline: bool,
        unterminated_code: LexErrorCode,
    ) -> Result<(), LexError> {
        let mut text_start = self.mark();
        while !self.is_at_end() && !self.at_string_terminator(terminator) {
            if !allow_newline && matches!(self.peek(), Some('\n' | '\r')) {
                return Err(self.error_from(
                    text_start,
                    unterminated_code,
                    "unterminated string literal",
                ));
            }
            if self.starts_with("${") {
                self.flush_text(text_start, self.pos);
                let start = self.mark();
                self.bump_ascii(2);
                self.push_token(TokenKind::InterpStart, self.span_from(start), false);
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

    fn at_string_terminator(&self, terminator: StringTerminator) -> bool {
        match terminator {
            StringTerminator::Quote => self.peek() == Some('"'),
            StringTerminator::TripleQuote => self.starts_with("\"\"\""),
        }
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
            let prev_pos = self.pos;
            self.skip_whitespace();
            if self.pos > prev_pos {
                continue;
            }
            if self.peek() == Some('"') {
                self.lex_string(true)?;
            } else if self.peek().is_some_and(is_digit) {
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
        self.push_token(TokenKind::InterpEnd, self.span_from(end), false);
        Ok(())
    }

    fn lex_escape(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        self.consume_escape_sequence(start)?;
        self.push_token(TokenKind::StringEscape, self.span_from(start), false);
        Ok(())
    }

    fn consume_escape_sequence(&mut self, start: Mark) -> Result<(), LexError> {
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
                self.bump(); // }
                let code_str = self.slice_from(start.pos);
                let hex = &code_str[3..code_str.len() - 1];
                let value = u32::from_str_radix(hex, 16).unwrap_or(u32::MAX);
                if value > 0x10FFFF {
                    return Err(self.error_from(
                        start,
                        LexErrorCode::InvalidUnicodeEscape,
                        "unicode escape must be a valid scalar value (<= 0x10FFFF)",
                    ));
                }
            }
            _ => {
                return Err(self.error_from(
                    start,
                    LexErrorCode::InvalidEscape,
                    "invalid escape sequence",
                ));
            }
        }
        Ok(())
    }

    fn lex_char(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        self.bump();
        if self.peek() == Some('\'') {
            return Err(self.error_from(start, LexErrorCode::EmptyChar, "empty char literal"));
        }
        let mut count = 0;
        if self.peek() == Some('\\') {
            self.consume_escape_sequence(start)?;
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
        self.bump();
        self.push_token(TokenKind::Char, self.span_from(start), false);
        Ok(())
    }

    fn lex_number(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        if self.starts_with("0x") {
            return self.lex_radix_number(start, Radix::Hex);
        }
        if self.starts_with("0b") {
            return self.lex_radix_number(start, Radix::Bin);
        }
        if self.starts_with("0o") {
            return self.lex_radix_number(start, Radix::Oct);
        }

        self.bump_digits_or_underscores();
        let mut is_float = false;
        if self.peek() == Some('.') && self.peek_next() != Some('.') {
            is_float = true;
            self.bump();
            if !self.peek().is_some_and(is_digit) {
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
            if !self.peek().is_some_and(is_digit) {
                return Err(self.error_from(
                    start,
                    LexErrorCode::InvalidNumericLiteral,
                    "expected digit in float exponent",
                ));
            }
            self.bump_digits_or_underscores();
        }

        let lexeme = self.slice_from(start.pos);
        if lexeme.ends_with('_') {
            return Err(self.error_from(
                start,
                LexErrorCode::InvalidNumericLiteral,
                "numeric literal cannot end with `_`",
            ));
        }
        if lexeme.contains("__") {
            return Err(self.error_from(
                start,
                LexErrorCode::InvalidNumericLiteral,
                "numeric literal cannot contain multiple consecutive underscores",
            ));
        }
        if !is_float && lexeme.len() > 1 && lexeme.starts_with('0') {
            return Err(self.error_from(
                start,
                LexErrorCode::LeadingZero,
                "decimal literals cannot have leading zeroes",
            ));
        }
        if self.peek().is_some_and(is_ident_start) {
            return Err(self.error_from(
                start,
                LexErrorCode::InvalidNumericLiteral,
                "numeric literal cannot be directly followed by an identifier",
            ));
        }

        let kind = if is_float {
            TokenKind::Float
        } else {
            TokenKind::IntDec
        };
        self.push_token(kind, self.span_from(start), false);
        Ok(())
    }

    fn lex_radix_number(&mut self, start: Mark, radix: Radix) -> Result<(), LexError> {
        self.bump_ascii(2);
        let digit_start = self.pos;
        while self
            .peek()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            let Some(ch) = self.peek() else {
                break;
            };
            if !radix.is_valid_digit(ch) {
                return Err(self.error_from(
                    start,
                    radix.invalid_digit_code(),
                    "invalid digit for numeric literal",
                ));
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
        let lexeme = self.slice_from(start.pos);
        if lexeme.ends_with('_') {
            return Err(self.error_from(
                start,
                LexErrorCode::InvalidNumericLiteral,
                "numeric literal cannot end with `_`",
            ));
        }
        if lexeme.contains("__") {
            return Err(self.error_from(
                start,
                LexErrorCode::InvalidNumericLiteral,
                "numeric literal cannot contain multiple consecutive underscores",
            ));
        }
        if self.peek().is_some_and(is_ident_start) {
            return Err(self.error_from(
                start,
                LexErrorCode::InvalidNumericLiteral,
                "numeric literal cannot be directly followed by an identifier",
            ));
        }
        self.push_token(radix.token_kind(), self.span_from(start), false);
        Ok(())
    }

    fn lex_ident_or_keyword(&mut self) {
        let start = self.mark();
        self.bump();
        let len = match self.backend {
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            SimdBackendKind::Avx2 => unsafe { crate::simd::avx2::scan_identifier(self.source[self.pos..].as_bytes()) },
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            SimdBackendKind::Sse2 => unsafe { crate::simd::sse2::scan_identifier(self.source[self.pos..].as_bytes()) },
            #[cfg(target_arch = "aarch64")]
            SimdBackendKind::Neon => unsafe { crate::simd::neon::scan_identifier(self.source[self.pos..].as_bytes()) },
            _ => crate::simd::scalar::scan_identifier(self.source[self.pos..].as_bytes()),
        };
        self.pos += len;
        self.col += len;

        while let Some(ch) = self.peek() {
            if is_ident_continue(ch) {
                self.bump();
            } else {
                break;
            }
        }
        let lexeme = self.slice_from(start.pos);
        let kind = keyword_kind(lexeme).unwrap_or_else(|| {
            let is_type = lexeme
                .as_bytes()
                .first()
                .is_some_and(|byte| byte.is_ascii_uppercase());
            if is_type {
                TokenKind::IdentType
            } else {
                TokenKind::IdentValue
            }
        });
        self.push_token(kind, self.span_from(start), false);
    }

    fn lex_operator_or_punctuation(&mut self) -> Result<(), LexError> {
        let start = self.mark();
        let Some((kind, len)) = token_kind_from_prefix(&self.source.as_bytes()[self.pos..]) else {
            if self.is_at_end() {
                return Ok(());
            }
            self.bump();
            return Err(self.error_from(start, LexErrorCode::InvalidChar, "invalid character"));
        };
        self.bump_ascii(len);
        self.push_token(kind, self.span_from(start), false);
        Ok(())
    }

    fn flush_text(&mut self, start: Mark, end_pos: usize) {
        if end_pos <= start.pos {
            return;
        }
        let span = Span::new(
            start.pos, end_pos, start.line, start.col, self.line, self.col,
        );
        self.push_token(TokenKind::StringText, span, false);
    }

    fn peek_next_significant_kind(&self) -> Option<TokenKind> {
        let bytes = self.source.as_bytes();
        let mut i = self.pos;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                i += 1;
                continue;
            }
            let remaining = bytes.len() - i;
            if remaining >= 2 && bytes[i] == b'/' {
                if bytes[i + 1] == b'/' {
                    while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
                        i += 1;
                    }
                    continue;
                } else if bytes[i + 1] == b'*' {
                    i += 2;
                    while i + 1 < bytes.len() {
                        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                    continue;
                }
            }
            return Some(peek_kind_from(&self.source[i..]));
        }
        Some(TokenKind::Eof)
    }

    fn push_token(&mut self, kind: TokenKind, span: Span, inserted: bool) {
        if !matches!(kind, TokenKind::DocComment) {
            self.prev_significant = Some(kind);
        }
        self.tokens.push(Token {
            kind,
            span,
            inserted,
        });
    }

    fn current_span(&self) -> Span {
        Span::new(self.pos, self.pos, self.line, self.col, self.line, self.col)
    }

    fn span_from(&self, start: Mark) -> Span {
        Span::new(
            start.pos, self.pos, start.line, start.col, self.line, self.col,
        )
    }

    #[cold]
    #[inline(never)]
    fn error_from(&self, start: Mark, code: LexErrorCode, message: &'static str) -> LexError {
        LexError::new(code, message, self.span_from(start))
    }

    fn mark(&self) -> Mark {
        Mark {
            pos: self.pos,
            line: self.line,
            col: self.col,
        }
    }

    fn slice_from(&self, start: usize) -> &str {
        &self.source[start..self.pos]
    }

    fn bump_digits_or_underscores(&mut self) {
        while self
            .peek()
            .is_some_and(|ch| is_digit(ch) || ch == '_')
        {
            self.bump();
        }
    }

    /// Advance past `n` bytes of known ASCII content that contains no newlines.
    /// All callers use this for fixed delimiters like `//`, `/*`, `*/`, `${`, `"""`, etc.
    fn bump_ascii(&mut self, n: usize) {
        debug_assert!(
            self.source.as_bytes()[self.pos..self.pos + n]
                .iter()
                .all(|&b| b.is_ascii() && b != b'\n'),
            "bump_ascii called with non-ASCII or newline content"
        );
        self.pos += n;
        self.col += n;
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
        let bytes = self.source.as_bytes();
        if self.pos < bytes.len() {
            let b = bytes[self.pos];
            if b < 128 {
                Some(b as char)
            } else {
                self.source[self.pos..].chars().next()
            }
        } else {
            None
        }
    }

    fn peek_next(&self) -> Option<char> {
        let bytes = self.source.as_bytes();
        if self.pos < bytes.len() {
            let b = bytes[self.pos];
            if b < 128 {
                let next_pos = self.pos + 1;
                if next_pos < bytes.len() {
                    let b2 = bytes[next_pos];
                    if b2 < 128 {
                        return Some(b2 as char);
                    }
                }
            }
        }
        let mut chars = self.source[self.pos..].chars();
        chars.next()?;
        chars.next()
    }

    fn starts_with(&self, prefix: &str) -> bool {
        self.source.as_bytes()[self.pos..].starts_with(prefix.as_bytes())
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.source.len()
    }
}

#[derive(Clone, Copy)]
enum StringTerminator {
    Quote,
    TripleQuote,
}
