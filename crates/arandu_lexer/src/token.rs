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
    #[allow(clippy::too_many_lines)]
    pub fn name(&self) -> &'static str {
        match self {
            TokenKind::IdentValue => "IDENT_VALUE",
            TokenKind::IdentType => "IDENT_TYPE",
            TokenKind::DocComment => "DOC_COMMENT",
            TokenKind::IntDec => "INT_DEC",
            TokenKind::IntHex => "INT_HEX",
            TokenKind::IntBin => "INT_BIN",
            TokenKind::IntOct => "INT_OCT",
            TokenKind::Float => "FLOAT",
            TokenKind::StringStart => "STRING_START",
            TokenKind::StringText => "STRING_TEXT",
            TokenKind::StringEscape => "STRING_ESCAPE",
            TokenKind::InterpStart => "INTERP_START",
            TokenKind::InterpEnd => "INTERP_END",
            TokenKind::StringEnd => "STRING_END",
            TokenKind::RawString => "RAW_STRING",
            TokenKind::MultilineStringStart => "MULTILINE_STRING_START",
            TokenKind::MultilineStringEnd => "MULTILINE_STRING_END",
            TokenKind::Char => "CHAR",
            TokenKind::KwIf => "KW_IF",
            TokenKind::KwElse => "KW_ELSE",
            TokenKind::KwFor => "KW_FOR",
            TokenKind::KwIn => "KW_IN",
            TokenKind::KwWhile => "KW_WHILE",
            TokenKind::KwMatch => "KW_MATCH",
            TokenKind::KwReturn => "KW_RETURN",
            TokenKind::KwBreak => "KW_BREAK",
            TokenKind::KwContinue => "KW_CONTINUE",
            TokenKind::KwFunc => "KW_FUNC",
            TokenKind::KwAsync => "KW_ASYNC",
            TokenKind::KwAwait => "KW_AWAIT",
            TokenKind::KwStruct => "KW_STRUCT",
            TokenKind::KwEnum => "KW_ENUM",
            TokenKind::KwInterface => "KW_INTERFACE",
            TokenKind::KwConst => "KW_CONST",
            TokenKind::KwType => "KW_TYPE",
            TokenKind::KwModule => "KW_MODULE",
            TokenKind::KwImport => "KW_IMPORT",
            TokenKind::KwFrom => "KW_FROM",
            TokenKind::KwAs => "KW_AS",
            TokenKind::KwPublic => "KW_PUBLIC",
            TokenKind::KwExtern => "KW_EXTERN",
            TokenKind::KwUnsafe => "KW_UNSAFE",
            TokenKind::KwWhere => "KW_WHERE",
            TokenKind::KwCatch => "KW_CATCH",
            TokenKind::KwIs => "KW_IS",
            TokenKind::KwSet => "KW_SET",
            TokenKind::KwOwn => "KW_OWN",
            TokenKind::KwMut => "KW_MUT",
            TokenKind::KwShared => "KW_SHARED",
            TokenKind::KwSelf => "KW_SELF",
            TokenKind::KwPtr => "KW_PTR",
            TokenKind::KwAlloc => "KW_ALLOC",
            TokenKind::KwFree => "KW_FREE",
            TokenKind::KwDefer => "KW_DEFER",
            TokenKind::KwErrdefer => "KW_ERRDEFER",
            TokenKind::TypeInt => "TYPE_INT",
            TokenKind::TypeUint => "TYPE_UINT",
            TokenKind::TypeFloat => "TYPE_FLOAT",
            TokenKind::TypeI8 => "TYPE_I8",
            TokenKind::TypeI16 => "TYPE_I16",
            TokenKind::TypeI32 => "TYPE_I32",
            TokenKind::TypeI64 => "TYPE_I64",
            TokenKind::TypeU8 => "TYPE_U8",
            TokenKind::TypeU16 => "TYPE_U16",
            TokenKind::TypeU32 => "TYPE_U32",
            TokenKind::TypeU64 => "TYPE_U64",
            TokenKind::TypeF32 => "TYPE_F32",
            TokenKind::TypeF64 => "TYPE_F64",
            TokenKind::TypeBool => "TYPE_BOOL",
            TokenKind::TypeByte => "TYPE_BYTE",
            TokenKind::TypeChar => "TYPE_CHAR",
            TokenKind::TypeStr => "TYPE_STR",
            TokenKind::TypeAny => "TYPE_ANY",
            TokenKind::TypeErr => "TYPE_ERR",
            TokenKind::BoolTrue => "BOOL_TRUE",
            TokenKind::BoolFalse => "BOOL_FALSE",
            TokenKind::Nil => "NIL",
            TokenKind::LParen => "LPAREN",
            TokenKind::RParen => "RPAREN",
            TokenKind::LBracket => "LBRACKET",
            TokenKind::RBracket => "RBRACKET",
            TokenKind::LBrace => "LBRACE",
            TokenKind::RBrace => "RBRACE",
            TokenKind::Comma => "COMMA",
            TokenKind::Dot => "DOT",
            TokenKind::Colon => "COLON",
            TokenKind::Semicolon => "SEMICOLON",
            TokenKind::At => "AT",
            TokenKind::Plus => "PLUS",
            TokenKind::Minus => "MINUS",
            TokenKind::Star => "STAR",
            TokenKind::Slash => "SLASH",
            TokenKind::Percent => "PERCENT",
            TokenKind::Amp => "AMP",
            TokenKind::Pipe => "PIPE",
            TokenKind::Caret => "CARET",
            TokenKind::Lt => "LT",
            TokenKind::Gt => "GT",
            TokenKind::Equal => "EQUAL",
            TokenKind::Bang => "BANG",
            TokenKind::Tilde => "TILDE",
            TokenKind::Question => "QUESTION",
            TokenKind::SafeDot => "SAFE_DOT",
            TokenKind::SafeIndexStart => "SAFE_INDEX_START",
            TokenKind::NullCoalesce => "NULL_COALESCE",
            TokenKind::LogicalOr => "LOGICAL_OR",
            TokenKind::LogicalAnd => "LOGICAL_AND",
            TokenKind::FatArrow => "FAT_ARROW",
            TokenKind::PlusEqual => "PLUS_EQUAL",
            TokenKind::MinusEqual => "MINUS_EQUAL",
            TokenKind::StarEqual => "STAR_EQUAL",
            TokenKind::SlashEqual => "SLASH_EQUAL",
            TokenKind::PercentEqual => "PERCENT_EQUAL",
            TokenKind::AmpEqual => "AMP_EQUAL",
            TokenKind::PipeEqual => "PIPE_EQUAL",
            TokenKind::CaretEqual => "CARET_EQUAL",
            TokenKind::ShiftLeftEqual => "SHIFT_LEFT_EQUAL",
            TokenKind::ShiftRightEqual => "SHIFT_RIGHT_EQUAL",
            TokenKind::ShiftLeft => "SHIFT_LEFT",
            TokenKind::ShiftRight => "SHIFT_RIGHT",
            TokenKind::EqualEqual => "EQUAL_EQUAL",
            TokenKind::BangEqual => "BANG_EQUAL",
            TokenKind::LtEqual => "LT_EQUAL",
            TokenKind::GtEqual => "GT_EQUAL",
            TokenKind::RangeInclusive => "RANGE_INCLUSIVE",
            TokenKind::RangeExclusive => "RANGE_EXCLUSIVE",
            TokenKind::Ellipsis => "ELLIPSIS",
            TokenKind::Eof => "EOF",
            TokenKind::Error(_) => "ERROR",
        }
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
