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
    → syntax_tree(file)          // CST rowan, ITEM por heurística de keywords
    → parse(file)                // lower_syntax_to_program (RD no texto do CST)
    → resolve / item_body_typeck / file_ide_diagnostics
```

- **Não** há dual: CST não depende de AST; AST é só lower do texto autoritativo do CST.
- Typeck continua em `Program` (estrutura tipada); a **origem** do programa é o CST.
- Highlighting LSP: `textDocument/semanticTokens/full` via `highlight_spans` no CST.

## Reparse de subtree

`reparse_subtree(old_tree, start, end, replacement)`:

1. Aplica o edit no texto.  
2. Se o edit cabe em um único `ITEM`: re-lex **só o texto daquele ITEM**, reconstrói o green do ITEM, e faz `replace_child` na raiz — **irmãos reusam o mesmo green** (`Arc` identity).  
3. Caso contrário, full `parse_syntax`.  

No path Salsa, `syntax_tree` reconstroi a partir do texto completo (correto sob edits arbitrários); a API de subtree serve buffers/IDE e testes de estabilidade/reuso.

## P6 — Hardening (feito)

| Entrega | Onde |
|---------|------|
| Docs CST-first | este plano, `arandu-salsa-lsp-architecture-v0.1.md`, README |
| Explain-rebuild | `RebuildLog` + `#[tracing::instrument]` em `syntax_tree` / `item_body_typeck` / `parse` |
| Métricas de pipeline | contadores de teste (`item_body_cutoff`, `ide_diag_delta`, green identity em `reparse_subtree`) |
| Critério de sucesso | edit em um item não muda texto/green de irmãos; typeck + semantic tokens verdes |

## Fora de escopo residual

- Lower AST **sem** re-lex RD (parser 100% driven por walk de green nodes).  
- `syntax_tree` Salsa incremental via `reparse_subtree` (hoje full text → green).  
- Highlight semântico por tipos (só léxico via CST por ora).  
- Code actions / format.
