//! CST-first green tree: lex → ITEM split → green; lower AST from CST text.

use super::kind::{AranduLanguage, SyntaxKind, SyntaxNode, SyntaxToken};
use arandu_lexer::{Token, TokenKind, lex_recovering};
use rowan::{GreenNode, GreenNodeBuilder, NodeOrToken, TextRange, TextSize};

/// Result of building a CST (authoritative source text + green tree).
#[derive(Debug, Clone)]
pub struct SyntaxTree {
    green: GreenNode,
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

    #[must_use]
    pub fn root(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    #[must_use]
    pub fn items(&self) -> Vec<SyntaxNode> {
        self.root()
            .children()
            .filter(|n| n.kind() == SyntaxKind::ITEM)
            .collect()
    }

    #[must_use]
    pub fn item_texts(&self) -> Vec<String> {
        self.items()
            .into_iter()
            .map(|n| n.text().to_string())
            .collect()
    }

    /// Byte range of each ITEM in source order.
    #[must_use]
    pub fn item_ranges(&self) -> Vec<(u32, u32)> {
        self.items()
            .into_iter()
            .map(|n| {
                let r = n.text_range();
                (u32::from(r.start()), u32::from(r.end()))
            })
            .collect()
    }

    /// Index of the ITEM covering `offset` (byte), if any.
    #[must_use]
    pub fn item_index_at(&self, offset: u32) -> Option<usize> {
        let off = TextSize::from(offset);
        self.items().into_iter().position(|n| {
            let r = n.text_range();
            r.start() <= off && off < r.end() || (off == r.end() && r.start() < r.end())
        })
    }
}

/// CST-first parse: top-level ITEM boundaries from lexer heuristics (no AST).
#[must_use]
pub fn parse_syntax(source: &str) -> SyntaxTree {
    let lexed = lex_recovering(source);
    let spans = find_top_level_item_spans(&lexed.tokens, source.len() as u32);
    let green = build_green(source, &lexed.tokens, &spans);
    SyntaxTree {
        green,
        text: source.to_string(),
    }
}

/// Build CST with explicit item spans (advanced / tests).
#[must_use]
pub fn parse_syntax_with_item_spans(source: &str, item_spans: &[(u32, u32)]) -> SyntaxTree {
    let lexed = lex_recovering(source);
    let green = build_green(source, &lexed.tokens, item_spans);
    SyntaxTree {
        green,
        text: source.to_string(),
    }
}

/// Lower CST → AST via the recursive-descent parser on the CST's authoritative text.
///
/// Typeck/resolve continue to consume [`crate::Program`]; the **source of truth**
/// is the CST (text + structure). There is no independent dual parse of spans from AST.
pub fn lower_syntax_to_program(
    tree: &SyntaxTree,
    file_id: u32,
) -> Result<crate::Program, crate::ParseError> {
    // RD lower on CST authoritative text (does not re-enter parse_syntax).
    let output = crate::parser::parse_tokens_to_program(tree.text(), file_id);
    if let Some(err) = output.diagnostics.into_iter().next() {
        Err(err)
    } else {
        Ok(output.program)
    }
}

/// Recovering lower (keeps parse diagnostics).
#[must_use]
pub fn lower_syntax_to_program_recovering(tree: &SyntaxTree, file_id: u32) -> crate::ParseOutput {
    crate::parser::parse_tokens_to_program(tree.text(), file_id)
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
            .filter(|(s, e)| e > s && (*s as usize) <= source.len())
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

/// Heuristic top-level item ranges from tokens (brace-aware).
///
/// Starts an item at module/import/decl keywords (optionally after `public`)
/// and extends until the next top-level keyword at depth 0 or EOF.
#[must_use]
pub fn find_top_level_item_spans(tokens: &[Token], source_len: u32) -> Vec<(u32, u32)> {
    let mut starts: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        if matches!(tok.kind, TokenKind::Eof | TokenKind::Error(_)) {
            i += 1;
            continue;
        }
        let is_public = matches!(tok.kind, TokenKind::KwPublic);
        let kw_i = if is_public { i + 1 } else { i };
        if kw_i >= tokens.len() {
            break;
        }
        let kw = &tokens[kw_i];
        if is_item_start_keyword(kw.kind) {
            starts.push(if is_public { tok.start } else { kw.start });
            i = kw_i + 1;
            continue;
        }
        i += 1;
    }

    if starts.is_empty() {
        return Vec::new();
    }

    let mut spans = Vec::with_capacity(starts.len());
    for (idx, &start) in starts.iter().enumerate() {
        let end = if idx + 1 < starts.len() {
            starts[idx + 1]
        } else {
            // End of last item: last non-eof token end or source_len
            tokens
                .iter()
                .rev()
                .find(|t| !matches!(t.kind, TokenKind::Eof))
                .map(|t| t.start.saturating_add(t.len))
                .unwrap_or(source_len)
                .min(source_len)
        };
        if end > start {
            spans.push((start, end));
        }
    }
    spans
}

fn is_item_start_keyword(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::KwModule
            | TokenKind::KwImport
            | TokenKind::KwFrom
            | TokenKind::KwFunc
            | TokenKind::KwStruct
            | TokenKind::KwEnum
            | TokenKind::KwInterface
            | TokenKind::KwConst
            | TokenKind::KwType
            | TokenKind::KwExtern
            | TokenKind::KwAsync // async func
    )
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

pub(crate) fn map_token_kind(kind: TokenKind) -> SyntaxKind {
    use TokenKind::*;
    match kind {
        DocComment => SyntaxKind::COMMENT,
        IdentValue | KwSelf => SyntaxKind::IDENT,
        IdentType => SyntaxKind::TYPE_IDENT,
        IntDec | IntHex | IntBin | IntOct | Float => SyntaxKind::NUMBER,
        StringStart | StringText | StringEscape | InterpStart | InterpEnd | StringEnd
        | RawString | MultilineStringStart | MultilineStringEnd => SyntaxKind::STRING,
        Char => SyntaxKind::CHAR,
        TypeInt | TypeUint | TypeFloat | TypeI8 | TypeI16 | TypeI32 | TypeI64 | TypeU8
        | TypeU16 | TypeU32 | TypeU64 | TypeF32 | TypeF64 | TypeBool | TypeByte | TypeChar
        | TypeStr | TypeAny | TypeErr => SyntaxKind::TYPE_IDENT,
        BoolTrue | BoolFalse | Nil => SyntaxKind::KEYWORD,
        k if is_keyword_kind(k) => SyntaxKind::KEYWORD,
        Error(_) => SyntaxKind::ERROR_TOKEN,
        Eof => SyntaxKind::WHITESPACE,
        _ => SyntaxKind::PUNCT,
    }
}

fn is_keyword_kind(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::KwIf
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
            | TokenKind::KwPtr
            | TokenKind::KwAlloc
            | TokenKind::KwFree
            | TokenKind::KwDefer
            | TokenKind::KwErrdefer
            | TokenKind::KwLet
    )
}

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

/// Full reparse after edit (always correct). Prefer [`reparse_subtree`] when possible.
#[must_use]
pub fn reparse_edit(
    old_source: &str,
    start: u32,
    end: u32,
    replacement: &str,
    _item_spans: &[(u32, u32)],
) -> (String, SyntaxTree) {
    let new_source = apply_text_edit(old_source, start, end, replacement);
    (new_source.clone(), parse_syntax(&new_source))
}

/// Reparse **only the ITEM subtree** covering the edit when the edit stays inside one item.
///
/// Algorithm:
/// 1. Apply the text edit.
/// 2. If the edit range is contained in a single [`SyntaxKind::ITEM`], re-lex **only that
///    ITEM's new text**, rebuild its green node, and [`GreenNodeData::replace_child`] on the
///    root so **sibling ITEM green nodes are reused** (cheap `Arc` clone).
/// 3. Otherwise fall back to full [`parse_syntax`].
///
/// Salsa's `syntax_tree` query still rebuilds from full text (correctness under arbitrary
/// edits); this API is for IDE buffers and tests that need local green reuse.
#[must_use]
pub fn reparse_subtree(
    old: &SyntaxTree,
    edit_start: u32,
    edit_end: u32,
    replacement: &str,
) -> (String, SyntaxTree) {
    let new_source = apply_text_edit(old.text(), edit_start, edit_end, replacement);
    let delta = replacement.len() as i64 - (edit_end.saturating_sub(edit_start) as i64);

    let old_items = old.items();
    let Some(idx) = old.item_index_at(edit_start) else {
        return (new_source.clone(), parse_syntax(&new_source));
    };
    let old_range = old_items[idx].text_range();
    let old_s = u32::from(old_range.start());
    let old_e = u32::from(old_range.end());
    if edit_start < old_s || edit_end > old_e {
        return (new_source.clone(), parse_syntax(&new_source));
    }

    let new_s = old_s;
    let Some(new_e) = (old_e as i64).checked_add(delta) else {
        return (new_source.clone(), parse_syntax(&new_source));
    };
    if new_e < new_s as i64 || new_e as usize > new_source.len() {
        return (new_source.clone(), parse_syntax(&new_source));
    }
    let new_e = new_e as u32;

    // Locate the ITEM among root green children (trivia siblings stay put).
    let old_root = old.root();
    let root_green = old_root.green();
    let item_kind: rowan::SyntaxKind = SyntaxKind::ITEM.into();
    let mut seen = 0usize;
    let mut child_index = None;
    for (i, child) in root_green.children().enumerate() {
        if let NodeOrToken::Node(n) = child
            && n.kind() == item_kind
        {
            if seen == idx {
                child_index = Some(i);
                break;
            }
            seen += 1;
        }
    }
    let Some(child_index) = child_index else {
        return (new_source.clone(), parse_syntax(&new_source));
    };

    // Re-lex + rebuild green for ONLY the edited ITEM slice.
    let item_text = &new_source[new_s as usize..new_e as usize];
    let lexed = lex_recovering(item_text);
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(SyntaxKind::ITEM.into());
    emit_tokens(
        &mut builder,
        item_text,
        &lexed.tokens,
        0,
        item_text.len() as u32,
    );
    builder.finish_node();
    let new_item = builder.finish();

    let new_green = root_green.replace_child(child_index, NodeOrToken::Node(new_item));
    let patched_root = SyntaxNode::new_root(new_green.clone());
    if patched_root.text().to_string() != new_source {
        return (new_source.clone(), parse_syntax(&new_source));
    }

    let tree = SyntaxTree {
        green: new_green,
        text: new_source.clone(),
    };
    // If a local edit introduced/removed top-level items (rare), prefer full structure.
    if tree.items().len() != old_items.len() {
        return (new_source.clone(), parse_syntax(&new_source));
    }

    (new_source, tree)
}

/// Highlight spans for LSP semantic tokens: `(start, end, class)`.
#[must_use]
pub fn highlight_spans(tree: &SyntaxTree) -> Vec<(u32, u32, &'static str)> {
    let mut out = Vec::new();
    let root = tree.root();
    for event in root.preorder_with_tokens() {
        let rowan::WalkEvent::Enter(el) = event else {
            continue;
        };
        let NodeOrToken::Token(tok) = el else {
            continue;
        };
        let Some(class) = tok.kind().highlight_class() else {
            continue;
        };
        let r = tok.text_range();
        out.push((u32::from(r.start()), u32::from(r.end()), class));
    }
    out
}

/// Iterate tokens for semantic highlighting.
pub fn for_each_highlight_token(tree: &SyntaxTree, mut f: impl FnMut(SyntaxToken, &'static str)) {
    let root = tree.root();
    for event in root.preorder_with_tokens() {
        let rowan::WalkEvent::Enter(el) = event else {
            continue;
        };
        let NodeOrToken::Token(tok) = el else {
            continue;
        };
        if let Some(class) = tok.kind().highlight_class() {
            f(tok, class);
        }
    }
}

#[must_use]
pub fn text_range(start: u32, end: u32) -> TextRange {
    TextRange::new(TextSize::from(start), TextSize::from(end))
}

#[allow(dead_code)]
fn _lang() -> AranduLanguage {
    unreachable!()
}
