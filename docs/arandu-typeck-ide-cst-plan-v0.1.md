# Plano — typeck incremental fino, IDE completa, delta de diag, CST

**Status:** plano em execução — **P0 + P1 (MVP typeck por função) no tree**.  
**Pré-requisito já no tree:** gold Salsa/LSP (F0–F5) — ver [`arandu-salsa-lsp-architecture-v0.1.md`](./arandu-salsa-lsp-architecture-v0.1.md).

### Progresso

| Fase | Status |
|------|--------|
| P0 baseline/doc | Feito (este doc + contadores) |
| P1 typeck por função | Feito: fingerprint por span + `item_body_typeck` |
| P2 typeck por item | **Feito:** `body_item_symbols` + `check_item_body_only` + `item_source_input` |
| P3 delta diags | **Feito:** `item_ide_diagnostics` + compose; teste `ide_diag_delta` |
| P4 IDE completa | **Feito:** hover, complete, signatureHelp, refs, rename, doc/workspace symbols; worker pool |
| Honestidade bloco AMIR | **Feito:** `check_*_by_block` tagueia diags com `BlockId` real |
| P5 CST Rowan | **Feito:** dual `parse_dual` / `syntax_tree` Salsa + `reparse_edit`; ITEM green reuse |
| P6 | Pendente (métricas extras) |

## Por que agora

O typeck ainda é **por arquivo** (`type_check(file)` monólito). Isso limita:

- latência on-type em arquivos grandes  
- delta real de diagnostics de typeck  
- hover/complete baratos  
- o ganho de um CST/reparse parcial (parse barato, análise ainda cara)

## Ordem (DAG)

```
P0 baseline/contratos
 → P1 typeck por função + file_typeck_view
 → P2 typeck por item (structs/methods/…)
 → P3 delta diags (item + bloco AMIR com spans)
 → P4 IDE completa (hover, complete, refs, rename, …)
 → P5 CST Rowan (dual → reparse parcial)
 → P6 hardening / métricas / roadmap
```

**Não** fazer CST (P5) antes de typeck fino (P1): reparse parcial sem fatiar typeck é cosmético.

## Unidades de invalidação

| Unidade | Chave | Notas |
|---------|-------|--------|
| File text | `SourceFile` | hoje |
| **Item** | `SymbolId` da decl | P1+ — unidade mínima de typeck fino |
| Block AMIR | `(SymbolId, BlockId)` | dataflow + diags com span |
| Syntax node | green id (P5) | reparse parcial |

Stale IDE handles: `AnalysisRevision` (já existe). Não `generation` em `SymbolId`.

## P1 em uma frase

```text
module_signatures(file)           // parede de tipos exportados
item_body_typeck(file, func_sym)  // só aquele corpo
file_typeck_view(file)            // compose → CLI/LSP/lower
```

**DoD:** edit em `beta` não WillExecute `item_body_typeck(alpha)`.

## IDE (P4) — ordem

1. Hover  
2. Completion  
3. Signature help  
4. References  
5. Rename  
6. Document / workspace symbols  
7. Code actions / format (depois)

Infra: pool de threads, cancel Salsa, harness JSON-RPC mínimo.

## CST (P5)

- Lib recomendada: **rowan**  
- Dual running (CST + AST atual) → reparse parcial → dropar dual  
- CST **não** substitui P1

## Estimativa

| Caminho | Esforço |
|---------|---------|
| Critico até IDE boa (P0–P1–P3–P4.1) | ~5–8 semanas foco |
| + CST reparse parcial | ~8–14 semanas foco |

## DoD global

1. Typeck fino medido (cutoff entre funções/itens).  
2. Delta diag por item (+ bloco quando span existir).  
3. IDE usável: hover, complete, goto, refs, rename, symbols, diags.  
4. CST + reparse parcial medido.  
5. Docs honestas (sem “5 ms” sem medição).

Detalhe completo de fases, riscos e testes: plano de sessão / implementação aprovado na conversa de design.
