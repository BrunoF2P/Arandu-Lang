# Arandu — Salsa, LSP e Identidades (v0.1)

**Status:** gold path implementado (F0–F3 + F5). F4 (delta por bloco) opcional/futuro.  
**Plano:** [`arandu-salsa-lsp-gold-plan-v0.1.md`](./arandu-salsa-lsp-gold-plan-v0.1.md).  
**Dono do grafo de queries:** `arandu_query` apenas.

## Salsa toca / não toca

| Crate | Papel | Salsa? |
|-------|--------|--------|
| `arandu_query` | `ArandCompilerDb`, `DatabaseImpl`, `SourceFile`, `AnalysisHost`, `#[salsa::tracked]` | **Dono** |
| `arandu_middle` | `SourceDatabase` trait, tipos, AMIR/HIR, IDs densos | Interface + dados |
| `arandu_resolve` / `arandu_typeck` / `arandu_mir` | Lógica pura; fronteira só | Fronteira só |
| `arandu_lexer` / `arandu_parser` / `arandu_base` / backends | Puros | **Nunca** |
| `arandu_cli` / `arandu_lsp` | Orquestram DB + edits | Cliente do grafo |

### Queries tracked

| Query | Estado |
|-------|--------|
| `parse`, `resolve`, `module_signatures`, `type_check`, `lower_amir` | Reais |
| `local_symbols`, `exported_symbols`, `func_amir` | Reais |
| `liveness_facts` | Real (`arandu_mir::liveness`) |
| `block_dataflow_facts` | live/init/moved/stmt counts por bloco |
| `func_analysis_diags` / `block_diagnostics` / `file_ide_diagnostics` | F4 — diags IDE memoizados |
| DX.5 `RebuildLog` | Opt-in (`-Zexplain-rebuild`) |

### I/O de fonte

- typeck/resolve: proibido `fs::read` (guardrail `architecture_invariants`).
- Registro: CLI / LSP / `DatabaseImpl::resolve_module_path` (fallback disco **só** na DB).
- Workers LSP **não** registram arquivos; só a main.

## Três identidades

| ID | Geracional? | Função |
|----|-------------|--------|
| `DocumentId` (`slotmap`) | **Sim** | Buffer LSP; close → stale |
| `FileId` + densos | **Não** | Análise na revisão atual |
| `AnalysisRevision` | Sim (host) | Handles IDE não atravessam edit |

`LspSymbolId { symbol, revision }` — resolve só se `revision == snap.revision`.

**Deadlock Salsa:** nunca segurar `AnalysisSnapshot` / clone de `DatabaseImpl` na **mesma** thread que chama `set_text` (Storage espera clones == 1).

## Legado

| Item | Status |
|------|--------|
| `CompileSession` | **Removido** |
| `symbol_span` dummy | **Span real** + `try_get` safe |
| tower-lsp / tokio no path de query | **Removidos** do `arandu_lsp` |

## LSP gold (implementado)

1. Main síncrona (`lsp-server`) + `Vfs` debounce 100 ms.  
2. Workers: `AnalysisSnapshot` (clone Storage) → diags/goto; publish só se DocumentId vivo e revision match.  
3. didChange **não** commita Salsa por tecla; flush no debounce / didSave / goto.  
4. Diagnostics via `file_ide_diagnostics` (F4); fingerprint blake3 evita republish no-op.  
5. D7: reparse completo do arquivo editado (sem Rowan).

## F4 / P3 — delta on-type

- `block_dataflow_facts`: live/init/moved/stmt por bloco.  
- **`item_ide_diagnostics`**: diags de typeck **por item** (`item_body_typeck`) + AMIR se func.  
- **`file_ide_diagnostics`**: union barata dos memos de item + signatures.  
- Early cutoff entre itens (testes `item_body_cutoff`, `ide_diag_delta`).  
- Typeck monólito substituído por compose P1/P2; wire LSP ainda manda lista full (protocolo).

## P5 — CST (rowan)

- `arandu_parser::syntax`: green tree `SOURCE_FILE` / `ITEM` / tokens.  
- Dual: `parse_dual` / Salsa `syntax_tree` (ITEM spans do AST).  
- `reparse_edit` + texto de ITEM estável quando só o irmão muda.  
- `item_source_input` fingerprinta texto do ITEM CST (`item_body_v3_cst`).

## Guardrails / testes

- `architecture_invariants`, `doc_store` stale, `analysis` revision stale, `vfs` debounce, `block_delta`.
