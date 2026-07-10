//! Syntax kinds for the Arandu CST (rowan).

/// Token and composite node kinds for the green/red tree.
///
/// Token kinds are a compact projection of [`arandu_lexer::TokenKind`] plus
/// composite structure for top-level items (P5 dual + item-level reuse).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    // --- trivia / tokens (generic) ---
    WHITESPACE = 0,
    COMMENT,
    /// Any non-trivia lexer token (identity preserved in text, not kind detail).
    TOKEN,
    ERROR_TOKEN,

    // --- composite ---
    /// Root covering the entire file.
    SOURCE_FILE,
    /// One top-level declaration (func/struct/const/…), token range of its span.
    ITEM,
    /// Unparsed / error region.
    ERROR,

    /// Sentinel — must stay last for Language bounds.
    #[doc(hidden)]
    __LAST,
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

impl SyntaxKind {
    #[must_use]
    pub const fn is_trivia(self) -> bool {
        matches!(self, Self::WHITESPACE | Self::COMMENT)
    }
}

/// Rowan language marker for Arandu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AranduLanguage {}

impl rowan::Language for AranduLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        assert!(raw.0 < SyntaxKind::__LAST as u16);
        // SAFETY: raw is produced by kind_to_raw from a valid SyntaxKind.
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type SyntaxNode = rowan::SyntaxNode<AranduLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<AranduLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<AranduLanguage>;
