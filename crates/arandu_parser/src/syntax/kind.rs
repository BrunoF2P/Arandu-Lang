//! Syntax kinds for the Arandu CST (rowan) — CST-first pipeline.

/// Token and composite node kinds for the green/red tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    // --- trivia ---
    WHITESPACE = 0,
    COMMENT,

    // --- tokens (for highlighting) ---
    KEYWORD,
    IDENT,
    TYPE_IDENT,
    NUMBER,
    STRING,
    CHAR,
    PUNCT,
    ERROR_TOKEN,

    // --- composite ---
    /// Root covering the entire file.
    SOURCE_FILE,
    /// Top-level unit: module / import / declaration (func, struct, …).
    ITEM,
    /// Unparsed / error region.
    ERROR,

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

    /// LSP semantic-token friendly name (stable strings).
    #[must_use]
    pub const fn highlight_class(self) -> Option<&'static str> {
        match self {
            Self::KEYWORD => Some("keyword"),
            Self::IDENT => Some("variable"),
            Self::TYPE_IDENT => Some("type"),
            Self::NUMBER => Some("number"),
            Self::STRING => Some("string"),
            Self::CHAR => Some("string"),
            Self::COMMENT => Some("comment"),
            Self::PUNCT => Some("operator"),
            Self::ERROR_TOKEN | Self::ERROR => Some("error"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AranduLanguage {}

impl rowan::Language for AranduLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        assert!(raw.0 < SyntaxKind::__LAST as u16);
        // SAFETY: raw produced by kind_to_raw from a valid SyntaxKind.
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type SyntaxNode = rowan::SyntaxNode<AranduLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<AranduLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<AranduLanguage>;
