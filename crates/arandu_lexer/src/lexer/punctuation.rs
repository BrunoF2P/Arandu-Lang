use crate::TokenKind;

use super::ident::{is_ident_continue, keyword_kind};

pub(super) fn peek_kind_from(rest: &str) -> TokenKind {
    if rest
        .chars()
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
    {
        let end = rest
            .char_indices()
            .find_map(|(index, ch)| (!is_ident_continue(ch)).then_some(index))
            .unwrap_or(rest.len());
        if let Some(kind) = keyword_kind(&rest[..end]) {
            return kind;
        }
    }

    token_kind_from_prefix(rest)
        .map(|(kind, _)| kind)
        .unwrap_or(TokenKind::Eof)
}

pub(super) fn token_kind_from_prefix(rest: &str) -> Option<(TokenKind, usize)> {
    let pairs = [
        ("<<=", TokenKind::ShiftLeftEqual),
        (">>=", TokenKind::ShiftRightEqual),
        ("...", TokenKind::Ellipsis),
        ("?.", TokenKind::SafeDot),
        ("?[", TokenKind::SafeIndexStart),
        ("??", TokenKind::NullCoalesce),
        ("||", TokenKind::LogicalOr),
        ("&&", TokenKind::LogicalAnd),
        ("=>", TokenKind::FatArrow),
        ("+=", TokenKind::PlusEqual),
        ("-=", TokenKind::MinusEqual),
        ("*=", TokenKind::StarEqual),
        ("/=", TokenKind::SlashEqual),
        ("%=", TokenKind::PercentEqual),
        ("&=", TokenKind::AmpEqual),
        ("|=", TokenKind::PipeEqual),
        ("^=", TokenKind::CaretEqual),
        ("<<", TokenKind::ShiftLeft),
        (">>", TokenKind::ShiftRight),
        ("==", TokenKind::EqualEqual),
        ("!=", TokenKind::BangEqual),
        ("<=", TokenKind::LtEqual),
        (">=", TokenKind::GtEqual),
        ("..=", TokenKind::RangeInclusive),
        ("..", TokenKind::RangeExclusive),
    ];
    for (lexeme, kind) in pairs {
        if rest.starts_with(lexeme) {
            return Some((kind, lexeme.len()));
        }
    }

    let ch = rest.chars().next()?;
    let kind = match ch {
        '(' => TokenKind::LParen,
        ')' => TokenKind::RParen,
        '[' => TokenKind::LBracket,
        ']' => TokenKind::RBracket,
        '{' => TokenKind::LBrace,
        '}' => TokenKind::RBrace,
        ',' => TokenKind::Comma,
        '.' => TokenKind::Dot,
        ':' => TokenKind::Colon,
        ';' => TokenKind::Semicolon,
        '@' => TokenKind::At,
        '+' => TokenKind::Plus,
        '-' => TokenKind::Minus,
        '*' => TokenKind::Star,
        '/' => TokenKind::Slash,
        '%' => TokenKind::Percent,
        '&' => TokenKind::Amp,
        '|' => TokenKind::Pipe,
        '^' => TokenKind::Caret,
        '<' => TokenKind::Lt,
        '>' => TokenKind::Gt,
        '=' => TokenKind::Equal,
        '!' => TokenKind::Bang,
        '~' => TokenKind::Tilde,
        '?' => TokenKind::Question,
        _ => return None,
    };
    Some((kind, ch.len_utf8()))
}
