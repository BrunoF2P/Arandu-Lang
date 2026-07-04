mod decl;
mod error;
mod expr;
mod pattern;
mod stmt;
mod types;

pub use error::{ParseError, ParseErrorCode};

use arandu_lexer::{Span, Token, TokenKind};

use crate::{
    Attribute, BinaryOp, BindingItem, Block, CatchHandler, Condition, ConstDecl, DeferBody,
    DocCommentAttachment, EnumDecl, EnumPayload, EnumVariant, Expr, ExternDecl, FieldDecl,
    FieldInit, ForBinding, ForClause, FuncDecl, FuncName, FuncSignature, GenericParam, ImportDecl,
    ImportItem, InterfaceDecl, LambdaBody, LambdaParam, MatchArm, MatchArmBody, ModuleDecl,
    Ownership, Param, Pattern, Place, PlaceSuffix, Program, ResultType, SetOp, SimpleStmt, Stmt,
    StringPart, StructDecl, TopLevelDecl, TypeAliasDecl, TypeExpr, TypeName, UnaryOp, Visibility,
    WhereItem,
};

#[derive(Debug, Clone)]
pub struct ParseOutput {
    pub program: Program,
    pub diagnostics: Vec<ParseError>,
}

/// Parses source, stopping at the first parse error.
///
/// # Errors
///
/// Returns the first [`ParseError`] if the source is invalid.
pub fn parse(source: &str) -> Result<Program, ParseError> {
    parse_with_file_id(source, 0)
}

#[tracing::instrument(level = "trace", target = "arandu_parser", skip(source))]
pub fn parse_with_file_id(source: &str, file_id: u32) -> Result<Program, ParseError> {
    let output = parse_recovering_with_file_id(source, file_id);
    if let Some(err) = output.diagnostics.into_iter().next() {
        Err(err)
    } else {
        Ok(output.program)
    }
}

pub fn parse_recovering(source: &str) -> ParseOutput {
    parse_recovering_with_file_id(source, 0)
}

pub fn parse_recovering_with_file_id(source: &str, file_id: u32) -> ParseOutput {
    let lexed = arandu_lexer::lex_recovering(source);
    let mut parser = Parser::new(lexed.source, lexed.tokens).with_file_id(file_id);
    let mut diagnostics: Vec<ParseError> = lexed
        .diagnostics
        .into_iter()
        .map(|err| ParseError::from_lex(err, file_id))
        .collect();

    // We expect parse_program to finish without returning Err
    // for recoverable nodes, but if it does return Err (e.g. at EOF), we catch it.
    let program = match parser.parse_program() {
        Ok(prog) => prog,
        Err(err) => {
            parser.diagnostics.push(err);
            // Construct a fallback program
            Program {
                span: Span {
                    file_id: parser.file_id,
                    start: 0,
                    end: 0,
                },
                module: None,
                imports: Vec::new(),
                decls: Vec::new(),
                docs: Vec::new(),
                pool: parser.pool.clone(),
            }
        }
    };

    diagnostics.extend(parser.diagnostics);

    ParseOutput {
        program,
        diagnostics,
    }
}

/// Parses source and returns an AST dump string.
///
/// # Errors
///
/// Returns the first [`ParseError`] if the source is invalid.
pub fn parse_to_string(source: &str) -> Result<String, ParseError> {
    Ok(parse(source)?.dump(source))
}

pub struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    allow_block_calls: bool,
    docs: Vec<DocCommentAttachment>,
    pending_docs: Vec<PendingDoc>,
    pub pool: crate::ast::ast_pool::AstPool,
    pub(super) diagnostics: Vec<ParseError>,
    pub file_id: u32,
    pub suppression_window: u32,
}

#[derive(Debug, Clone)]
pub(super) struct PendingDoc {
    span: Span,
    text: String,
}

impl<'a> Parser<'a> {
    #[must_use]
    pub fn new(source: &'a str, tokens: Vec<Token>) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
            allow_block_calls: true,
            docs: Vec::new(),
            pending_docs: Vec::new(),
            diagnostics: Vec::new(),
            pool: crate::ast::ast_pool::AstPool::new(),
            file_id: 0,
            suppression_window: 0,
        }
    }

    #[must_use]
    pub fn with_file_id(mut self, file_id: u32) -> Self {
        self.file_id = file_id;
        self
    }

    /// Parses a full program, collecting recoverable errors in [`Self::diagnostics`].
    ///
    /// # Errors
    ///
    /// Returns a fatal [`ParseError`] when parsing cannot continue (for example at EOF).
    pub fn parse_program(&mut self) -> Result<Program, ParseError> {
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
                match self.parse_import() {
                    Ok(import) => imports.push(import),
                    Err(err) => {
                        self.report_error(err);
                        self.synchronize_top_level();
                    }
                }
                continue;
            }
            match self.parse_top_level_decl() {
                Ok(decl) => {
                    let decl_id = self.pool.alloc_decl(decl);
                    decls.push(decl_id);
                }
                Err(err) => {
                    self.report_error(err);
                    self.synchronize_top_level();
                }
            }
        }

        Ok(Program {
            span: self.span_from_mark(start),
            module,
            imports,
            decls,
            docs: std::mem::take(&mut self.docs),
            pool: std::mem::take(&mut self.pool),
        })
    }
    pub(super) fn mark(&self) -> usize {
        self.pos
    }

    pub(super) fn span_from_mark(&self, start: usize) -> Span {
        let start_span = self.tokens.get(start).map_or_else(
            || self.current().span(self.file_id),
            |token| token.span(self.file_id),
        );
        let end_span = if self.pos == start {
            start_span
        } else {
            self.tokens
                .get(self.pos.saturating_sub(1))
                .map_or(start_span, |token| token.span(self.file_id))
        };
        span_between(start_span, end_span)
    }

    pub(super) fn skip_semicolons(&mut self) {
        while self.at_kind_name("SEMICOLON") {
            self.advance();
        }
    }

    pub(super) fn collect_doc_comments(&mut self) {
        while matches!(self.current().kind, TokenKind::DocComment) {
            self.pending_docs.push(PendingDoc {
                span: self.current().span(self.file_id),
                text: self.current_text().to_string(),
            });
            self.advance();
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
        self.expect_name("SEMICOLON")
    }

    pub(super) fn expect_ident_value(&mut self) -> Result<String, ParseError> {
        match &self.current().kind {
            TokenKind::IdentValue => {
                let name = self.current_text().to_string();
                self.advance();
                Ok(name)
            }
            _ => Err(ParseError::expected(
                ParseErrorCode::ExpectedToken,
                "expected value identifier",
                self.current(),
                self.file_id,
                self.source,
                &["value identifier"],
            )),
        }
    }

    pub(super) fn expect_ident_type(&mut self) -> Result<String, ParseError> {
        match &self.current().kind {
            TokenKind::IdentType | TokenKind::TypeErr => {
                let name = self.current_text().to_string();
                self.advance();
                Ok(name)
            }
            _ => Err(ParseError::expected(
                ParseErrorCode::ExpectedToken,
                "expected type identifier",
                self.current(),
                self.file_id,
                self.source,
                &["type identifier"],
            )),
        }
    }

    pub(super) fn expect_name_like(&mut self) -> Result<String, ParseError> {
        match &self.current().kind {
            TokenKind::TypeErr | TokenKind::IdentValue | TokenKind::IdentType => {
                let name = self.current_text().to_string();
                self.advance();
                Ok(name)
            }
            _ => Err(ParseError::expected(
                ParseErrorCode::ExpectedToken,
                "expected identifier",
                self.current(),
                self.file_id,
                self.source,
                &["identifier"],
            )),
        }
    }

    pub(super) fn expect_module_segment(&mut self) -> Result<String, ParseError> {
        match &self.current().kind {
            TokenKind::IdentValue => {
                let name = self.current_text().to_string();
                self.advance();
                Ok(name)
            }
            kind if is_contextual_module_segment(kind) => {
                let text = self.current_text().to_string();
                self.advance();
                Ok(text)
            }
            _ => Err(ParseError::expected(
                ParseErrorCode::ExpectedToken,
                "expected module path segment",
                self.current(),
                self.file_id,
                self.source,
                &["module path segment"],
            )),
        }
    }

    pub(super) fn expect_name(&mut self, name: &str) -> Result<(), ParseError> {
        if self.at_kind_name(name) {
            self.advance();
            Ok(())
        } else {
            Err(ParseError::expected(
                ParseErrorCode::ExpectedToken,
                format!("expected {name}"),
                self.current(),
                self.file_id,
                self.source,
                token_expectation_names(name),
            ))
        }
    }

    pub(super) fn eat_name(&mut self, name: &str) -> bool {
        if self.at_kind_name(name) {
            self.advance();
            true
        } else {
            false
        }
    }

    pub(super) fn expect_kind(&mut self, kind: TokenKind) -> Result<(), ParseError> {
        if self.current().kind == kind {
            self.advance();
            Ok(())
        } else {
            let name = kind.name();
            Err(ParseError::expected(
                ParseErrorCode::ExpectedToken,
                format!("expected {name}"),
                self.current(),
                self.file_id,
                self.source,
                token_expectation_names(name),
            ))
        }
    }

    pub(super) fn eat_kind(&mut self, kind: TokenKind) -> bool {
        if self.current().kind == kind {
            self.advance();
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

    pub(super) fn token_text(&self, token: &Token) -> &str {
        token.lexeme(self.source)
    }

    pub(super) fn current_text(&self) -> &str {
        self.token_text(self.current())
    }

    pub(super) fn previous(&self) -> &Token {
        &self.tokens[self.pos - 1]
    }

    pub(super) fn advance(&mut self) -> &Token {
        self.suppression_window += 1;
        self.advance_raw()
    }

    pub(super) fn advance_raw(&mut self) -> &Token {
        let token = &self.tokens[self.pos];
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        token
    }

    pub(super) fn report_error(&mut self, err: ParseError) {
        if self.suppression_window >= 3 {
            self.diagnostics.push(err);
        }
        self.suppression_window = 0;
    }

    #[cold]
    #[inline(never)]
    pub(super) fn synchronize_top_level(&mut self) {
        if matches!(
            self.current().kind,
            TokenKind::KwFunc
                | TokenKind::KwStruct
                | TokenKind::KwEnum
                | TokenKind::KwInterface
                | TokenKind::KwExtern
                | TokenKind::KwType
                | TokenKind::KwConst
                | TokenKind::KwImport
                | TokenKind::KwModule
        ) {
            return;
        }
        self.advance_raw();
        while !self.at_kind_name("EOF") {
            if matches!(
                self.current().kind,
                TokenKind::KwFunc
                    | TokenKind::KwStruct
                    | TokenKind::KwEnum
                    | TokenKind::KwInterface
                    | TokenKind::KwExtern
                    | TokenKind::KwType
                    | TokenKind::KwConst
                    | TokenKind::KwImport
                    | TokenKind::KwModule
            ) {
                break;
            }
            self.advance_raw();
        }
    }

    #[cold]
    #[inline(never)]
    pub(super) fn synchronize_stmt(&mut self) {
        if matches!(
            self.current().kind,
            TokenKind::RBrace
                | TokenKind::KwReturn
                | TokenKind::KwIf
                | TokenKind::KwFor
                | TokenKind::KwWhile
                | TokenKind::KwMatch
                | TokenKind::KwBreak
                | TokenKind::KwContinue
                | TokenKind::KwDefer
                | TokenKind::KwErrdefer
        ) {
            return;
        }
        self.advance_raw();
        while !self.at_kind_name("EOF") {
            if self.previous().kind == TokenKind::Semicolon {
                break;
            }
            if matches!(
                self.current().kind,
                TokenKind::RBrace
                    | TokenKind::KwReturn
                    | TokenKind::KwIf
                    | TokenKind::KwFor
                    | TokenKind::KwWhile
                    | TokenKind::KwMatch
                    | TokenKind::KwBreak
                    | TokenKind::KwContinue
                    | TokenKind::KwDefer
                    | TokenKind::KwErrdefer
            ) {
                break;
            }
            self.advance_raw();
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
    TOKEN_INFO_TABLE[kind.index()].is_type_token
}

pub(super) fn primitive_type_name(kind: &TokenKind) -> Option<&'static str> {
    TOKEN_INFO_TABLE[kind.index()].primitive_type_name
}

#[derive(Clone, Copy)]
struct TokenInfo {
    is_type_token: bool,
    is_contextual_module_segment: bool,
    primitive_type_name: Option<&'static str>,
}

static TOKEN_INFO_TABLE: [TokenInfo; TokenKind::COUNT] = {
    let mut table = [TokenInfo {
        is_type_token: false,
        is_contextual_module_segment: false,
        primitive_type_name: None,
    }; TokenKind::COUNT];
    let mut i = 0;
    while i < TokenKind::COUNT {
        let kind = TokenKind::index_to_token_kind(i);
        let prim = match kind {
            TokenKind::TypeInt => Some("int"),
            TokenKind::TypeUint => Some("uint"),
            TokenKind::TypeFloat => Some("float"),
            TokenKind::TypeI8 => Some("i8"),
            TokenKind::TypeI16 => Some("i16"),
            TokenKind::TypeI32 => Some("i32"),
            TokenKind::TypeI64 => Some("i64"),
            TokenKind::TypeU8 => Some("u8"),
            TokenKind::TypeU16 => Some("u16"),
            TokenKind::TypeU32 => Some("u32"),
            TokenKind::TypeU64 => Some("u64"),
            TokenKind::TypeF32 => Some("f32"),
            TokenKind::TypeF64 => Some("f64"),
            TokenKind::TypeBool => Some("bool"),
            TokenKind::TypeByte => Some("byte"),
            TokenKind::TypeChar => Some("char"),
            TokenKind::TypeStr => Some("str"),
            TokenKind::TypeAny => Some("any"),
            TokenKind::TypeErr => Some("Err"),
            _ => None,
        };
        let is_type =
            prim.is_some() || matches!(kind, TokenKind::IdentType | TokenKind::IdentValue);
        let is_contextual = matches!(
            kind,
            TokenKind::IdentType
                | TokenKind::KwIf
                | TokenKind::KwElse
                | TokenKind::KwFor
                | TokenKind::KwIn
                | TokenKind::KwWhile
                | TokenKind::KwMatch
                | TokenKind::KwReturn
                | TokenKind::KwBreak
                | TokenKind::KwContinue
                | TokenKind::KwFunc
                | TokenKind::KwAsync
                | TokenKind::KwAwait
                | TokenKind::KwStruct
                | TokenKind::KwEnum
                | TokenKind::KwInterface
                | TokenKind::KwConst
                | TokenKind::KwType
                | TokenKind::KwModule
                | TokenKind::KwImport
                | TokenKind::KwFrom
                | TokenKind::KwAs
                | TokenKind::KwPublic
                | TokenKind::KwExtern
                | TokenKind::KwUnsafe
                | TokenKind::KwWhere
                | TokenKind::KwCatch
                | TokenKind::KwIs
                | TokenKind::KwSet
                | TokenKind::KwOwn
                | TokenKind::KwMut
                | TokenKind::KwShared
                | TokenKind::KwSelf
                | TokenKind::KwPtr
                | TokenKind::KwDefer
                | TokenKind::KwErrdefer
                | TokenKind::KwLet
                | TokenKind::TypeInt
                | TokenKind::TypeUint
                | TokenKind::TypeFloat
                | TokenKind::TypeI8
                | TokenKind::TypeI16
                | TokenKind::TypeI32
                | TokenKind::TypeI64
                | TokenKind::TypeU8
                | TokenKind::TypeU16
                | TokenKind::TypeU32
                | TokenKind::TypeU64
                | TokenKind::TypeF32
                | TokenKind::TypeF64
                | TokenKind::TypeBool
                | TokenKind::TypeByte
                | TokenKind::TypeChar
                | TokenKind::TypeStr
                | TokenKind::TypeAny
                | TokenKind::TypeErr
        );
        table[i] = TokenInfo {
            is_type_token: is_type,
            is_contextual_module_segment: is_contextual,
            primitive_type_name: prim,
        };
        i += 1;
    }
    table
};

pub(super) fn token_expectation_names(name: &str) -> &'static [&'static str] {
    match name {
        "AT" => &["@"],
        "COLON" => &[":"],
        "COMMA" => &[","],
        "DOT" => &["."],
        "EQUAL" => &["="],
        "FAT_ARROW" => &["=>"],
        "GT" => &[">"],
        "INTERP_END" | "RBRACE" => &["}"],
        "LBRACE" => &["{"],
        "LBRACKET" => &["["],
        "LPAREN" => &["("],
        "LT" => &["<"],
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
    TOKEN_INFO_TABLE[kind.index()].is_contextual_module_segment
}

pub(super) fn span_between(start: Span, end: Span) -> Span {
    Span {
        file_id: start.file_id,
        start: start.start,
        end: end.end,
    }
}
