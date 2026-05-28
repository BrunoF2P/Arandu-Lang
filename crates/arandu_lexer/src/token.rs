use std::fmt;

use crate::LexErrorCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub file_id: usize,
    pub start: usize,
    pub end: usize,
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl Span {
    #[must_use]
    pub fn new(
        start: usize,
        end: usize,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) -> Self {
        Self {
            file_id: 0,
            start,
            end,
            start_line,
            start_col,
            end_line,
            end_col,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub inserted: bool,
}

impl Token {
    #[must_use]
    pub fn lexeme<'a>(&self, source: &'a str) -> &'a str {
        if self.inserted || matches!(self.kind, TokenKind::Error(_) | TokenKind::Eof) {
            return "";
        }
        &source[self.span.start..self.span.end]
    }

    #[must_use]
    pub fn raw_string_content<'a>(&self, source: &'a str) -> &'a str {
        let lexeme = self.lexeme(source);
        if let Some(stripped) = lexeme.strip_prefix("r\"\"\"") {
            return stripped.strip_suffix("\"\"\"").unwrap_or("");
        }
        if let Some(stripped) = lexeme.strip_prefix("r\"") {
            return stripped.strip_suffix('"').unwrap_or("");
        }
        ""
    }

    #[must_use]
    pub fn char_content<'a>(&self, source: &'a str) -> &'a str {
        let lexeme = self.lexeme(source);
        lexeme
            .strip_prefix('\'')
            .and_then(|text| text.strip_suffix('\''))
            .unwrap_or("")
    }

    #[must_use]
    pub fn dump(&self, source: &str) -> String {
        if self.kind == TokenKind::Semicolon && self.inserted {
            "SEMICOLON(inserted)".to_string()
        } else {
            self.kind.display_with(self, source)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    IdentValue,
    IdentType,
    DocComment,
    IntDec,
    IntHex,
    IntBin,
    IntOct,
    Float,
    StringStart,
    StringText,
    StringEscape,
    InterpStart,
    InterpEnd,
    StringEnd,
    RawString,
    MultilineStringStart,
    MultilineStringEnd,
    Char,
    KwIf,
    KwElse,
    KwFor,
    KwIn,
    KwWhile,
    KwMatch,
    KwReturn,
    KwBreak,
    KwContinue,
    KwFunc,
    KwAsync,
    KwAwait,
    KwStruct,
    KwEnum,
    KwInterface,
    KwConst,
    KwType,
    KwModule,
    KwImport,
    KwFrom,
    KwAs,
    KwPublic,
    KwExtern,
    KwUnsafe,
    KwWhere,
    KwCatch,
    KwIs,
    KwSet,
    KwOwn,
    KwMut,
    KwShared,
    KwSelf,
    KwPtr,
    KwAlloc,
    KwFree,
    KwDefer,
    KwErrdefer,
    TypeInt,
    TypeUint,
    TypeFloat,
    TypeI8,
    TypeI16,
    TypeI32,
    TypeI64,
    TypeU8,
    TypeU16,
    TypeU32,
    TypeU64,
    TypeF32,
    TypeF64,
    TypeBool,
    TypeByte,
    TypeChar,
    TypeStr,
    TypeAny,
    TypeErr,
    BoolTrue,
    BoolFalse,
    Nil,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Dot,
    Colon,
    Semicolon,
    At,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    Pipe,
    Caret,
    Lt,
    Gt,
    Equal,
    Bang,
    Tilde,
    Question,
    SafeDot,
    SafeIndexStart,
    NullCoalesce,
    LogicalOr,
    LogicalAnd,
    FatArrow,
    PlusEqual,
    MinusEqual,
    StarEqual,
    SlashEqual,
    PercentEqual,
    AmpEqual,
    PipeEqual,
    CaretEqual,
    ShiftLeftEqual,
    ShiftRightEqual,
    ShiftLeft,
    ShiftRight,
    EqualEqual,
    BangEqual,
    LtEqual,
    GtEqual,
    RangeInclusive,
    RangeExclusive,
    Ellipsis,
    Eof,
    Error(LexErrorCode),
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenKind::Error(code) => write!(f, "ERROR({code:?})"),
            other => f.write_str(other.name()),
        }
    }
}

impl TokenKind {
    #[must_use]
    pub fn display_with(self, token: &Token, source: &str) -> String {
        match self {
            TokenKind::IdentValue => format!("IDENT_VALUE({})", token.lexeme(source)),
            TokenKind::IdentType => format!("IDENT_TYPE({})", token.lexeme(source)),
            TokenKind::DocComment => format!("DOC_COMMENT({})", token.lexeme(source)),
            TokenKind::IntDec => format!("INT_DEC({})", token.lexeme(source)),
            TokenKind::IntHex => format!("INT_HEX({})", token.lexeme(source)),
            TokenKind::IntBin => format!("INT_BIN({})", token.lexeme(source)),
            TokenKind::IntOct => format!("INT_OCT({})", token.lexeme(source)),
            TokenKind::Float => format!("FLOAT({})", token.lexeme(source)),
            TokenKind::StringText => format!("STRING_TEXT({})", token.lexeme(source)),
            TokenKind::StringEscape => format!("STRING_ESCAPE({})", token.lexeme(source)),
            TokenKind::RawString => {
                format!("RAW_STRING({})", token.raw_string_content(source))
            }
            TokenKind::Char => format!("CHAR({})", token.char_content(source)),
            TokenKind::Error(code) => format!("ERROR({code:?})"),
            other => other.name().to_string(),
        }
    }

    #[must_use]
    pub fn name(&self) -> &'static str {
        crate::token_name::name(self)
    }

    #[must_use]
    pub fn can_end_statement(self) -> bool {
        matches!(
            self,
            TokenKind::IdentValue
                | TokenKind::IdentType
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
                | TokenKind::IntDec
                | TokenKind::IntHex
                | TokenKind::IntBin
                | TokenKind::IntOct
                | TokenKind::Float
                | TokenKind::BoolTrue
                | TokenKind::BoolFalse
                | TokenKind::Nil
                | TokenKind::Char
                | TokenKind::StringEnd
                | TokenKind::RawString
                | TokenKind::MultilineStringEnd
                | TokenKind::RParen
                | TokenKind::RBracket
                | TokenKind::RBrace
                | TokenKind::Question
                | TokenKind::KwReturn
                | TokenKind::KwBreak
                | TokenKind::KwContinue
        )
    }

    #[must_use]
    pub fn prevents_semicolon_before(self) -> bool {
        matches!(
            self,
            TokenKind::RParen
                | TokenKind::RBracket
                | TokenKind::Comma
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Percent
                | TokenKind::Amp
                | TokenKind::Pipe
                | TokenKind::Caret
                | TokenKind::ShiftLeft
                | TokenKind::ShiftRight
                | TokenKind::Dot
                | TokenKind::SafeDot
                | TokenKind::SafeIndexStart
                | TokenKind::Question
                | TokenKind::NullCoalesce
                | TokenKind::LogicalOr
                | TokenKind::LogicalAnd
                | TokenKind::Equal
                | TokenKind::EqualEqual
                | TokenKind::BangEqual
                | TokenKind::Lt
                | TokenKind::Gt
                | TokenKind::LtEqual
                | TokenKind::GtEqual
                | TokenKind::RangeExclusive
                | TokenKind::RangeInclusive
                | TokenKind::FatArrow
                | TokenKind::KwElse
                | TokenKind::KwCatch
                | TokenKind::KwAs
                | TokenKind::KwWhere
                | TokenKind::KwFrom
        )
    }
}
