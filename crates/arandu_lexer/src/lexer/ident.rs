use crate::TokenKind;

pub(super) fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}

pub(super) fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

pub(super) fn keyword_kind(text: &str) -> Option<TokenKind> {
    Some(match text {
        "if" => TokenKind::KwIf,
        "else" => TokenKind::KwElse,
        "for" => TokenKind::KwFor,
        "in" => TokenKind::KwIn,
        "while" => TokenKind::KwWhile,
        "match" => TokenKind::KwMatch,
        "return" => TokenKind::KwReturn,
        "break" => TokenKind::KwBreak,
        "continue" => TokenKind::KwContinue,
        "func" => TokenKind::KwFunc,
        "async" => TokenKind::KwAsync,
        "await" => TokenKind::KwAwait,
        "struct" => TokenKind::KwStruct,
        "enum" => TokenKind::KwEnum,
        "interface" => TokenKind::KwInterface,
        "const" => TokenKind::KwConst,
        "type" => TokenKind::KwType,
        "module" => TokenKind::KwModule,
        "import" => TokenKind::KwImport,
        "from" => TokenKind::KwFrom,
        "as" => TokenKind::KwAs,
        "public" => TokenKind::KwPublic,
        "extern" => TokenKind::KwExtern,
        "unsafe" => TokenKind::KwUnsafe,
        "where" => TokenKind::KwWhere,
        "catch" => TokenKind::KwCatch,
        "is" => TokenKind::KwIs,
        "set" => TokenKind::KwSet,
        "own" => TokenKind::KwOwn,
        "mut" => TokenKind::KwMut,
        "shared" => TokenKind::KwShared,
        "self" => TokenKind::KwSelf,
        "ptr" => TokenKind::KwPtr,
        "alloc" => TokenKind::KwAlloc,
        "free" => TokenKind::KwFree,
        "defer" => TokenKind::KwDefer,
        "errdefer" => TokenKind::KwErrdefer,
        "int" => TokenKind::TypeInt,
        "uint" => TokenKind::TypeUint,
        "float" => TokenKind::TypeFloat,
        "i8" => TokenKind::TypeI8,
        "i16" => TokenKind::TypeI16,
        "i32" => TokenKind::TypeI32,
        "i64" => TokenKind::TypeI64,
        "u8" => TokenKind::TypeU8,
        "u16" => TokenKind::TypeU16,
        "u32" => TokenKind::TypeU32,
        "u64" => TokenKind::TypeU64,
        "f32" => TokenKind::TypeF32,
        "f64" => TokenKind::TypeF64,
        "bool" => TokenKind::TypeBool,
        "byte" => TokenKind::TypeByte,
        "char" => TokenKind::TypeChar,
        "str" => TokenKind::TypeStr,
        "any" => TokenKind::TypeAny,
        "Err" => TokenKind::TypeErr,
        "true" => TokenKind::BoolTrue,
        "false" => TokenKind::BoolFalse,
        "nil" => TokenKind::Nil,
        _ => return None,
    })
}
