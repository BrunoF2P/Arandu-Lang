# Plano — typeck incremental fino, IDE completa, delta de diag, CST

**Status:** **P0–P6 implementados** (CST-first, sem dual AST independente).  
**Pré-requisito:** gold Salsa/LSP — [`arandu-salsa-lsp-architecture-v0.1.md`](./arandu-salsa-lsp-architecture-v0.1.md).

## Progresso

| Fase | Status |
|------|--------|
| P0 baseline/doc | Feito |
| P1 typeck por função | Feito |
| P2 typeck por item | Feito |
| P3 delta diags | Feito |
| P4 IDE completa | Feito (+ semantic tokens CST) |
| Honestidade bloco AMIR | Feito (`*_by_block`) |
| **P5 CST Rowan** | **CST-first:** `syntax_tree` → `parse` lower; `reparse_subtree` |
| **P6 hardening** | Docs + métricas de pipeline; explain-rebuild cobre `syntax_tree` |

## Pipeline canônico (hoje)

```text
SourceFile.text
    → syntax_tree(file)          // CST rowan; cache + reparse_subtree em edit contíguo
    → parse(file)                // lower_syntax_to_program (RD nos tokens do CST, sem re-lex)
    → resolve / item_body_typeck / file_ide_diagnostics
```

- **Não** há dual: CST não depende de AST; AST é lower do token stream cacheado no CST.
- Typeck continua em `Program` (estrutura tipada); a **origem** do programa é o CST.
- Highlighting LSP: `textDocument/semanticTokens/full` via `highlight_spans` no CST.
- `;` opcional antes de `}` no parser (corpos one-line).

## Reparse de subtree

`reparse_subtree(old_tree, start, end, replacement)`:

1. Aplica o edit no texto.  
2. Se o edit cabe em um único `ITEM`: re-lex **só o texto daquele ITEM**, reconstrói o green do ITEM, e faz `replace_child` na raiz — **irmãos reusam o mesmo green** (`Arc` identity).  
3. Atualiza o token stream do arquivo (um lex do source novo) para o lower.  
4. Caso contrário, full `parse_syntax`.  

**Salsa:** `DatabaseImpl` mantém cache CST por `FileId`; em `syntax_tree`, se o texto mudou por um edit contíguo (`single_contiguous_edit`), usa `reparse_subtree`.

## P6 — Hardening (feito)

| Entrega | Onde |
|---------|------|
| Docs CST-first | este plano, `arandu-salsa-lsp-architecture-v0.1.md`, README |
| Explain-rebuild | `RebuildLog` + `#[tracing::instrument]` em `syntax_tree` / `item_body_typeck` / `parse` |
| Métricas de pipeline | contadores de teste (`item_body_cutoff`, `ide_diag_delta`, green identity em `reparse_subtree`) |
| Critério de sucesso | edit em um item não muda texto/green de irmãos; typeck + semantic tokens verdes |

## Residuais corrigidos

| Residual | Correção |
|----------|----------|
| Lower re-lex full | Tokens cacheados no `SyntaxTree`; `parse_token_stream` |
| `syntax_tree` sempre full | Cache + `reparse_subtree` em edit contíguo |
| One-line `return 1 }` | `;` opcional antes de `RBRACE`/`EOF` no parser |
| Heap H0 | Arc tokens/text, token splice, `Arc<Program>`, `HashEq::share` |
| Highlight tipado (F2a) | `file_highlights` + legend LSP (`function`/`parameter`/…) |
| Format + code action (F3) | crate `arandu_fmt`; CLI `fmt`; LSP formatting + quickfix `;` |

## F1 — green estrutural + event sink + lower guiado (feito)

- **Event sink:** RD emite `ParseEvent::{Start,Token,Finish}` → `build_green_from_events` (gaps → WHITESPACE).  
- Kinds: `FUNC_ITEM` / … + `BLOCK` + `STMT` emitidos no parse (não só heurística).  
- Fallback heurístico se eventos desbalanceados.  
- Lower: walk nos itens green + RD com seek; fallback full RD se incompleto.  
- `inspect_green_structure` conta funcs/blocks/stmts.

## Em progresso / próximo

- Lower AST **só** lendo green (sem RD de corpo) — EXPR nodes no green.  
- Format pretty-print por green (hoje: higiene de whitespace).
