mod decl;
mod error;
mod expr;
mod pattern;
mod stmt;
mod types;

pub use error::{ParseError, ParseErrorCode};

use arandu_lexer::{Span, Token, TokenKind, lex};

use crate::*;

pub fn parse(source: &str) -> Result<Program, ParseError> {
    let tokens = lex(source).map_err(ParseError::from_lex)?;
    Parser::new(tokens).parse_program()
}

pub fn parse_to_string(source: &str) -> Result<String, ParseError> {
    Ok(parse(source)?.dump())
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    allow_block_calls: bool,
    docs: Vec<DocCommentAttachment>,
    pending_docs: Vec<PendingDoc>,
}

#[derive(Debug, Clone)]
pub(super) struct PendingDoc {
    span: Span,
    text: String,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            allow_block_calls: true,
            docs: Vec::new(),
            pending_docs: Vec::new(),
        }
    }

    pub fn parse_program(mut self) -> Result<Program, ParseError> {
        let start = self.mark();
        self.skip_semicolons();
        self.collect_doc_comments();
        let module = if self.at_kind_name("KW_MODULE") {
            Some(self.parse_module()?)
        } else {
            None
        };

        let mut imports = Vec::new();
        let mut decls = Vec::new();
        while !self.at_kind_name("EOF") {
            self.skip_semicolons();
            self.collect_doc_comments();
            if self.at_kind_name("EOF") {
                break;
            }
            if self.at_kind_name("KW_IMPORT") {
                imports.push(self.parse_import()?);
                continue;
            }
            decls.push(self.parse_top_level_decl()?);
        }

        Ok(Program {
            span: self.span_from_mark(start),
            module,
            imports,
            decls,
            docs: self.docs,
        })
    }
    pub(super) fn mark(&self) -> usize {
        self.pos
    }

    pub(super) fn span_from_mark(&self, start: usize) -> Span {
        let start_span = self
            .tokens
            .get(start)
            .map(|token| token.span)
            .unwrap_or_else(|| self.current().span);
        let end_span = if self.pos == start {
            start_span
        } else {
            self.tokens
                .get(self.pos.saturating_sub(1))
                .map(|token| token.span)
                .unwrap_or(start_span)
        };
        span_between(start_span, end_span)
    }

    pub(super) fn skip_semicolons(&mut self) {
        while self.at_kind_name("SEMICOLON") {
            self.consume();
        }
    }

    pub(super) fn collect_doc_comments(&mut self) {
        while let TokenKind::DocComment(text) = &self.current().kind {
            self.pending_docs.push(PendingDoc {
                span: self.current().span,
                text: text.clone(),
            });
            self.consume();
            self.skip_semicolons();
        }
    }

    pub(super) fn discard_doc_comments(&mut self) {
        self.collect_doc_comments();
        self.pending_docs.clear();
    }

    pub(super) fn take_pending_docs(&mut self) -> Vec<PendingDoc> {
        std::mem::take(&mut self.pending_docs)
    }

    pub(super) fn attach_docs(&mut self, docs: Vec<PendingDoc>, target_span: Span) {
        self.docs
            .extend(docs.into_iter().map(|doc| DocCommentAttachment {
                span: doc.span,
                text: doc.text,
                target_span,
            }));
    }

    pub(super) fn expect_semicolon(&mut self) -> Result<(), ParseError> {
        self.expect_name("SEMICOLON").map(|_| ())
    }

    pub(super) fn expect_ident_value(&mut self) -> Result<String, ParseError> {
        match &self.current().kind {
            TokenKind::IdentValue(name) => {
                let name = name.clone();
                self.consume();
                Ok(name)
            }
            _ => Err(ParseError::expected(
                ParseErrorCode::ExpectedToken,
                "expected value identifier",
                self.current(),
                &["value identifier"],
            )),
        }
    }

    pub(super) fn expect_ident_type(&mut self) -> Result<String, ParseError> {
        match &self.current().kind {
            TokenKind::IdentType(name) => {
                let name = name.clone();
                self.consume();
                Ok(name)
            }
            _ => Err(ParseError::expected(
                ParseErrorCode::ExpectedToken,
                "expected type identifier",
                self.current(),
                &["type identifier"],
            )),
        }
    }

    pub(super) fn expect_name_like(&mut self) -> Result<String, ParseError> {
        match &self.current().kind {
            TokenKind::IdentValue(name) | TokenKind::IdentType(name) => {
                let name = name.clone();
                self.consume();
                Ok(name)
            }
            _ => Err(ParseError::expected(
                ParseErrorCode::ExpectedToken,
                "expected identifier",
                self.current(),
                &["identifier"],
            )),
        }
    }

    pub(super) fn expect_module_segment(&mut self) -> Result<String, ParseError> {
        match &self.current().kind {
            TokenKind::IdentValue(name) => {
                let name = name.clone();
                self.consume();
                Ok(name)
            }
            kind if is_contextual_module_segment(kind) => {
                let text = self.current().lexeme.clone();
                self.consume();
                Ok(text)
            }
            _ => Err(ParseError::expected(
                ParseErrorCode::ExpectedToken,
                "expected module path segment",
                self.current(),
                &["module path segment"],
            )),
        }
    }

    pub(super) fn expect_name(&mut self, name: &str) -> Result<Token, ParseError> {
        if self.at_kind_name(name) {
            Ok(self.advance())
        } else {
            Err(ParseError::expected(
                ParseErrorCode::ExpectedToken,
                format!("expected {name}"),
                self.current(),
                token_expectation_names(name),
            ))
        }
    }

    pub(super) fn eat_name(&mut self, name: &str) -> bool {
        if self.at_kind_name(name) {
            self.consume();
            true
        } else {
            false
        }
    }

    pub(super) fn at_kind_name(&self, name: &str) -> bool {
        self.current().kind.name() == name
    }

    pub(super) fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    pub(super) fn previous(&self) -> &Token {
        &self.tokens[self.pos - 1]
    }

    pub(super) fn advance(&mut self) -> Token {
        let token = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        token
    }

    pub(super) fn consume(&mut self) {
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
    }

    pub(super) fn parse_expr_without_block_calls(
        &mut self,
        min_bp: u8,
    ) -> Result<Expr, ParseError> {
        let previous = self.allow_block_calls;
        self.allow_block_calls = false;
        let result = self.parse_expr(min_bp);
        self.allow_block_calls = previous;
        result
    }
}

pub(super) fn is_type_token(kind: &TokenKind) -> bool {
    primitive_type_name(kind).is_some()
        || matches!(kind, TokenKind::IdentType(_) | TokenKind::IdentValue(_))
}

pub(super) fn primitive_type_name(kind: &TokenKind) -> Option<&'static str> {
    Some(match kind {
        TokenKind::TypeInt => "int",
        TokenKind::TypeUint => "uint",
        TokenKind::TypeFloat => "float",
        TokenKind::TypeI8 => "i8",
        TokenKind::TypeI16 => "i16",
        TokenKind::TypeI32 => "i32",
        TokenKind::TypeI64 => "i64",
        TokenKind::TypeU8 => "u8",
        TokenKind::TypeU16 => "u16",
        TokenKind::TypeU32 => "u32",
        TokenKind::TypeU64 => "u64",
        TokenKind::TypeF32 => "f32",
        TokenKind::TypeF64 => "f64",
        TokenKind::TypeBool => "bool",
        TokenKind::TypeByte => "byte",
        TokenKind::TypeChar => "char",
        TokenKind::TypeStr => "str",
        TokenKind::TypeAny => "any",
        TokenKind::TypeErr => "Err",
        _ => return None,
    })
}

pub(super) fn token_expectation_names(name: &str) -> &'static [&'static str] {
    match name {
        "AT" => &["@"],
        "COLON" => &[":"],
        "COMMA" => &[","],
        "DOT" => &["."],
        "EQUAL" => &["="],
        "FAT_ARROW" => &["=>"],
        "GT" => &[">"],
        "INTERP_END" => &["}"],
        "LBRACE" => &["{"],
        "LBRACKET" => &["["],
        "LPAREN" => &["("],
        "LT" => &["<"],
        "RBRACE" => &["}"],
        "RBRACKET" => &["]"],
        "RPAREN" => &[")"],
        "SEMICOLON" => &["statement terminator"],
        "STRING_END" => &["string end"],
        "STRING_START" => &["string literal"],
        "KW_FROM" => &["from"],
        "KW_FUNC" => &["func"],
        "KW_MODULE" => &["module"],
        _ => &["token"],
    }
}

pub(super) fn merge_text_parts(parts: Vec<StringPart>) -> Vec<StringPart> {
    let mut merged = Vec::new();
    for part in parts {
        match (merged.last_mut(), part) {
            (
                Some(StringPart::Text {
                    span: existing_span,
                    text: existing,
                }),
                StringPart::Text { span, text: next },
            ) => {
                existing.push_str(&next);
                *existing_span = span_between(*existing_span, span);
            }
            (_, part) => merged.push(part),
        }
    }
    merged
}

pub(super) fn is_contextual_module_segment(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::KwMatch
            | TokenKind::KwAsync
            | TokenKind::KwExtern
            | TokenKind::KwStruct
            | TokenKind::KwEnum
            | TokenKind::KwInterface
            | TokenKind::KwFunc
            | TokenKind::KwType
            | TokenKind::KwConst
            | TokenKind::KwPublic
            | TokenKind::KwFrom
            | TokenKind::KwAs
    )
}

pub(super) fn span_between(start: Span, end: Span) -> Span {
    Span {
        file_id: start.file_id,
        start: start.start,
        end: end.end,
        start_line: start.start_line,
        start_col: start.start_col,
        end_line: end.end_line,
        end_col: end.end_col,
    }
}
