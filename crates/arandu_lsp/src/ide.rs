//! IDE analysis helpers over a frozen [`AnalysisSnapshot`] (P4).
//!
//! Pure queries on typeck/resolve results — no Salsa writes.

use arandu_base::LineIndex;
use arandu_middle::{NodeKey, SymbolId, SymbolKind};
use arandu_query::{AnalysisSnapshot, SourceFile};
use arandu_semantics::TypeCheckResult;
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionResponse, CompletionItem,
    CompletionItemKind, Documentation, Hover, HoverContents, Location, MarkedString, Position,
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensLegend,
    SymbolInformation, SymbolKind as LspSymbolKind, TextEdit as LspTextEdit, Url, WorkspaceEdit,
};
use rustc_hash::FxHashMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::conv::{position_to_offset, span_to_range};

/// Snapshot of open docs for multi-file IDE features.
#[derive(Clone)]
pub struct DocSnap {
    pub source: SourceFile,
    pub path: Arc<PathBuf>,
    pub uri: Url,
}

/// Type-check the file (composed P1/P2 view).
#[must_use]
pub fn typecheck(
    snap: &AnalysisSnapshot,
    source: SourceFile,
) -> arandu_query::db::HashEq<TypeCheckResult> {
    arandu_query::passes::type_check(&snap.db, source)
}

fn ty_str(t: &arandu_middle::types::ArType) -> String {
    format!("{t:?}")
}

/// Tightest name/ref/definition containing `offset`.
#[must_use]
pub fn symbol_at(tc: &TypeCheckResult, offset: u32) -> Option<SymbolId> {
    let mut best: Option<(u32, SymbolId)> = None;
    let consider = |map: &FxHashMap<NodeKey, SymbolId>, best: &mut Option<(u32, SymbolId)>| {
        for (key, &sym) in map {
            if key.start <= offset && offset < key.end {
                let w = key.end.saturating_sub(key.start);
                if best.is_none_or(|(bw, _)| w < bw) {
                    *best = Some((w, sym));
                }
            }
        }
    };
    consider(&tc.resolved.value_refs, &mut best);
    consider(&tc.resolved.type_refs, &mut best);
    consider(&tc.resolved.definitions, &mut best);
    best.map(|(_, s)| s)
}

/// Word prefix before `offset` for completion filtering.
#[must_use]
pub fn prefix_at(text: &str, offset: u32) -> String {
    let off = (offset as usize).min(text.len());
    let bytes = text.as_bytes();
    let mut i = off;
    while i > 0 {
        let c = bytes[i - 1];
        if c.is_ascii_alphanumeric() || c == b'_' {
            i -= 1;
        } else {
            break;
        }
    }
    text[i..off].to_string()
}

#[must_use]
pub fn hover(
    snap: &AnalysisSnapshot,
    source: SourceFile,
    text: &str,
    position: Position,
) -> Option<Hover> {
    let index = LineIndex::new(text);
    let offset = position_to_offset(&index, position, text);
    let tc = typecheck(snap, source);
    let sym = symbol_at(&tc, offset)?;
    let symbol = tc.symbols.try_get(sym)?;
    let ty = tc
        .type_info
        .decl_type(sym)
        .map(|t| ty_str(&t))
        .unwrap_or_else(|| "?".into());
    let kind = format!("{:?}", symbol.kind);
    let name = symbol.name.to_string();
    let md = format!("```arandu\n{name}: {ty}\n```\n\n_{kind}_ (`{sym:?}`)");
    let range = span_to_range(&index, symbol.span);
    Some(Hover {
        contents: HoverContents::Scalar(MarkedString::String(md)),
        range: Some(range),
    })
}

#[must_use]
pub fn completions(
    snap: &AnalysisSnapshot,
    source: SourceFile,
    text: &str,
    position: Position,
) -> Vec<CompletionItem> {
    let index = LineIndex::new(text);
    let offset = position_to_offset(&index, position, text);
    let prefix = prefix_at(text, offset);
    let prefix_l = prefix.to_ascii_lowercase();
    let tc = typecheck(snap, source);

    let mut items = Vec::new();
    // Keywords
    for kw in [
        "func",
        "struct",
        "enum",
        "const",
        "let",
        "mut",
        "set",
        "return",
        "if",
        "else",
        "match",
        "import",
        "module",
        "true",
        "false",
        "nil",
        "err",
        "interface",
        "extern",
    ] {
        if prefix.is_empty() || kw.starts_with(&prefix_l) || kw.starts_with(&prefix) {
            items.push(CompletionItem {
                label: kw.into(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..CompletionItem::default()
            });
        }
    }
    // Symbols from the file's table
    for symbol in tc.symbols.iter() {
        let name = symbol.name.to_string();
        if !prefix.is_empty()
            && !name.to_ascii_lowercase().starts_with(&prefix_l)
            && !name.starts_with(&prefix)
        {
            continue;
        }
        let kind = match symbol.kind {
            SymbolKind::Func | SymbolKind::AssociatedFunc | SymbolKind::ExternFunc => {
                CompletionItemKind::FUNCTION
            }
            SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Interface
            | SymbolKind::TypeAlias => CompletionItemKind::STRUCT,
            SymbolKind::Const => CompletionItemKind::CONSTANT,
            SymbolKind::Field | SymbolKind::EnumVariant => CompletionItemKind::FIELD,
            SymbolKind::Param | SymbolKind::Local => CompletionItemKind::VARIABLE,
            _ => CompletionItemKind::TEXT,
        };
        let detail = tc.type_info.decl_type(symbol.id).map(|t| ty_str(&t));
        items.push(CompletionItem {
            label: name,
            kind: Some(kind),
            detail,
            documentation: Some(Documentation::String(format!("{:?}", symbol.kind))),
            ..CompletionItem::default()
        });
    }
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items.dedup_by(|a, b| a.label == b.label);
    items.truncate(200);
    items
}

#[must_use]
pub fn references(
    snap: &AnalysisSnapshot,
    source: SourceFile,
    text: &str,
    position: Position,
    uri: &Url,
) -> Vec<Location> {
    let index = LineIndex::new(text);
    let offset = position_to_offset(&index, position, text);
    let tc = typecheck(snap, source);
    let Some(sym) = symbol_at(&tc, offset) else {
        return Vec::new();
    };
    let mut locs = Vec::new();
    let push_key = |key: &NodeKey, locs: &mut Vec<Location>| {
        let span = arandu_base::Span::new(sym.file_id, key.start, key.end);
        locs.push(Location {
            uri: uri.clone(),
            range: span_to_range(&index, span),
        });
    };
    for (key, &s) in &tc.resolved.definitions {
        if s == sym {
            push_key(key, &mut locs);
        }
    }
    for (key, &s) in &tc.resolved.value_refs {
        if s == sym {
            push_key(key, &mut locs);
        }
    }
    for (key, &s) in &tc.resolved.type_refs {
        if s == sym {
            push_key(key, &mut locs);
        }
    }
    locs.sort_by_key(|l| (l.range.start.line, l.range.start.character));
    locs.dedup_by(|a, b| a.range == b.range);
    locs
}

#[must_use]
pub fn rename_edits(
    snap: &AnalysisSnapshot,
    source: SourceFile,
    text: &str,
    position: Position,
    uri: &Url,
    new_name: &str,
) -> Option<lsp_types::WorkspaceEdit> {
    let locs = references(snap, source, text, position, uri);
    if locs.is_empty() {
        return None;
    }
    let mut changes: HashMap<Url, Vec<lsp_types::TextEdit>> = HashMap::new();
    for loc in locs {
        changes
            .entry(loc.uri)
            .or_default()
            .push(lsp_types::TextEdit {
                range: loc.range,
                new_text: new_name.to_string(),
            });
    }
    Some(lsp_types::WorkspaceEdit {
        changes: Some(changes),
        ..lsp_types::WorkspaceEdit::default()
    })
}

#[must_use]
#[allow(deprecated)] // SymbolInformation::deprecated field in lsp-types 0.94
pub fn document_symbols(
    snap: &AnalysisSnapshot,
    source: SourceFile,
    text: &str,
    uri: &Url,
) -> Vec<SymbolInformation> {
    let index = LineIndex::new(text);
    let tc = typecheck(snap, source);
    let mut out = Vec::new();
    for symbol in tc.symbols.iter() {
        // Top-level-ish: global scope or methods
        let kind = match symbol.kind {
            SymbolKind::Func | SymbolKind::AssociatedFunc | SymbolKind::ExternFunc => {
                LspSymbolKind::FUNCTION
            }
            SymbolKind::Struct => LspSymbolKind::STRUCT,
            SymbolKind::Enum => LspSymbolKind::ENUM,
            SymbolKind::Interface => LspSymbolKind::INTERFACE,
            SymbolKind::Const => LspSymbolKind::CONSTANT,
            SymbolKind::TypeAlias => LspSymbolKind::TYPE_PARAMETER,
            SymbolKind::Field => LspSymbolKind::FIELD,
            SymbolKind::EnumVariant => LspSymbolKind::ENUM_MEMBER,
            _ => continue,
        };
        let range = span_to_range(&index, symbol.span);
        out.push(SymbolInformation {
            name: symbol.name.to_string(),
            kind,
            tags: None,
            deprecated: None,
            location: Location {
                uri: uri.clone(),
                range,
            },
            container_name: None,
        });
    }
    out
}

#[must_use]
#[allow(deprecated)]
pub fn workspace_symbols(
    snap: &AnalysisSnapshot,
    docs: &[DocSnap],
    query: &str,
) -> Vec<SymbolInformation> {
    let q = query.to_ascii_lowercase();
    let mut out = Vec::new();
    for doc in docs {
        let text = doc.source.text(&snap.db);
        let index = LineIndex::new(&text);
        let tc = typecheck(snap, doc.source);
        for symbol in tc.symbols.iter() {
            let name = symbol.name.to_string();
            if !q.is_empty() && !name.to_ascii_lowercase().contains(&q) {
                continue;
            }
            let kind = match symbol.kind {
                SymbolKind::Func | SymbolKind::AssociatedFunc | SymbolKind::ExternFunc => {
                    LspSymbolKind::FUNCTION
                }
                SymbolKind::Struct => LspSymbolKind::STRUCT,
                SymbolKind::Enum => LspSymbolKind::ENUM,
                SymbolKind::Interface => LspSymbolKind::INTERFACE,
                SymbolKind::Const => LspSymbolKind::CONSTANT,
                _ => continue,
            };
            out.push(SymbolInformation {
                name,
                kind,
                tags: None,
                deprecated: None,
                location: Location {
                    uri: doc.uri.clone(),
                    range: span_to_range(&index, symbol.span),
                },
                container_name: Some(doc.path.display().to_string()),
            });
        }
    }
    out.truncate(200);
    out
}

#[must_use]
pub fn signature_help(
    snap: &AnalysisSnapshot,
    source: SourceFile,
    text: &str,
    position: Position,
) -> Option<lsp_types::SignatureHelp> {
    // Minimal: show type of symbol under / before cursor if it's a function.
    let index = LineIndex::new(text);
    let offset = position_to_offset(&index, position, text);
    // Walk back to an identifier near `(`
    let mut probe = offset.saturating_sub(1);
    let bytes = text.as_bytes();
    while probe > 0
        && (bytes[probe as usize].is_ascii_whitespace() || bytes[probe as usize] == b'(')
    {
        probe -= 1;
    }
    let tc = typecheck(snap, source);
    let sym = symbol_at(&tc, probe)?;
    let symbol = tc.symbols.try_get(sym)?;
    if !matches!(
        symbol.kind,
        SymbolKind::Func | SymbolKind::AssociatedFunc | SymbolKind::ExternFunc
    ) {
        return None;
    }
    let ty = tc
        .type_info
        .decl_type(sym)
        .map(|t| ty_str(&t))
        .unwrap_or_else(|| "func".into());
    let label = format!("{}: {}", symbol.name, ty);
    Some(lsp_types::SignatureHelp {
        signatures: vec![lsp_types::SignatureInformation {
            label,
            documentation: None,
            parameters: None,
            active_parameter: None,
        }],
        active_signature: Some(0),
        active_parameter: None,
    })
}

/// Legend order for semantic tokens (must match [`arandu_query::HlKind`] discriminant).
///
/// Modifiers bit order: declaration=0, modification/mutable=1, definition=2 (F2b).
#[must_use]
pub fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::KEYWORD,     // 0
            SemanticTokenType::FUNCTION,    // 1
            SemanticTokenType::VARIABLE,    // 2
            SemanticTokenType::PARAMETER,   // 3
            SemanticTokenType::TYPE,        // 4
            SemanticTokenType::STRUCT,      // 5
            SemanticTokenType::ENUM,        // 6
            SemanticTokenType::INTERFACE,   // 7
            SemanticTokenType::NAMESPACE,   // 8
            SemanticTokenType::NUMBER,      // 9
            SemanticTokenType::STRING,      // 10
            SemanticTokenType::COMMENT,     // 11
            SemanticTokenType::OPERATOR,    // 12
            SemanticTokenType::PROPERTY,    // 13
            SemanticTokenType::ENUM_MEMBER, // 14 Constant
        ],
        token_modifiers: vec![
            SemanticTokenModifier::DECLARATION,  // bit 0 MOD_DECLARATION
            SemanticTokenModifier::MODIFICATION, // bit 1 MOD_MUTABLE (closest to mut)
            SemanticTokenModifier::DEFINITION,   // bit 2 MOD_DEFINITION
        ],
    }
}

/// Format entire document (F3a) → LSP text edits (usually one full replace).
#[must_use]
pub fn format_document(text: &str) -> Vec<LspTextEdit> {
    let edits = arandu_fmt::format_edits(text);
    if edits.is_empty() {
        return Vec::new();
    }
    let index = LineIndex::new(text);
    edits
        .into_iter()
        .map(|e| {
            let start = offset_to_position_local(&index, e.start);
            let end = offset_to_position_local(&index, e.end);
            LspTextEdit {
                range: lsp_types::Range { start, end },
                new_text: e.new_text,
            }
        })
        .collect()
}

/// Code actions from diagnostic messages (`;`, braces, parens).
#[must_use]
pub fn code_actions(uri: &Url, context: &lsp_types::CodeActionContext) -> CodeActionResponse {
    let mut out = Vec::new();
    for d in &context.diagnostics {
        let actions = arandu_fmt::actions_for_diagnostic(0, 0, d.message.as_str());
        for a in actions {
            let start = d.range.start;
            let new_text = a
                .edits
                .first()
                .map(|e| e.new_text.clone())
                .unwrap_or_default();
            let mut changes = HashMap::new();
            changes.insert(
                uri.clone(),
                vec![LspTextEdit {
                    range: lsp_types::Range { start, end: start },
                    new_text,
                }],
            );
            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: a.title.into(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![d.clone()]),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..WorkspaceEdit::default()
                }),
                ..CodeAction::default()
            }));
        }
    }
    out
}

/// Build LSP semantic tokens from type-aware [`arandu_query::file_highlights`].
#[must_use]
pub fn semantic_tokens(snap: &AnalysisSnapshot, source: SourceFile) -> SemanticTokens {
    encode_highlights(
        &arandu_query::file_highlights(&snap.db, source),
        &source.text(&snap.db),
    )
}

/// Range semantic tokens (F2b).
#[must_use]
pub fn semantic_tokens_range(
    snap: &AnalysisSnapshot,
    source: SourceFile,
    range_start: u32,
    range_end: u32,
) -> SemanticTokens {
    let all = arandu_query::file_highlights(&snap.db, source);
    let slice = arandu_query::highlights_in_range(&all, range_start, range_end);
    encode_highlights(&slice, &source.text(&snap.db))
}

fn encode_highlights(highlights: &[arandu_query::HlToken], text: &str) -> SemanticTokens {
    let index = LineIndex::new(text);
    let mut data = Vec::with_capacity(highlights.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;
    for hl in highlights {
        let token_type = hl.kind.legend_index();
        let start_pos = offset_to_position_local(&index, hl.start);
        let length = hl.end.saturating_sub(hl.start);
        let delta_line = start_pos.line.saturating_sub(prev_line);
        let delta_start = if delta_line == 0 {
            start_pos.character.saturating_sub(prev_start)
        } else {
            start_pos.character
        };
        data.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type,
            token_modifiers_bitset: u32::from(hl.mods),
        });
        prev_line = start_pos.line;
        prev_start = start_pos.character;
    }
    SemanticTokens {
        result_id: None,
        data,
    }
}

fn offset_to_position_local(index: &LineIndex, offset: u32) -> Position {
    let (line1, col1) = index.line_col(offset);
    Position {
        line: line1.saturating_sub(1),
        character: col1.saturating_sub(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arandu_query::AnalysisHost;

    #[test]
    fn prefix_at_identifier() {
        assert_eq!(prefix_at("let foo_bar = 1", 11), "foo_bar");
        assert_eq!(prefix_at("io.", 3), "");
    }

    #[test]
    fn completions_include_func_and_keyword() {
        let mut host = AnalysisHost::new();
        let file = host.new_file("h.aru".into(), "func main(): int { return 1 }\n".into());
        let snap = host.snapshot();
        let text = file.text(&snap.db);
        let items = completions(
            &snap,
            file,
            &text,
            Position {
                line: 0,
                character: text.len() as u32,
            },
        );
        assert!(
            items.iter().any(|i| i.label == "func" || i.label == "main"),
            "expected keyword or main in completions, got {} items",
            items.len()
        );
    }

    #[test]
    fn semantic_tokens_from_cst_nonempty() {
        let mut host = AnalysisHost::new();
        let file = host.new_file("st.aru".into(), "func main(): int { return 1 }\n".into());
        let snap = host.snapshot();
        let tokens = semantic_tokens(&snap, file);
        assert!(
            !tokens.data.is_empty(),
            "expected semantic tokens from CST keywords/idents"
        );
    }

    #[test]
    fn document_symbols_does_not_panic() {
        let mut host = AnalysisHost::new();
        let file = host.new_file("h.aru".into(), "func main(): int { return 1 }\n".into());
        let snap = host.snapshot();
        let text = file.text(&snap.db);
        let uri = Url::parse("file:///h.aru").unwrap();
        let _syms = document_symbols(&snap, file, &text, &uri);
        // Table population depends on resolve+typeck paths; smoke only.
    }
}
