//! Build a green tree from lexer tokens (+ optional AST item spans).

use super::kind::{AranduLanguage, SyntaxKind, SyntaxNode};
use arandu_lexer::{Token, TokenKind, lex_recovering};
use rowan::{GreenNode, GreenNodeBuilder, TextRange, TextSize};

/// Result of building a CST.
#[derive(Debug, Clone)]
pub struct SyntaxTree {
    green: GreenNode,
    /// Original source (owned) so red tree text stays valid.
    text: String,
}

impl SyntaxTree {
    #[must_use]
    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Red tree rooted at [`SyntaxKind::SOURCE_FILE`].
    #[must_use]
    pub fn root(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    /// All top-level [`SyntaxKind::ITEM`] children.
    #[must_use]
    pub fn items(&self) -> Vec<SyntaxNode> {
        self.root()
            .children()
            .filter(|n| n.kind() == SyntaxKind::ITEM)
            .collect()
    }

    /// Text content of each ITEM node (stable for unchanged items under green interning).
    #[must_use]
    pub fn item_texts(&self) -> Vec<String> {
        self.items()
            .into_iter()
            .map(|n| n.text().to_string())
            .collect()
    }
}

/// Build a lossless-ish CST from source alone (flat tokens under SOURCE_FILE).
#[must_use]
pub fn parse_syntax(source: &str) -> SyntaxTree {
    parse_syntax_with_item_spans(source, &[])
}

/// Dual path: group tokens into ITEM nodes using AST top-level decl spans.
///
/// Spans are byte offsets into `source` (same as lexer/AST). Overlapping or
/// out-of-order spans are clamped / skipped.
#[must_use]
pub fn parse_syntax_with_item_spans(source: &str, item_spans: &[(u32, u32)]) -> SyntaxTree {
    let lexed = lex_recovering(source);
    let green = build_green(source, &lexed.tokens, item_spans);
    SyntaxTree {
        green,
        text: source.to_string(),
    }
}

fn build_green(source: &str, tokens: &[Token], item_spans: &[(u32, u32)]) -> GreenNode {
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(SyntaxKind::SOURCE_FILE.into());

    if item_spans.is_empty() {
        emit_tokens(&mut builder, source, tokens, 0, source.len() as u32);
    } else {
        let mut spans: Vec<(u32, u32)> = item_spans
            .iter()
            .copied()
            .filter(|(s, e)| e > s && (*s as usize) < source.len())
            .collect();
        spans.sort_by_key(|(s, _)| *s);

        let mut cursor = 0u32;
        for (start, end) in spans {
            let start = start.max(cursor);
            let end = end.min(source.len() as u32).max(start);
            if start > cursor {
                emit_tokens(&mut builder, source, tokens, cursor, start);
            }
            if end > start {
                builder.start_node(SyntaxKind::ITEM.into());
                emit_tokens(&mut builder, source, tokens, start, end);
                builder.finish_node();
            }
            cursor = end;
        }
        if (cursor as usize) < source.len() {
            emit_tokens(&mut builder, source, tokens, cursor, source.len() as u32);
        }
    }

    builder.finish_node();
    builder.finish()
}

fn emit_tokens(
    builder: &mut GreenNodeBuilder<'_>,
    source: &str,
    tokens: &[Token],
    range_start: u32,
    range_end: u32,
) {
    let mut cursor = range_start;
    for tok in tokens {
        if matches!(tok.kind, TokenKind::Eof | TokenKind::Error(_)) {
            continue;
        }
        let ts = tok.start;
        let te = tok.start.saturating_add(tok.len);
        if te <= range_start || ts >= range_end {
            continue;
        }
        // Fill gaps (whitespace/newlines not always tokenized).
        if ts > cursor {
            let gs = cursor as usize;
            let ge = ts.min(range_end) as usize;
            if ge > gs {
                let gap = &source[gs..ge];
                if !gap.is_empty() {
                    builder.token(SyntaxKind::WHITESPACE.into(), gap);
                }
            }
            cursor = ts.min(range_end);
        }
        let s = ts.max(range_start) as usize;
        let e = te.min(range_end) as usize;
        if e <= s {
            continue;
        }
        let text = &source[s..e];
        let kind = map_token_kind(tok.kind);
        builder.token(kind.into(), text);
        cursor = te.min(range_end);
    }
    if cursor < range_end {
        let gap = &source[cursor as usize..range_end as usize];
        if !gap.is_empty() {
            builder.token(SyntaxKind::WHITESPACE.into(), gap);
        }
    }
}

fn map_token_kind(kind: TokenKind) -> SyntaxKind {
    match kind {
        TokenKind::DocComment => SyntaxKind::COMMENT,
        // Lexer may not emit pure WS tokens — gaps are filled only by token text.
        // If future WS tokens appear, map them here.
        _ => SyntaxKind::TOKEN,
    }
}

/// Apply a full-document style byte-range replacement.
#[must_use]
pub fn apply_text_edit(source: &str, start: u32, end: u32, replacement: &str) -> String {
    let start = (start as usize).min(source.len());
    let end = (end as usize).min(source.len()).max(start);
    let mut out = String::with_capacity(source.len() - (end - start) + replacement.len());
    out.push_str(&source[..start]);
    out.push_str(replacement);
    out.push_str(&source[end..]);
    out
}

/// Reparse after an edit.
///
/// **v1 strategy:** rebuild the full green tree. Unchanged ITEM subtrees share
/// green storage via rowan's interning when text content matches (measurable
/// item-text equality in tests). Item-span dual uses AST spans when provided.
///
/// Returns `(new_source, new_tree)`.
#[must_use]
pub fn reparse_edit(
    old_source: &str,
    start: u32,
    end: u32,
    replacement: &str,
    item_spans: &[(u32, u32)],
) -> (String, SyntaxTree) {
    let new_source = apply_text_edit(old_source, start, end, replacement);
    // Shift item spans after the edit for dual grouping.
    let delta = replacement.len() as i64 - (end.saturating_sub(start) as i64);
    let new_spans: Vec<(u32, u32)> = item_spans
        .iter()
        .filter_map(|&(s, e)| {
            if e <= start {
                Some((s, e))
            } else if s >= end {
                let s2 = (s as i64 + delta).max(0) as u32;
                let e2 = (e as i64 + delta).max(0) as u32;
                Some((s2, e2))
            } else {
                // Overlaps edit — drop span; item will be re-detected next dual parse with AST.
                None
            }
        })
        .collect();
    let tree = parse_syntax_with_item_spans(&new_source, &new_spans);
    (new_source, tree)
}

/// TextRange helper for tests.
#[must_use]
pub fn text_range(start: u32, end: u32) -> TextRange {
    TextRange::new(TextSize::from(start), TextSize::from(end))
}

/// Phantom use of AranduLanguage so type aliases stay consistent.
#[allow(dead_code)]
fn _lang() -> AranduLanguage {
    unreachable!()
}
